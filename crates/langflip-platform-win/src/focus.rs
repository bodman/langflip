//! Foreground window & GUI-thread focus observer (`T028`, Phase 2 UIA seam).

use std::cell::RefCell;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::thread;
use std::time::{Duration, Instant};

use langflip_core::key::scan;
use langflip_core::ports::{ControlKind, FallbackText, FocusContext, FocusId, FocusObserver};
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, OpenClipboard,
    SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, IUIAutomationValuePattern,
    TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start, UIA_ButtonControlTypeId,
    UIA_CheckBoxControlTypeId, UIA_HyperlinkControlTypeId, UIA_ImageControlTypeId,
    UIA_ListItemControlTypeId, UIA_MenuBarControlTypeId, UIA_MenuItemControlTypeId,
    UIA_RadioButtonControlTypeId, UIA_ScrollBarControlTypeId, UIA_SliderControlTypeId,
    UIA_TabItemControlTypeId, UIA_TextPatternId, UIA_TitleBarControlTypeId,
    UIA_ToolBarControlTypeId, UIA_TreeItemControlTypeId, UIA_ValuePatternId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, SendInput, VIRTUAL_KEY, VK_C, VK_CONTROL,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GUITHREADINFO, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
};

use crate::injector::LANGFLIP_INJECTED_MARKER;

/// Returns `(focus_hwnd, focus_thread_id)` for the currently focused input control,
/// falling back to the top-level foreground window if `GetGUIThreadInfo` is unavailable.
///
/// Phase 1.5 proved this is essential on Windows 11 Notepad where `fg_tid != focus_tid`.
pub(crate) fn foreground_gui_target() -> Option<(HWND, u32)> {
    let fg_hwnd = unsafe { GetForegroundWindow() };
    if fg_hwnd.0.is_null() {
        return None;
    }
    let fg_tid = unsafe { GetWindowThreadProcessId(fg_hwnd, None) };
    let mut gui = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    let focus_hwnd = if unsafe { GetGUIThreadInfo(fg_tid, &mut gui) }.is_ok()
        && !gui.hwndFocus.0.is_null()
    {
        gui.hwndFocus
    } else {
        fg_hwnd
    };
    let focus_tid = unsafe { GetWindowThreadProcessId(focus_hwnd, None) };
    Some((focus_hwnd, focus_tid))
}

/// Returns `true` when the foreground app is Windows 11's WinUI 3 Notepad
/// (`Notepad` / `RichEditD2DPT`), which is known to drop or reorder rapid
/// zero-hold `SendInput` bursts.
pub(crate) fn is_win11_notepad_foreground() -> bool {
    let fg_hwnd = unsafe { GetForegroundWindow() };
    if !fg_hwnd.0.is_null() && window_class_name(fg_hwnd).eq_ignore_ascii_case("Notepad") {
        return true;
    }
    if let Some((focus_hwnd, _)) = foreground_gui_target()
        && window_class_name(focus_hwnd).eq_ignore_ascii_case("RichEditD2DPT")
    {
        return true;
    }
    false
}

fn window_class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    if len <= 0 {
        return String::new();
    }
    OsString::from_wide(&buf[..len as usize])
        .to_string_lossy()
        .into_owned()
}

thread_local! {
    static WORKER_UIA: RefCell<Option<IUIAutomation>> = const { RefCell::new(None) };
}

fn with_worker_uia<R>(f: impl FnOnce(&IUIAutomation) -> R) -> Option<R> {
    WORKER_UIA.with(|slot| {
        let mut borrow = slot.borrow_mut();
        if borrow.is_none() {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }
            let uia: IUIAutomation =
                unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.ok()?;
            *borrow = Some(uia);
        }
        borrow.as_ref().map(f)
    })
}

fn query_uia_has_selection(uia: &IUIAutomation) -> bool {
    let Ok(elem) = (unsafe { uia.GetFocusedElement() }) else {
        return false;
    };
    let Ok(pattern) =
        (unsafe { elem.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) })
    else {
        return false;
    };
    let Ok(ranges) = (unsafe { pattern.GetSelection() }) else {
        return false;
    };
    let len = unsafe { ranges.Length() }.unwrap_or(0);
    for i in 0..len {
        if let Ok(range) = unsafe { ranges.GetElement(i) } {
            let cmp = unsafe {
                range.CompareEndpoints(
                    TextPatternRangeEndpoint_Start,
                    &range,
                    TextPatternRangeEndpoint_End,
                )
            };
            if matches!(cmp, Ok(v) if v != 0) {
                return true;
            }
        }
    }
    false
}

/// Returns `true` when the foreground window is a known terminal host where
/// empty-buffer UIA fallback must never run.
fn is_terminal_foreground() -> bool {
    const TERMINAL_CLASSES: &[&str] = &[
        "CASCADIA_HOSTING_WINDOW_CLASS",
        "ConsoleWindowClass",
        "mintty",
        "Alacritty",
        "WezTerm",
    ];
    let matches_terminal = |hwnd: HWND| {
        if hwnd.0.is_null() {
            return false;
        }
        let cls = window_class_name(hwnd);
        TERMINAL_CLASSES
            .iter()
            .any(|t| cls.eq_ignore_ascii_case(t))
    };
    let fg_hwnd = unsafe { GetForegroundWindow() };
    if matches_terminal(fg_hwnd) {
        return true;
    }
    if let Some((focus_hwnd, _)) = foreground_gui_target()
        && matches_terminal(focus_hwnd)
    {
        return true;
    }
    false
}

const CF_UNICODETEXT: u32 = 13;
const CLIPBOARD_POLL_TIMEOUT_MS: u64 = 60;

/// Checks via UIA whether it is safe to run the `Shift+Home` + `Ctrl+C` fallback.
/// Rejects password fields, read-only controls, and non-text UI widgets (buttons,
/// menus, list items, etc.), while allowing editable inputs and browser/Electron
/// documents where the DOM caret is active after switching back to a tab.
fn uia_allows_fallback_read(uia: &IUIAutomation) -> (bool, bool) {
    let Ok(elem) = (unsafe { uia.GetFocusedElement() }) else {
        // If UIA cannot inspect the element, stay conservative and allow only if
        // not a terminal (already checked by caller).
        return (true, false);
    };

    // Privacy invariant: NEVER read password fields.
    if unsafe { elem.CurrentIsPassword() }.is_ok_and(|b| b.as_bool()) {
        return (false, false);
    }

    // Never modify read-only controls.
    if let Ok(vp) =
        unsafe { elem.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
        && unsafe { vp.CurrentIsReadOnly() }.is_ok_and(|b| b.as_bool())
    {
        return (false, false);
    }

    // Reject obvious non-text widgets (buttons, file lists, menus, tabs, etc.).
    if let Ok(ct) = unsafe { elem.CurrentControlType() } {
        let non_text = [
            UIA_ButtonControlTypeId,
            UIA_CheckBoxControlTypeId,
            UIA_RadioButtonControlTypeId,
            UIA_MenuItemControlTypeId,
            UIA_MenuBarControlTypeId,
            UIA_ListItemControlTypeId,
            UIA_TreeItemControlTypeId,
            UIA_TabItemControlTypeId,
            UIA_ToolBarControlTypeId,
            UIA_SliderControlTypeId,
            UIA_ScrollBarControlTypeId,
            UIA_HyperlinkControlTypeId,
            UIA_ImageControlTypeId,
            UIA_TitleBarControlTypeId,
        ]
        .contains(&ct);
        if non_text {
            return (false, false);
        }
    }

    let already_selected = query_uia_has_selection(uia);
    (true, already_selected)
}

fn send_marked_inputs(inputs: &[INPUT]) {
    if !inputs.is_empty() {
        let _ = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    }
}

fn scan_key(key: langflip_core::key::PhysicalKey, key_up: bool) -> INPUT {
    let mut flags: KEYBD_EVENT_FLAGS = KEYEVENTF_SCANCODE;
    if key.extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: key.scan_code,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: LANGFLIP_INJECTED_MARKER,
            },
        },
    }
}

fn vk_key(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    let flags = if key_up {
        KEYEVENTF_KEYUP
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: LANGFLIP_INJECTED_MARKER,
            },
        },
    }
}

fn send_shift_home() {
    send_marked_inputs(&[
        scan_key(scan::LEFT_SHIFT, false),
        scan_key(scan::HOME, false),
        scan_key(scan::HOME, true),
        scan_key(scan::LEFT_SHIFT, true),
    ]);
}

fn send_ctrl_c() {
    send_marked_inputs(&[
        vk_key(VK_CONTROL, false),
        vk_key(VK_C, false),
        vk_key(VK_C, true),
        vk_key(VK_CONTROL, true),
    ]);
}

fn send_right_arrow() {
    send_marked_inputs(&[
        scan_key(scan::RIGHT, false),
        scan_key(scan::RIGHT, true),
    ]);
}

fn read_clipboard_text() -> Option<String> {
    if unsafe { OpenClipboard(None) }.is_err() {
        return None;
    }
    let result = (|| {
        let handle = unsafe { GetClipboardData(CF_UNICODETEXT) }.ok()?;
        let ptr = unsafe { GlobalLock(HGLOBAL(handle.0)) } as *const u16;
        if ptr.is_null() {
            return None;
        }
        let mut len = 0usize;
        // Safety cap to avoid scanning unbounded memory on malformed clipboard data.
        while len < 65_536 && unsafe { *ptr.add(len) } != 0 {
            len += 1;
        }
        let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
        let text = OsString::from_wide(slice).to_string_lossy().into_owned();
        let _ = unsafe { GlobalUnlock(HGLOBAL(handle.0)) };
        Some(text)
    })();
    let _ = unsafe { CloseClipboard() };
    result
}

fn write_clipboard_text(text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let byte_len = wide.len() * std::mem::size_of::<u16>();
    let Ok(hmem) = (unsafe { GlobalAlloc(GMEM_MOVEABLE, byte_len) }) else {
        return;
    };
    let ptr = unsafe { GlobalLock(hmem) } as *mut u16;
    if ptr.is_null() {
        return;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
    }
    let _ = unsafe { GlobalUnlock(hmem) };

    if unsafe { OpenClipboard(None) }.is_ok() {
        let _ = unsafe { EmptyClipboard() };
        let _ = unsafe { SetClipboardData(CF_UNICODETEXT, Some(HANDLE(hmem.0))) };
        let _ = unsafe { CloseClipboard() };
    }
}

fn read_line_via_selection_fallback(max_len: usize) -> Option<FallbackText> {
    if max_len == 0 || is_terminal_foreground() {
        return None;
    }

    let (allowed, already_selected) =
        with_worker_uia(uia_allows_fallback_read).unwrap_or((true, false));
    if !allowed {
        return None;
    }

    let saved_clipboard = read_clipboard_text();
    let seq_before = unsafe { GetClipboardSequenceNumber() };

    if !already_selected {
        send_shift_home();
        thread::sleep(Duration::from_millis(12));
    }
    send_ctrl_c();

    // Wait briefly for the target app to populate the clipboard.
    let deadline = Instant::now() + Duration::from_millis(CLIPBOARD_POLL_TIMEOUT_MS);
    let mut updated = false;
    while Instant::now() < deadline {
        if unsafe { GetClipboardSequenceNumber() } != seq_before {
            updated = true;
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }

    if !updated {
        return None;
    }

    let copied = read_clipboard_text();

    // Always restore the user's previous clipboard text immediately.
    if let Some(prev) = saved_clipboard {
        write_clipboard_text(&prev);
    }

    let text = copied?;
    if text.is_empty()
        || text.trim().is_empty()
        || text.contains(['\r', '\n'])
        || text.chars().count() > max_len
    {
        if !already_selected {
            send_right_arrow();
        }
        return None;
    }

    Some(FallbackText {
        text,
        replaces_selection: true,
    })
}

#[derive(Debug, Default, Clone, Copy)]
pub struct WinFocusObserver;

impl FocusObserver for WinFocusObserver {
    fn current_focus(&self) -> FocusContext {
        let focus_id = foreground_gui_target()
            .map(|(hwnd, _)| FocusId(hwnd.0 as usize as u64))
            .unwrap_or(FocusId(0));
        FocusContext {
            focus_id,
            element_id: None,
            control_kind: ControlKind::Unknown,
        }
    }

    fn has_active_selection(&self) -> bool {
        with_worker_uia(query_uia_has_selection).unwrap_or(false)
    }

    fn read_line_before_caret(&self, max_len: usize) -> Option<FallbackText> {
        read_line_via_selection_fallback(max_len)
    }

    fn clear_fallback_selection(&self) {
        send_right_arrow();
    }
}
