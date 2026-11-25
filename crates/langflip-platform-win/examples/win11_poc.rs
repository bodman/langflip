//! Phase 1.5 Windows 11 Proof-of-Concept Spike (`T007`).
//!
//! Validates the riskiest Win32 assumptions before full implementation:
//! 1. `GetKeyboardLayoutList` + Registry `HKL -> KLID` resolution
//! 2. `GetGUIThreadInfo` vs `GetForegroundWindow` active layout detection
//! 3. `WM_INPUTLANGCHANGEREQUEST` + confirmation polling latency
//! 4. `SendInput` physical scan-code injection + `VK_E8` mask key

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("win11_poc is a Windows-only example; cross-compile with:");
    eprintln!("  ./scripts/cargo.sh build --release --locked --target x86_64-pc-windows-gnu -p langflip-platform-win --example win11_poc");
}

#[cfg(target_os = "windows")]
fn main() {
    win_impl::run();
}

#[cfg(target_os = "windows")]
mod win_impl {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::thread;
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ, RegCloseKey, RegEnumKeyExW, RegOpenKeyExW,
        RegQueryValueExW,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardLayout, GetKeyboardLayoutList, HKL, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, SendInput, VIRTUAL_KEY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo, GetWindowTextW,
        GetWindowThreadProcessId, PostMessageW, WM_INPUTLANGCHANGEREQUEST,
    };
    use windows::core::PCWSTR;

    const LANGFLIP_MARKER: usize = 0x4C46_4C50;
    const VK_MASK_UNASSIGNED: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);

    // AT Set 1 physical scan codes for "hello" and Backspace.
    const SCAN_HELLO: [u16; 5] = [0x23, 0x12, 0x26, 0x26, 0x18];
    const SCAN_BACKSPACE: u16 = 0x0E;

    #[derive(Debug, Clone)]
    struct LayoutInfo {
        hkl: HKL,
        lang_id: u16,
        device_id: u16,
        klid: String,
        text: String,
    }

    pub fn run() {
        println!("=== LangFlip Phase 1.5 Windows 11 PoC ===");
        let layouts = enumerate_installed_layouts();
        println!("Installed keyboard layouts ({}):", layouts.len());
        for (i, l) in layouts.iter().enumerate() {
            println!(
                "  [{}] HKL={:#018X}  lang_id={:#06X}  device_id={:#06X}  KLID={}  name=\"{}\"",
                i, l.hkl.0 as usize, l.lang_id, l.device_id, l.klid, l.text
            );
        }

        if layouts.len() < 2 {
            eprintln!("Need at least 2 installed keyboard layouts to test switching.");
            return;
        }

        println!("\nFocus a text input (Notepad, Chrome/Edge, or Windows Terminal) within 5 seconds...");
        for sec in (1..=5).rev() {
            println!("  starting in {}s...", sec);
            thread::sleep(Duration::from_secs(1));
        }

        let fg_hwnd = unsafe { GetForegroundWindow() };
        if fg_hwnd.0.is_null() {
            eprintln!("error: GetForegroundWindow returned null");
            return;
        }

        let title = get_window_title(fg_hwnd);
        let fg_tid = unsafe { GetWindowThreadProcessId(fg_hwnd, None) };
        let fg_hkl = unsafe { GetKeyboardLayout(fg_tid) };

        let mut gui = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        let gui_ok = unsafe { GetGUIThreadInfo(fg_tid, &mut gui) }.is_ok();
        let focus_hwnd = if gui_ok && !gui.hwndFocus.0.is_null() {
            gui.hwndFocus
        } else {
            fg_hwnd
        };
        let focus_tid = unsafe { GetWindowThreadProcessId(focus_hwnd, None) };
        let focus_hkl = unsafe { GetKeyboardLayout(focus_tid) };

        println!("\nTarget window: \"{}\" (fg_hwnd={:?}, focus_hwnd={:?})", title, fg_hwnd.0, focus_hwnd.0);
        println!(
            "  fg_tid={}    -> HKL={:#018X}",
            fg_tid, fg_hkl.0 as usize
        );
        println!(
            "  focus_tid={} -> HKL={:#018X} (gui_info_ok={})",
            focus_tid, focus_hkl.0 as usize, gui_ok
        );

        let initial_hkl = focus_hkl;
        let target_layout = pick_other_layout(&layouts, initial_hkl);
        println!(
            "\n[Step 1] Switching layout: {:#018X} -> {} ({:#018X}, \"{}\")",
            initial_hkl.0 as usize, target_layout.klid, target_layout.hkl.0 as usize, target_layout.text
        );

        let (confirmed, elapsed_ms, observed_hkl) =
            switch_and_confirm(focus_hwnd, focus_tid, target_layout.hkl, 100);
        println!(
            "  switch_and_confirm: confirmed={} elapsed={}ms observed_hkl={:#018X}",
            confirmed, elapsed_ms, observed_hkl.0 as usize
        );

        if !confirmed {
            println!("  ABORTING injection because layout switch was not confirmed (safe path).");
            return;
        }

        println!("\n[Step 2] Injecting mask key + 5 scan-code keys ('hello' physical keys)...");
        let t_inject = Instant::now();
        let (sent1, exp1) = send_mask_and_keys(&SCAN_HELLO);
        println!("  SendInput(type 5 keys): sent={}/{} in {}ms", sent1, exp1, t_inject.elapsed().as_millis());
        thread::sleep(Duration::from_millis(600));

        println!("\n[Step 3] Switching back to initial layout ({:#018X}), erasing 5 chars, replaying 5 keys...", initial_hkl.0 as usize);
        let t_total = Instant::now();
        let (confirmed2, elapsed2, _) = switch_and_confirm(focus_hwnd, focus_tid, initial_hkl, 100);
        println!("  switch back: confirmed={} elapsed={}ms", confirmed2, elapsed2);
        if !confirmed2 {
            println!("  ABORTING erase+replay because switch-back was not confirmed.");
            return;
        }

        let (sent_bs, exp_bs) = send_backspaces(SCAN_HELLO.len());
        let (sent_rep, exp_rep) = send_scan_keys(&SCAN_HELLO);
        println!(
            "  erase+replay: backspaces={}/{}, replay={}/{}, total_convert={}ms",
            sent_bs,
            exp_bs,
            sent_rep,
            exp_rep,
            t_total.elapsed().as_millis()
        );
        println!("\n=== PoC complete ===");
    }

    fn pick_other_layout(layouts: &[LayoutInfo], current: HKL) -> &LayoutInfo {
        // Prefer toggling between EN (0x0409) and HE (0x040D) if present.
        let current_lang = (current.0 as usize & 0xFFFF) as u16;
        let preferred_lang = if current_lang == 0x0409 {
            Some(0x040D)
        } else if current_lang == 0x040D {
            Some(0x0409)
        } else {
            None
        };

        if let Some(lang) = preferred_lang
            && let Some(found) = layouts.iter().find(|l| l.lang_id == lang)
        {
            return found;
        }
        layouts
            .iter()
            .find(|l| l.hkl != current)
            .unwrap_or(&layouts[0])
    }

    fn switch_and_confirm(
        target_hwnd: HWND,
        target_tid: u32,
        target_hkl: HKL,
        timeout_ms: u64,
    ) -> (bool, u128, HKL) {
        let start = Instant::now();
        let _ = unsafe {
            PostMessageW(
                Some(target_hwnd),
                WM_INPUTLANGCHANGEREQUEST,
                WPARAM(0),
                LPARAM(target_hkl.0 as isize),
            )
        };

        let deadline = Duration::from_millis(timeout_ms);
        loop {
            let now_hkl = unsafe { GetKeyboardLayout(target_tid) };
            if now_hkl == target_hkl {
                return (true, start.elapsed().as_millis(), now_hkl);
            }
            if start.elapsed() >= deadline {
                return (false, start.elapsed().as_millis(), now_hkl);
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn send_mask_and_keys(scans: &[u16]) -> (usize, usize) {
        let mut inputs = Vec::with_capacity(1 + scans.len() * 2);
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_MASK_UNASSIGNED,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: LANGFLIP_MARKER,
                },
            },
        });
        append_scan_taps(&mut inputs, scans);
        dispatch_inputs(&inputs)
    }

    fn send_backspaces(count: usize) -> (usize, usize) {
        let scans = vec![SCAN_BACKSPACE; count];
        send_scan_keys(&scans)
    }

    fn send_scan_keys(scans: &[u16]) -> (usize, usize) {
        let mut inputs = Vec::with_capacity(scans.len() * 2);
        append_scan_taps(&mut inputs, scans);
        dispatch_inputs(&inputs)
    }

    fn append_scan_taps(inputs: &mut Vec<INPUT>, scans: &[u16]) {
        for &scan in scans {
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: scan,
                        dwFlags: KEYEVENTF_SCANCODE,
                        time: 0,
                        dwExtraInfo: LANGFLIP_MARKER,
                    },
                },
            });
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: scan,
                        dwFlags: KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: LANGFLIP_MARKER,
                    },
                },
            });
        }
    }

    fn dispatch_inputs(inputs: &[INPUT]) -> (usize, usize) {
        let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) } as usize;
        (sent, inputs.len())
    }

    fn get_window_title(hwnd: HWND) -> String {
        let mut buf = [0u16; 256];
        let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
        if len <= 0 {
            return String::new();
        }
        OsString::from_wide(&buf[..len as usize])
            .to_string_lossy()
            .into_owned()
    }

    fn enumerate_installed_layouts() -> Vec<LayoutInfo> {
        let count = unsafe { GetKeyboardLayoutList(None) };
        if count <= 0 {
            return Vec::new();
        }
        let mut hkls = vec![HKL::default(); count as usize];
        let filled = unsafe { GetKeyboardLayoutList(Some(&mut hkls)) };
        if filled <= 0 {
            return Vec::new();
        }
        hkls.truncate(filled as usize);

        let reg_layouts = load_registry_keyboard_layouts();

        hkls.into_iter()
            .map(|hkl| {
                let raw = hkl.0 as usize;
                let lang_id = (raw & 0xFFFF) as u16;
                let device_id = ((raw >> 16) & 0xFFFF) as u16;

                let (klid, text) = resolve_hkl_from_registry(lang_id, device_id, &reg_layouts);
                LayoutInfo {
                    hkl,
                    lang_id,
                    device_id,
                    klid,
                    text,
                }
            })
            .collect()
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
        // High nibble 0xF indicates a Layout Id lookup (e.g., 0xF0xx -> "00xx").
        if (device_id & 0xF000) == 0xF000 {
            let target_layout_id = device_id & 0x0FFF;
            if let Some(entry) = entries
                .iter()
                .find(|e| e.layout_id == Some(target_layout_id))
            {
                return (entry.klid.clone(), entry.layout_text.clone());
            }
        }

        // Otherwise standard KLID is zero-padded device_id (or lang_id if device_id == 0).
        let id = if device_id != 0 { device_id } else { lang_id };
        let candidate = format!("{:08X}", id);
        if let Some(entry) = entries.iter().find(|e| e.klid.eq_ignore_ascii_case(&candidate)) {
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
                    Some(windows::core::PWSTR(name_buf.as_mut_ptr())),
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

    fn to_wide_null(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
}
