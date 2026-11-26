//! Thin Windows 11 `LayoutController` (`T027`).
//!
//! Uses `GetGUIThreadInfo`, `GetKeyboardLayoutList` + Registry `HKL -> KLID`
//! resolution, `LCIDToLocaleName` for BCP-47 tags, and `WM_INPUTLANGCHANGEREQUEST`
//! with confirmation polling.

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::thread;
use std::time::{Duration, Instant};

use langflip_core::layout::{InstalledLayout, LayoutId};
use langflip_core::ports::{LayoutController, LayoutSwitchOutcome};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Globalization::{
    GetLocaleInfoEx, LCIDToLocaleName, LOCALE_SENGLISHDISPLAYNAME, LOCALE_SENGLISHLANGUAGENAME,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ, RegCloseKey, RegEnumKeyExW, RegOpenKeyExW,
    RegQueryValueExW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, GetKeyboardLayoutList, HKL,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_INPUTLANGCHANGEREQUEST};
use windows::core::{PCWSTR, PWSTR};

use crate::focus::foreground_gui_target;

/// Maps Windows `HKL` handles (stored as `usize` so this controller is `Send`)
/// to normalized `LayoutId` (KLID) strings and back.
pub struct WinLayoutController {
    hkl_to_id: HashMap<usize, LayoutId>,
    id_to_hkl: HashMap<LayoutId, usize>,
    installed: Vec<InstalledLayout>,
}

impl WinLayoutController {
    pub fn new() -> Self {
        let mut ctrl = Self {
            hkl_to_id: HashMap::new(),
            id_to_hkl: HashMap::new(),
            installed: Vec::new(),
        };
        ctrl.refresh_installed();
        ctrl
    }

    pub fn refresh_installed(&mut self) {
        self.hkl_to_id.clear();
        self.id_to_hkl.clear();
        self.installed.clear();

        let count = unsafe { GetKeyboardLayoutList(None) };
        if count <= 0 {
            return;
        }
        let mut hkls = vec![HKL::default(); count as usize];
        let filled = unsafe { GetKeyboardLayoutList(Some(&mut hkls)) };
        if filled <= 0 {
            return;
        }
        hkls.truncate(filled as usize);

        let reg_entries = load_registry_keyboard_layouts();
        let mut raw_items = Vec::new();
        for hkl in hkls {
            let raw = hkl.0 as usize;
            let lang_id = (raw & 0xFFFF) as u16;
            let device_id = ((raw >> 16) & 0xFFFF) as u16;

            let (klid, reg_text) = resolve_hkl_from_registry(lang_id, device_id, &reg_entries);
            let id = LayoutId::new(&klid);
            let language_tag = lcid_to_bcp47(lang_id);
            let friendly = friendly_locale_name(language_tag.as_deref(), &reg_text);

            self.hkl_to_id.insert(raw, id.clone());
            self.id_to_hkl.insert(id.clone(), raw);
            raw_items.push((id, language_tag, friendly, reg_text));
        }

        // Only disambiguate with the registry layout variant if two installed
        // layouts share the same friendly locale name.
        for (idx, (id, language_tag, friendly, reg_text)) in raw_items.iter().enumerate() {
            let duplicate = raw_items
                .iter()
                .enumerate()
                .any(|(j, (_, _, other_name, _))| j != idx && other_name == friendly);
            let display_name = if duplicate && !reg_text.is_empty() && reg_text != "<unknown>" {
                format!("{friendly} — {reg_text}")
            } else {
                friendly.clone()
            };
            self.installed.push(InstalledLayout {
                id: id.clone(),
                language_tag: language_tag.clone(),
                display_name,
            });
        }
    }

    /// Cheap, allocation-minimal lookup used inside the hook callback to tag each `KeyEvent`.
    pub fn layout_id_for_hkl(&self, hkl: HKL) -> LayoutId {
        let raw = hkl.0 as usize;
        if let Some(id) = self.hkl_to_id.get(&raw) {
            return id.clone();
        }
        let lang_id = (raw & 0xFFFF) as u16;
        let device_id = ((raw >> 16) & 0xFFFF) as u16;
        let id = if device_id != 0 && (device_id & 0xF000) != 0xF000 {
            device_id
        } else {
            lang_id
        };
        LayoutId::new(format!("{id:08X}"))
    }

    pub fn hkl_map_snapshot(&self) -> HashMap<usize, LayoutId> {
        self.hkl_to_id.clone()
    }
}

impl Default for WinLayoutController {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutController for WinLayoutController {
    type Error = String;

    fn active_layout(&self) -> Result<LayoutId, Self::Error> {
        let Some((_, focus_tid)) = foreground_gui_target() else {
            return Err("no foreground window".into());
        };
        let hkl = unsafe { GetKeyboardLayout(focus_tid) };
        Ok(self.layout_id_for_hkl(hkl))
    }

    fn switch_and_confirm(
        &mut self,
        target: &LayoutId,
        confirm_timeout_ms: u64,
    ) -> Result<LayoutSwitchOutcome, Self::Error> {
        let Some((focus_hwnd, focus_tid)) = foreground_gui_target() else {
            return Ok(LayoutSwitchOutcome::RejectedOrTimedOut);
        };
        let Some(&target_raw) = self.id_to_hkl.get(target) else {
            return Err(format!("target layout `{target}` is not installed"));
        };
        let target_hkl = HKL(target_raw as *mut std::ffi::c_void);

        let start = Instant::now();
        let _ = unsafe {
            PostMessageW(
                Some(focus_hwnd),
                WM_INPUTLANGCHANGEREQUEST,
                WPARAM(0),
                LPARAM(target_hkl.0 as isize),
            )
        };

        let deadline = Duration::from_millis(confirm_timeout_ms);
        loop {
            let now_hkl = unsafe { GetKeyboardLayout(focus_tid) };
            if now_hkl == target_hkl {
                return Ok(LayoutSwitchOutcome::Confirmed(target.clone()));
            }
            if start.elapsed() >= deadline {
                return Ok(LayoutSwitchOutcome::RejectedOrTimedOut);
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn list_installed_layouts(&self) -> Result<Vec<InstalledLayout>, Self::Error> {
        Ok(self.installed.clone())
    }
}

struct RegLayoutEntry {
    klid: String,
    layout_id: Option<u16>,
    layout_text: String,
}

fn resolve_hkl_from_registry(
    lang_id: u16,
    device_id: u16,
    entries: &[RegLayoutEntry],
) -> (String, String) {
    if (device_id & 0xF000) == 0xF000 {
        let target_layout_id = device_id & 0x0FFF;
        if let Some(entry) = entries
            .iter()
            .find(|e| e.layout_id == Some(target_layout_id))
        {
            return (entry.klid.clone(), entry.layout_text.clone());
        }
    }

    let id = if device_id != 0 { device_id } else { lang_id };
    let candidate = format!("{id:08X}");
    if let Some(entry) = entries
        .iter()
        .find(|e| e.klid.eq_ignore_ascii_case(&candidate))
    {
        return (entry.klid.clone(), entry.layout_text.clone());
    }

    (candidate, String::from("<unknown>"))
}

fn load_registry_keyboard_layouts() -> Vec<RegLayoutEntry> {
    let mut results = Vec::new();
    let subkey = to_wide_null("SYSTEM\\CurrentControlSet\\Control\\Keyboard Layouts");
    let mut root_key = HKEY::default();
    if unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            KEY_READ,
            &mut root_key,
        )
    }
    .is_err()
    {
        return results;
    }

    let mut index = 0u32;
    loop {
        let mut name_buf = [0u16; 64];
        let mut name_len = name_buf.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                root_key,
                index,
                Some(PWSTR(name_buf.as_mut_ptr())),
                &mut name_len,
                None,
                None,
                None,
                None,
            )
        };
        if status.is_err() {
            break;
        }
        index += 1;

        let klid = OsString::from_wide(&name_buf[..name_len as usize])
            .to_string_lossy()
            .to_ascii_uppercase();

        let klid_wide = to_wide_null(&klid);
        let mut child_key = HKEY::default();
        if unsafe {
            RegOpenKeyExW(
                root_key,
                PCWSTR(klid_wide.as_ptr()),
                Some(0),
                KEY_READ,
                &mut child_key,
            )
        }
        .is_ok()
        {
            let layout_text = query_reg_sz(child_key, "Layout Text").unwrap_or_default();
            let layout_id = query_reg_sz(child_key, "Layout Id")
                .and_then(|s| u16::from_str_radix(s.trim(), 16).ok());
            let _ = unsafe { RegCloseKey(child_key) };
            results.push(RegLayoutEntry {
                klid,
                layout_id,
                layout_text,
            });
        }
    }

    let _ = unsafe { RegCloseKey(root_key) };
    results
}

fn query_reg_sz(key: HKEY, value_name: &str) -> Option<String> {
    let name_wide = to_wide_null(value_name);
    let mut reg_type = REG_SZ;
    let mut buf = [0u8; 512];
    let mut byte_len = buf.len() as u32;
    unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(name_wide.as_ptr()),
            None,
            Some(&mut reg_type),
            Some(buf.as_mut_ptr()),
            Some(&mut byte_len),
        )
        .ok()
        .ok()?;
    }
    let u16_len = (byte_len as usize) / 2;
    let mut wide = Vec::with_capacity(u16_len);
    for &chunk in buf[..u16_len * 2].as_chunks::<2>().0 {
        let val = u16::from_le_bytes(chunk);
        if val == 0 {
            break;
        }
        wide.push(val);
    }
    Some(OsString::from_wide(&wide).to_string_lossy().into_owned())
}

fn lcid_to_bcp47(lang_id: u16) -> Option<String> {
    let mut buf = [0u16; 85];
    let len = unsafe { LCIDToLocaleName(u32::from(lang_id), Some(&mut buf), 0) };
    if len <= 1 {
        return None;
    }
    Some(
        OsString::from_wide(&buf[..(len as usize - 1)])
            .to_string_lossy()
            .into_owned(),
    )
}

fn friendly_locale_name(bcp47: Option<&str>, fallback: &str) -> String {
    let Some(tag) = bcp47 else {
        return fallback.to_string();
    };
    let tag_wide = to_wide_null(tag);
    // For languages with a single primary region (like Hebrew he-IL), show clean "Hebrew";
    // for multi-region languages (like English en-IN / en-US / en-GB), show "English (India)".
    let lctype = if tag.eq_ignore_ascii_case("he-IL") || tag.eq_ignore_ascii_case("he") {
        LOCALE_SENGLISHLANGUAGENAME
    } else {
        LOCALE_SENGLISHDISPLAYNAME
    };
    let mut buf = [0u16; 128];
    let len = unsafe { GetLocaleInfoEx(PCWSTR(tag_wide.as_ptr()), lctype, Some(&mut buf)) };
    if len > 1 {
        return OsString::from_wide(&buf[..(len as usize - 1)])
            .to_string_lossy()
            .into_owned();
    }
    fallback.to_string()
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
