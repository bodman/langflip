//! Foreground window & GUI-thread focus observer (`T028`, Phase 2 UIA seam).

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use langflip_core::ports::{ControlKind, FocusContext, FocusId, FocusObserver};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GUITHREADINFO, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
};

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
}
