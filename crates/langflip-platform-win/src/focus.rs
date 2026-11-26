//! Foreground window & GUI-thread focus observer (`T028`, Phase 2 UIA seam).

use std::cell::RefCell;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use langflip_core::ports::{ControlKind, FallbackText, FocusContext, FocusId, FocusObserver};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, IUIAutomationValuePattern,
    TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start, TextUnit_Line,
    UIA_EditControlTypeId, UIA_TextPatternId, UIA_ValuePatternId,
};
use windows::Win32::UI::Controls::EM_GETSEL;
use windows::Win32::UI::WindowsAndMessaging::{
    GUITHREADINFO, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
    SendMessageW,
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

fn query_uia_line_before_caret(uia: &IUIAutomation, max_len: usize) -> Option<FallbackText> {
    if max_len == 0 || is_terminal_foreground() {
        return None;
    }
    let elem = unsafe { uia.GetFocusedElement() }.ok()?;

    // Privacy invariant: NEVER read password fields.
    if unsafe { elem.CurrentIsPassword() }.is_ok_and(|b| b.as_bool()) {
        return None;
    }

    // Never modify read-only controls.
    let val_pattern =
        unsafe { elem.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }.ok();
    if let Some(ref vp) = val_pattern
        && unsafe { vp.CurrentIsReadOnly() }.is_ok_and(|b| b.as_bool())
    {
        return None;
    }

    let read_cap = i32::try_from(max_len.saturating_add(1)).unwrap_or(i32::MAX);

    // Path 1: IUIAutomationTextPattern (Notepad WinUI 3, Chrome/Edge Omnibox & web inputs, RichEdit, Word, etc.)
    if let Ok(text_pat) =
        unsafe { elem.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
        && let Ok(ranges) = unsafe { text_pat.GetSelection() }
        && unsafe { ranges.Length() }.unwrap_or(0) > 0
        && let Ok(sel) = unsafe { ranges.GetElement(0) }
    {
        let has_sel = matches!(
            unsafe {
                sel.CompareEndpoints(
                    TextPatternRangeEndpoint_Start,
                    &sel,
                    TextPatternRangeEndpoint_End,
                )
            },
            Ok(v) if v != 0
        );
        if has_sel {
            let text = unsafe { sel.GetText(read_cap) }.ok()?.to_string();
            if !text.is_empty()
                && !text.contains(['\r', '\n'])
                && text.chars().count() <= max_len
            {
                return Some(FallbackText {
                    text,
                    replaces_selection: true,
                });
            }
            return None;
        }

        if let Ok(line_range) = unsafe { sel.Clone() }
            && unsafe { line_range.ExpandToEnclosingUnit(TextUnit_Line) }.is_ok()
            && unsafe {
                line_range.MoveEndpointByRange(
                    TextPatternRangeEndpoint_End,
                    &sel,
                    TextPatternRangeEndpoint_Start,
                )
            }
            .is_ok()
            && let Ok(raw) = unsafe { line_range.GetText(read_cap) }
        {
            let raw_str = raw.to_string();
            let line = raw_str.rsplit(['\r', '\n']).next().unwrap_or("");
            if !line.is_empty() && line.chars().count() <= max_len {
                return Some(FallbackText {
                    text: line.to_string(),
                    replaces_selection: false,
                });
            }
        }
        return None;
    }

    // Path 2: Single-line Edit fallback via ValuePattern + EM_GETSEL (classic Win32 Edit controls).
    if let Some(vp) = val_pattern
        && unsafe { elem.CurrentControlType() }.ok() == Some(UIA_EditControlTypeId)
        && let Ok(raw) = unsafe { vp.CurrentValue() }
    {
        let val = raw.to_string();
        if val.is_empty() || val.contains(['\r', '\n']) {
            return None;
        }
        let (focus_hwnd, _) = foreground_gui_target()?;
        let mut start: u32 = 0;
        let mut end: u32 = 0;
        unsafe {
            SendMessageW(
                focus_hwnd,
                EM_GETSEL,
                Some(WPARAM(&mut start as *mut u32 as usize)),
                Some(LPARAM(&mut end as *mut u32 as isize)),
            );
        }
        let chars: Vec<char> = val.chars().collect();
        let start = start as usize;
        let end = end as usize;
        if start < end && end <= chars.len() {
            let len = end - start;
            if len <= max_len {
                return Some(FallbackText {
                    text: chars[start..end].iter().collect(),
                    replaces_selection: true,
                });
            }
        } else if start == end && end > 0 && end <= chars.len() && end <= max_len {
            return Some(FallbackText {
                text: chars[..end].iter().collect(),
                replaces_selection: false,
            });
        }
    }

    None
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
        with_worker_uia(|uia| query_uia_line_before_caret(uia, max_len)).flatten()
    }
}
