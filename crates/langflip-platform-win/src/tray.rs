//! Native Windows 11 system tray icon, right-click context menu (Settings / Exit),
//! single-instance mutex guard, and Settings dialog (`T010`).

use std::cell::RefCell;

use langflip_core::hotkey::HotkeySpec;
use langflip_core::key::{SUPPORTED_HOTKEY_NAMES, key_name_for, parse_key_name};
use langflip_core::layout::LayoutPair;
use langflip_core::ports::{SettingsSnapshot, TrayHost};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT,
    POINT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, COLOR_BTNFACE, CreateFontW, DEFAULT_CHARSET, DEFAULT_PITCH, DeleteObject,
    FF_SWISS, FW_NORMAL, HBRUSH, HFONT, OUT_DEFAULT_PRECIS,
};
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Controls::{BST_CHECKED, BST_UNCHECKED};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, BS_PUSHBUTTON,
    CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CBS_DROPDOWNLIST, CreatePopupMenu, CreateWindowExW,
    DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos,
    GetMessageW, GetSystemMetrics, HICON, IDC_ARROW, LoadCursorW, MB_ICONWARNING, MB_OK,
    MF_STRING, MSG, MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, SendMessageW, SetForegroundWindow,
    ShowWindow, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_NULL,
    WM_RBUTTONUP, WM_SETFONT, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
    WS_VSCROLL,
};
use windows::core::PCWSTR;

use crate::icon::create_tray_icon;

const WM_APP_TRAY: u32 = WM_APP + 1;
const ID_TRAY_ICON: u32 = 1;
const ID_MENU_SETTINGS: usize = 1001;
const ID_MENU_EXIT: usize = 1002;

const ID_COMBO_PRIMARY: i32 = 2001;
const ID_COMBO_SECONDARY: i32 = 2002;
const ID_COMBO_KEY: i32 = 2003;
const ID_CHK_CTRL: i32 = 2004;
const ID_CHK_ALT: i32 = 2005;
const ID_CHK_SHIFT: i32 = 2006;
const ID_CHK_META: i32 = 2007;
const ID_BTN_SAVE: i32 = 2008;
const ID_BTN_CANCEL: i32 = 2009;

/// Attaches to the parent console (e.g., PowerShell) when `--metrics` is passed
/// so diagnostics print even though the binary uses `windows_subsystem = "windows"`.
pub fn attach_parent_console() {
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

/// Single-instance mutex guard preventing duplicate LangFlip tray icons/hooks.
pub struct SingleInstanceGuard {
    handle: HANDLE,
}

impl SingleInstanceGuard {
    /// Returns `Ok(Some(guard))` if this is the first instance, or `Ok(None)` if already running.
    pub fn acquire() -> Result<Option<Self>, String> {
        let name = to_wide_null("Local\\LangFlipSingleInstanceMutex");
        let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }
            .map_err(|e| format!("CreateMutexW failed: {e}"))?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            let _ = unsafe { CloseHandle(handle) };
            return Ok(None);
        }
        Ok(Some(Self { handle }))
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

type SaveCallback = Box<dyn FnMut(LayoutPair, HotkeySpec) -> Result<(), String>>;
type ExitCallback = Box<dyn FnOnce()>;

struct TrayState {
    hinstance: HINSTANCE,
    tray_hwnd: HWND,
    settings_hwnd: Option<HWND>,
    icon: HICON,
    font: HFONT,
    taskbar_created_msg: u32,
    snapshot: SettingsSnapshot,
    on_save: SaveCallback,
    on_exit: Option<ExitCallback>,
    // Settings dialog child handles
    combo_primary: HWND,
    combo_secondary: HWND,
    combo_key: HWND,
    chk_ctrl: HWND,
    chk_alt: HWND,
    chk_shift: HWND,
    chk_meta: HWND,
}

thread_local! {
    static TRAY_STATE: RefCell<Option<TrayState>> = const { RefCell::new(None) };
}

#[derive(Debug, Default)]
pub struct WinTrayHost;

impl TrayHost for WinTrayHost {
    type Error = String;

    fn run<S, E>(
        self,
        initial: SettingsSnapshot,
        on_save: S,
        on_exit: E,
    ) -> Result<(), Self::Error>
    where
        S: FnMut(LayoutPair, HotkeySpec) -> Result<(), String> + 'static,
        E: FnOnce() + 'static,
    {
        let hmodule =
            unsafe { GetModuleHandleW(None) }.map_err(|e| format!("GetModuleHandleW: {e}"))?;
        let hinstance: HINSTANCE = hmodule.into();
        let icon = create_tray_icon()?;
        let font = create_ui_font();

        let tray_class = to_wide_null("LangFlipTrayHostClass");
        let settings_class = to_wide_null("LangFlipSettingsDialogClass");
        let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default();

        let wc_tray = WNDCLASSW {
            lpfnWndProc: Some(tray_wnd_proc),
            hInstance: hinstance,
            lpszClassName: PCWSTR(tray_class.as_ptr()),
            hIcon: icon,
            hCursor: cursor,
            ..Default::default()
        };
        unsafe { RegisterClassW(&wc_tray) };

        let wc_settings = WNDCLASSW {
            lpfnWndProc: Some(settings_wnd_proc),
            hInstance: hinstance,
            lpszClassName: PCWSTR(settings_class.as_ptr()),
            hIcon: icon,
            hCursor: cursor,
            hbrBackground: HBRUSH((COLOR_BTNFACE.0 + 1) as *mut std::ffi::c_void),
            ..Default::default()
        };
        unsafe { RegisterClassW(&wc_settings) };

        let title = to_wide_null("LangFlip");
        let tray_hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(tray_class.as_ptr()),
                PCWSTR(title.as_ptr()),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(hinstance),
                None,
            )
        }
        .map_err(|e| format!("CreateWindowExW(tray) failed: {e}"))?;

        let taskbar_msg_name = to_wide_null("TaskbarCreated");
        let taskbar_created_msg = unsafe { RegisterWindowMessageW(PCWSTR(taskbar_msg_name.as_ptr())) };

        TRAY_STATE.with(|slot| {
            *slot.borrow_mut() = Some(TrayState {
                hinstance,
                tray_hwnd,
                settings_hwnd: None,
                icon,
                font,
                taskbar_created_msg,
                snapshot: initial,
                on_save: Box::new(on_save),
                on_exit: Some(Box::new(on_exit)),
                combo_primary: HWND::default(),
                combo_secondary: HWND::default(),
                combo_key: HWND::default(),
                chk_ctrl: HWND::default(),
                chk_alt: HWND::default(),
                chk_shift: HWND::default(),
                chk_meta: HWND::default(),
            });
        });

        add_tray_icon(tray_hwnd, icon)?;

        let mut msg = MSG::default();
        while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
            let _ = unsafe { TranslateMessage(&msg) };
            unsafe { DispatchMessageW(&msg) };
        }

        remove_tray_icon(tray_hwnd);
        TRAY_STATE.with(|slot| {
            if let Some(mut state) = slot.borrow_mut().take() {
                if let Some(exit_cb) = state.on_exit.take() {
                    exit_cb();
                }
                let _ = unsafe { DestroyIcon(state.icon) };
                if !state.font.is_invalid() {
                    let _ = unsafe { DeleteObject(state.font.into()) };
                }
            }
        });
        Ok(())
    }
}

fn add_tray_icon(hwnd: HWND, icon: HICON) -> Result<(), String> {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: ID_TRAY_ICON,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_APP_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    let tip = "LangFlip — Keyboard Layout Converter";
    for (dst, src) in nid.szTip.iter_mut().zip(tip.encode_utf16()) {
        *dst = src;
    }
    if unsafe { Shell_NotifyIconW(NIM_ADD, &nid) }.as_bool() {
        Ok(())
    } else {
        Err("Shell_NotifyIconW(NIM_ADD) failed".into())
    }
}

fn remove_tray_icon(hwnd: HWND) {
    let nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: ID_TRAY_ICON,
        ..Default::default()
    };
    let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
}

fn show_tray_menu(hwnd: HWND) {
    let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
        return;
    };
    let settings_lbl = to_wide_null("Settings...");
    let exit_lbl = to_wide_null("Exit");
    let _ = unsafe {
        AppendMenuW(
            menu,
            MF_STRING,
            ID_MENU_SETTINGS,
            PCWSTR(settings_lbl.as_ptr()),
        )
    };
    let _ = unsafe { AppendMenuW(menu, MF_STRING, ID_MENU_EXIT, PCWSTR(exit_lbl.as_ptr())) };

    let mut pt = POINT::default();
    let _ = unsafe { GetCursorPos(&mut pt) };
    let _ = unsafe { SetForegroundWindow(hwnd) };
    let _ = unsafe {
        TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            Some(0),
            hwnd,
            None,
        )
    };
    let _ = unsafe { PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0)) };
    let _ = unsafe { DestroyMenu(menu) };
}

fn open_or_focus_settings_dialog() {
    TRAY_STATE.with(|slot| {
        let mut borrow = slot.borrow_mut();
        let Some(state) = borrow.as_mut() else {
            return;
        };

        if let Some(existing) = state.settings_hwnd {
            let _ = unsafe { ShowWindow(existing, SW_SHOW) };
            let _ = unsafe { SetForegroundWindow(existing) };
            return;
        }

        let width = 410;
        let height = 265;
        let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        let x = ((screen_w - width) / 2).max(40);
        let y = ((screen_h - height) / 2).max(40);

        let class_name = to_wide_null("LangFlipSettingsDialogClass");
        let title = to_wide_null("LangFlip Settings");
        let Ok(dlg) = (unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
                x,
                y,
                width,
                height,
                Some(state.tray_hwnd),
                None,
                Some(state.hinstance),
                None,
            )
        }) else {
            return;
        };

        let hinst = state.hinstance;
        let font = state.font;

        create_label(dlg, hinst, font, "Primary language:", 20, 20, 125, 22);
        let combo_primary = create_combobox(dlg, hinst, font, ID_COMBO_PRIMARY, 150, 17, 230, 180);

        create_label(dlg, hinst, font, "Secondary language:", 20, 58, 125, 22);
        let combo_secondary =
            create_combobox(dlg, hinst, font, ID_COMBO_SECONDARY, 150, 55, 230, 180);

        create_label(dlg, hinst, font, "Trigger hotkey:", 20, 100, 125, 22);
        let combo_key = create_combobox(dlg, hinst, font, ID_COMBO_KEY, 150, 97, 140, 200);

        create_label(dlg, hinst, font, "Modifiers:", 20, 138, 125, 22);
        let chk_ctrl = create_checkbox(dlg, hinst, font, "Ctrl", ID_CHK_CTRL, 150, 136, 52, 22);
        let chk_alt = create_checkbox(dlg, hinst, font, "Alt", ID_CHK_ALT, 206, 136, 48, 22);
        let chk_shift = create_checkbox(dlg, hinst, font, "Shift", ID_CHK_SHIFT, 258, 136, 56, 22);
        let chk_meta = create_checkbox(dlg, hinst, font, "Win", ID_CHK_META, 318, 136, 54, 22);

        let _btn_save = create_button(dlg, hinst, font, "Save", ID_BTN_SAVE, 210, 182, 80, 28, true);
        let _btn_cancel =
            create_button(dlg, hinst, font, "Cancel", ID_BTN_CANCEL, 300, 182, 80, 28, false);

        // Populate installed languages
        let mut primary_idx = 0usize;
        let mut secondary_idx = usize::from(state.snapshot.installed_layouts.len() > 1);
        for (i, layout) in state.snapshot.installed_layouts.iter().enumerate() {
            let label = format!("{} ({})", layout.display_name, layout.id.as_str().to_ascii_uppercase());
            combo_add_string(combo_primary, &label);
            combo_add_string(combo_secondary, &label);
            if layout.id == state.snapshot.layouts.primary {
                primary_idx = i;
            }
            if layout.id == state.snapshot.layouts.secondary {
                secondary_idx = i;
            }
        }
        combo_set_cursel(combo_primary, primary_idx);
        combo_set_cursel(combo_secondary, secondary_idx);

        // Populate hotkey dropdown
        let current_key_name = key_name_for(state.snapshot.hotkey.key).unwrap_or("Pause");
        let mut key_idx = 0usize;
        for (i, &name) in SUPPORTED_HOTKEY_NAMES.iter().enumerate() {
            combo_add_string(combo_key, name);
            if name.eq_ignore_ascii_case(current_key_name) {
                key_idx = i;
            }
        }
        combo_set_cursel(combo_key, key_idx);

        set_checked(chk_ctrl, state.snapshot.hotkey.ctrl);
        set_checked(chk_alt, state.snapshot.hotkey.alt);
        set_checked(chk_shift, state.snapshot.hotkey.shift);
        set_checked(chk_meta, state.snapshot.hotkey.meta);

        state.settings_hwnd = Some(dlg);
        state.combo_primary = combo_primary;
        state.combo_secondary = combo_secondary;
        state.combo_key = combo_key;
        state.chk_ctrl = chk_ctrl;
        state.chk_alt = chk_alt;
        state.chk_shift = chk_shift;
        state.chk_meta = chk_meta;

        let _ = unsafe { ShowWindow(dlg, SW_SHOW) };
        let _ = unsafe { SetForegroundWindow(dlg) };
    });
}

fn handle_settings_save(dlg: HWND) {
    let result: Result<(), String> = TRAY_STATE.with(|slot| {
        let mut borrow = slot.borrow_mut();
        let Some(state) = borrow.as_mut() else {
            return Err("internal tray state missing".into());
        };

        let p_idx = combo_get_cursel(state.combo_primary);
        let s_idx = combo_get_cursel(state.combo_secondary);
        let k_idx = combo_get_cursel(state.combo_key);

        let primary = state
            .snapshot
            .installed_layouts
            .get(p_idx)
            .ok_or_else(|| "Please select a primary language.".to_string())?
            .id
            .clone();
        let secondary = state
            .snapshot
            .installed_layouts
            .get(s_idx)
            .ok_or_else(|| "Please select a secondary language.".to_string())?
            .id
            .clone();

        if primary == secondary {
            return Err("Primary and secondary languages must be different.".into());
        }

        let key_name = SUPPORTED_HOTKEY_NAMES
            .get(k_idx)
            .copied()
            .unwrap_or("Pause");
        let key = parse_key_name(key_name)
            .ok_or_else(|| format!("Unsupported hotkey `{key_name}`."))?;

        let new_hotkey = HotkeySpec {
            key,
            ctrl: is_checked(state.chk_ctrl),
            alt: is_checked(state.chk_alt),
            shift: is_checked(state.chk_shift),
            meta: is_checked(state.chk_meta),
        };
        let new_pair = LayoutPair { primary, secondary };

        (state.on_save)(new_pair.clone(), new_hotkey)?;
        state.snapshot.layouts = new_pair;
        state.snapshot.hotkey = new_hotkey;
        Ok(())
    });

    match result {
        Ok(()) => {
            let _ = unsafe { DestroyWindow(dlg) };
        }
        Err(msg) => {
            let text = to_wide_null(&msg);
            let cap = to_wide_null("LangFlip Settings");
            unsafe {
                MessageBoxW(
                    Some(dlg),
                    PCWSTR(text.as_ptr()),
                    PCWSTR(cap.as_ptr()),
                    MB_OK | MB_ICONWARNING,
                );
            }
        }
    }
}

unsafe extern "system" fn tray_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let taskbar_msg = TRAY_STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|st| st.taskbar_created_msg)
            .unwrap_or(0)
    });

    if taskbar_msg != 0 && msg == taskbar_msg {
        TRAY_STATE.with(|s| {
            if let Some(st) = s.borrow().as_ref() {
                let _ = add_tray_icon(st.tray_hwnd, st.icon);
            }
        });
        return LRESULT(0);
    }

    match msg {
        WM_APP_TRAY => {
            let event = (lparam.0 & 0xFFFF) as u32;
            if event == WM_RBUTTONUP {
                show_tray_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let cmd = wparam.0 & 0xFFFF;
            if cmd == ID_MENU_SETTINGS {
                open_or_focus_settings_dialog();
            } else if cmd == ID_MENU_EXIT {
                remove_tray_icon(hwnd);
                unsafe { PostQuitMessage(0) };
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            remove_tray_icon(hwnd);
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

unsafe extern "system" fn settings_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as i32;
            if id == ID_BTN_SAVE {
                handle_settings_save(hwnd);
                return LRESULT(0);
            } else if id == ID_BTN_CANCEL {
                let _ = unsafe { DestroyWindow(hwnd) };
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_CLOSE => {
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_DESTROY => {
            TRAY_STATE.with(|slot| {
                if let Some(state) = slot.borrow_mut().as_mut() {
                    state.settings_hwnd = None;
                }
            });
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn create_ui_font() -> HFONT {
    let face = to_wide_null("Segoe UI");
    unsafe {
        CreateFontW(
            -15,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            Default::default(),
            CLEARTYPE_QUALITY,
            (DEFAULT_PITCH.0 | FF_SWISS.0) as u32,
            PCWSTR(face.as_ptr()),
        )
    }
}

fn apply_font(hwnd: HWND, font: HFONT) {
    if !font.is_invalid() {
        unsafe { SendMessageW(hwnd, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1))) };
    }
}

#[allow(clippy::too_many_arguments)]
fn create_label(
    parent: HWND,
    hinst: HINSTANCE,
    font: HFONT,
    text: &str,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> HWND {
    let cls = to_wide_null("STATIC");
    let txt = to_wide_null(text);
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(cls.as_ptr()),
            PCWSTR(txt.as_ptr()),
            WS_CHILD | WS_VISIBLE,
            x,
            y,
            w,
            h,
            Some(parent),
            None,
            Some(hinst),
            None,
        )
    }
    .unwrap_or_default();
    apply_font(hwnd, font);
    hwnd
}

#[allow(clippy::too_many_arguments)]
fn create_combobox(
    parent: HWND,
    hinst: HINSTANCE,
    font: HFONT,
    id: i32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> HWND {
    let cls = to_wide_null("COMBOBOX");
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(cls.as_ptr()),
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
            x,
            y,
            w,
            h,
            Some(parent),
            Some(windows::Win32::UI::WindowsAndMessaging::HMENU(
                id as isize as *mut std::ffi::c_void,
            )),
            Some(hinst),
            None,
        )
    }
    .unwrap_or_default();
    apply_font(hwnd, font);
    hwnd
}

#[allow(clippy::too_many_arguments)]
fn create_checkbox(
    parent: HWND,
    hinst: HINSTANCE,
    font: HFONT,
    text: &str,
    id: i32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> HWND {
    let cls = to_wide_null("BUTTON");
    let txt = to_wide_null(text);
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(cls.as_ptr()),
            PCWSTR(txt.as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
            x,
            y,
            w,
            h,
            Some(parent),
            Some(windows::Win32::UI::WindowsAndMessaging::HMENU(
                id as isize as *mut std::ffi::c_void,
            )),
            Some(hinst),
            None,
        )
    }
    .unwrap_or_default();
    apply_font(hwnd, font);
    hwnd
}

#[allow(clippy::too_many_arguments)]
fn create_button(
    parent: HWND,
    hinst: HINSTANCE,
    font: HFONT,
    text: &str,
    id: i32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    default: bool,
) -> HWND {
    let cls = to_wide_null("BUTTON");
    let txt = to_wide_null(text);
    let style_bits = if default {
        BS_DEFPUSHBUTTON as u32
    } else {
        BS_PUSHBUTTON as u32
    };
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(cls.as_ptr()),
            PCWSTR(txt.as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(style_bits),
            x,
            y,
            w,
            h,
            Some(parent),
            Some(windows::Win32::UI::WindowsAndMessaging::HMENU(
                id as isize as *mut std::ffi::c_void,
            )),
            Some(hinst),
            None,
        )
    }
    .unwrap_or_default();
    apply_font(hwnd, font);
    hwnd
}

fn combo_add_string(combo: HWND, text: &str) {
    let wide = to_wide_null(text);
    unsafe { SendMessageW(combo, CB_ADDSTRING, Some(WPARAM(0)), Some(LPARAM(wide.as_ptr() as isize))) };
}

fn combo_set_cursel(combo: HWND, index: usize) {
    unsafe { SendMessageW(combo, CB_SETCURSEL, Some(WPARAM(index)), Some(LPARAM(0))) };
}

fn combo_get_cursel(combo: HWND) -> usize {
    let res = unsafe { SendMessageW(combo, CB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))) };
    res.0.max(0) as usize
}

fn set_checked(chk: HWND, checked: bool) {
    let state = if checked { BST_CHECKED.0 } else { BST_UNCHECKED.0 };
    unsafe { SendMessageW(chk, BM_SETCHECK, Some(WPARAM(state as usize)), Some(LPARAM(0))) };
}

fn is_checked(chk: HWND) -> bool {
    let res = unsafe { SendMessageW(chk, BM_GETCHECK, Some(WPARAM(0)), Some(LPARAM(0))) };
    res.0 as u32 == BST_CHECKED.0
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
