//! Option 3 — Nord Calm Windows 11 system tray icon, Fluent-themed right-click
//! context menu (Settings / Exit), single-instance mutex guard, and styled Settings dialog.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

use langflip_core::hotkey::HotkeySpec;
use langflip_core::key::{SUPPORTED_HOTKEY_NAMES, key_name_for, parse_key_name};
use langflip_core::layout::LayoutPair;
use langflip_core::ports::{SettingsSnapshot, TrayHost};
use windows::Win32::Foundation::{
    COLORREF, CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HINSTANCE, HWND, LPARAM,
    LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CreateFontW, CreatePen, CreateSolidBrush, DEFAULT_CHARSET,
    DEFAULT_PITCH, DeleteObject, DrawTextW, DT_CENTER, DT_SINGLELINE, DT_VCENTER, EndPaint,
    FF_SWISS, FW_NORMAL, FW_SEMIBOLD, FillRect, HBRUSH, HDC, HFONT, InvalidateRect,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, RoundRect, SelectObject, SetBkColor, SetBkMode,
    SetTextColor, TRANSPARENT,
};
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Controls::{
    DRAWITEMSTRUCT, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, ODS_SELECTED,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, BS_OWNERDRAW, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CBS_DROPDOWNLIST,
    CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetClientRect, GetCursorPos, GetDlgCtrlID, GetMessageW, GetSystemMetrics,
    HICON, HMENU, IDC_ARROW, LoadCursorW, MB_ICONWARNING, MB_OK, MF_GRAYED, MF_SEPARATOR,
    MF_STRING, MSG, MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, SendMessageW, SetForegroundWindow,
    ShowWindow, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_CTLCOLORBTN, WM_CTLCOLOREDIT,
    WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DRAWITEM, WM_NULL, WM_PAINT,
    WM_RBUTTONUP, WM_SETFONT, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
    WS_VSCROLL,
};
use windows::core::{PCSTR, PCWSTR};

use crate::icon::create_tray_icon;

const WM_APP_TRAY: u32 = WM_APP + 1;
const ID_TRAY_ICON: u32 = 1;
const ID_MENU_HEADER: usize = 1000;
const ID_MENU_SETTINGS: usize = 1001;
const ID_MENU_EXIT: usize = 1002;

const ID_COMBO_PRIMARY: i32 = 2001;
const ID_COMBO_SECONDARY: i32 = 2002;
const ID_COMBO_KEY: i32 = 2003;
const ID_PILL_CTRL: i32 = 2004;
const ID_PILL_ALT: i32 = 2005;
const ID_PILL_SHIFT: i32 = 2006;
const ID_PILL_META: i32 = 2007;
const ID_BTN_SAVE: i32 = 2008;
const ID_BTN_CANCEL: i32 = 2009;
const ID_LBL_SUBTITLE: i32 = 2010;
const ID_LBL_SECTION_1: i32 = 2011;
const ID_LBL_SECTION_2: i32 = 2012;
const ID_LBL_TITLE: i32 = 2013;
const ID_LBL_SECTION_3: i32 = 2014;
const ID_PILL_AUTOSTART: i32 = 2015;

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

/// Option 3 — Nord Calm low-contrast Nordic slate palette.
#[derive(Clone, Copy)]
struct NordPalette {
    window_bg: COLORREF,
    card_bg: COLORREF,
    card_border: COLORREF,
    input_bg: COLORREF,
    input_border: COLORREF,
    text_title: COLORREF,
    text_primary: COLORREF,
    text_muted: COLORREF,
    section_ice: COLORREF,
    pill_on_bg: COLORREF,
    pill_on_border: COLORREF,
    pill_on_text: COLORREF,
    save_bg: COLORREF,
    save_pressed: COLORREF,
    cancel_bg: COLORREF,
    cancel_pressed: COLORREF,
}

const NORD: NordPalette = NordPalette {
    window_bg: rgb(36, 41, 51),      // #242933
    card_bg: rgb(46, 52, 64),        // #2E3440
    card_border: rgb(59, 66, 82),    // #3B4252
    input_bg: rgb(36, 41, 51),       // #242933
    input_border: rgb(67, 76, 94),   // #434C5E
    text_title: rgb(236, 239, 244),  // #ECEFF4
    text_primary: rgb(216, 222, 233),// #D8DEE9
    text_muted: rgb(140, 153, 174),  // #8C99AE
    section_ice: rgb(136, 192, 208), // #88C0D0
    pill_on_bg: rgb(50, 75, 87),     // #324B57
    pill_on_border: rgb(136, 192, 208), // #88C0D0
    pill_on_text: rgb(229, 244, 248),   // #E5F4F8
    save_bg: rgb(94, 129, 172),      // #5E81AC
    save_pressed: rgb(76, 107, 145), // #4C6B91
    cancel_bg: rgb(46, 52, 64),      // #2E3440
    cancel_pressed: rgb(59, 66, 82), // #3B4252
};

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}

type SaveCallback = Box<dyn FnMut(LayoutPair, HotkeySpec, bool) -> Result<(), String>>;
type ExitCallback = Box<dyn FnOnce()>;

static TASKBAR_CREATED_MSG: AtomicU32 = AtomicU32::new(0);

struct DialogResources {
    dpi: u32,
    window_brush: HBRUSH,
    card_brush: HBRUSH,
    input_brush: HBRUSH,
    font_title: HFONT,
    font_section: HFONT,
    font_body: HFONT,
    font_sub: HFONT,
    font_pill: HFONT,
    font_pill_bold: HFONT,
}

impl DialogResources {
    fn new(dpi: u32) -> Self {
        Self {
            dpi,
            window_brush: unsafe { CreateSolidBrush(NORD.window_bg) },
            card_brush: unsafe { CreateSolidBrush(NORD.card_bg) },
            input_brush: unsafe { CreateSolidBrush(NORD.input_bg) },
            font_title: create_ui_font(scale(20, dpi), true),
            font_section: create_ui_font(scale(12, dpi), true),
            font_body: create_ui_font(scale(14, dpi), false),
            font_sub: create_ui_font(scale(13, dpi), false),
            font_pill: create_ui_font(scale(12, dpi), false),
            font_pill_bold: create_ui_font(scale(12, dpi), true),
        }
    }

    fn destroy(self) {
        unsafe {
            let _ = DeleteObject(self.window_brush.into());
            let _ = DeleteObject(self.card_brush.into());
            let _ = DeleteObject(self.input_brush.into());
            let _ = DeleteObject(self.font_title.into());
            let _ = DeleteObject(self.font_section.into());
            let _ = DeleteObject(self.font_body.into());
            let _ = DeleteObject(self.font_sub.into());
            let _ = DeleteObject(self.font_pill.into());
            let _ = DeleteObject(self.font_pill_bold.into());
        }
    }
}

#[derive(Clone, Copy, Default)]
struct PillStates {
    ctrl: bool,
    alt: bool,
    shift: bool,
    meta: bool,
    auto_start: bool,
}

struct TrayState {
    hinstance: HINSTANCE,
    tray_hwnd: HWND,
    settings_hwnd: Option<HWND>,
    icon: HICON,
    snapshot: SettingsSnapshot,
    on_save: SaveCallback,
    on_exit: Option<ExitCallback>,
    dialog_res: Option<DialogResources>,
    pills: PillStates,
    // Settings dialog child handles
    combo_primary: HWND,
    combo_secondary: HWND,
    combo_key: HWND,
    pill_ctrl: HWND,
    pill_alt: HWND,
    pill_shift: HWND,
    pill_meta: HWND,
    pill_autostart: HWND,
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
        S: FnMut(LayoutPair, HotkeySpec, bool) -> Result<(), String> + 'static,
        E: FnOnce() + 'static,
    {
        init_common_controls();
        enable_uxtheme_dark_menus();

        let hmodule =
            unsafe { GetModuleHandleW(None) }.map_err(|e| format!("GetModuleHandleW: {e}"))?;
        let hinstance: HINSTANCE = hmodule.into();
        let icon = create_tray_icon()?;

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
        let taskbar_created_msg =
            unsafe { RegisterWindowMessageW(PCWSTR(taskbar_msg_name.as_ptr())) };
        TASKBAR_CREATED_MSG.store(taskbar_created_msg, Ordering::Relaxed);

        TRAY_STATE.with(|slot| {
            *slot.borrow_mut() = Some(TrayState {
                hinstance,
                tray_hwnd,
                settings_hwnd: None,
                icon,
                snapshot: initial,
                on_save: Box::new(on_save),
                on_exit: Some(Box::new(on_exit)),
                dialog_res: None,
                pills: PillStates::default(),
                combo_primary: HWND::default(),
                combo_secondary: HWND::default(),
                combo_key: HWND::default(),
                pill_ctrl: HWND::default(),
                pill_alt: HWND::default(),
                pill_shift: HWND::default(),
                pill_meta: HWND::default(),
                pill_autostart: HWND::default(),
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
                if let Some(res) = state.dialog_res.take() {
                    res.destroy();
                }
                if let Some(exit_cb) = state.on_exit.take() {
                    exit_cb();
                }
                let _ = unsafe { DestroyIcon(state.icon) };
            }
        });
        Ok(())
    }
}

fn init_common_controls() {
    let icc = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_STANDARD_CLASSES,
    };
    let _ = unsafe { InitCommonControlsEx(&icc) };
}

fn enable_uxtheme_dark_menus() {
    let dll = to_wide_null("uxtheme.dll");
    let Ok(module) = (unsafe { LoadLibraryW(PCWSTR(dll.as_ptr())) }) else {
        return;
    };
    unsafe {
        if let Some(f) = GetProcAddress(module, PCSTR(135usize as *const u8)) {
            let set_preferred: extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            let _ = set_preferred(2); // ForceDark to match Nord Calm
        }
        if let Some(f) = GetProcAddress(module, PCSTR(136usize as *const u8)) {
            let flush: extern "system" fn() = std::mem::transmute(f);
            flush();
        }
    }
}

/// Applies Windows 11 dark-mode combobox theme (`DarkMode_CFD`) dynamically via `uxtheme.dll`.
fn apply_dark_combobox_theme(hwnd: HWND) {
    let dll = to_wide_null("uxtheme.dll");
    let Ok(module) = (unsafe { LoadLibraryW(PCWSTR(dll.as_ptr())) }) else {
        return;
    };
    let proc_name = b"SetWindowTheme\0";
    let theme_name = to_wide_null("DarkMode_CFD");
    unsafe {
        if let Some(f) = GetProcAddress(module, PCSTR(proc_name.as_ptr())) {
            let set_theme: extern "system" fn(HWND, PCWSTR, PCWSTR) -> i32 = std::mem::transmute(f);
            let _ = set_theme(hwnd, PCWSTR(theme_name.as_ptr()), PCWSTR::null());
        }
    }
}

fn apply_nord_dwm_styling(hwnd: HWND) {
    let dark_val: i32 = 1;
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&dark_val as *const i32).cast(),
            std::mem::size_of::<i32>() as u32,
        )
    };
    // Seamless Nord Calm title bar matching the window background (#242933)
    let caption = NORD.window_bg.0;
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            (&caption as *const u32).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };
    let caption_text = NORD.text_muted.0;
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_TEXT_COLOR,
            (&caption_text as *const u32).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };
    let corner = DWMWCP_ROUND;
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&corner as *const DWM_WINDOW_CORNER_PREFERENCE).cast(),
            std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        )
    };
}

fn scale(px: i32, dpi: u32) -> i32 {
    (((px as i64 * dpi as i64) + 48) / 96) as i32
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
    let header_lbl = to_wide_null("LangFlip");
    let settings_lbl = to_wide_null("Settings...");
    let exit_lbl = to_wide_null("Exit");
    let _ = unsafe {
        AppendMenuW(
            menu,
            MF_STRING | MF_GRAYED,
            ID_MENU_HEADER,
            PCWSTR(header_lbl.as_ptr()),
        )
    };
    let _ = unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()) };
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
    let setup = TRAY_STATE.with(|slot| {
        let borrow = slot.borrow();
        let state = borrow.as_ref()?;
        Some((
            state.settings_hwnd,
            state.hinstance,
            state.tray_hwnd,
            state.snapshot.clone(),
        ))
    });

    let Some((existing_hwnd, hinst, tray_hwnd, snapshot)) = setup else {
        return;
    };

    if let Some(existing) = existing_hwnd {
        let _ = unsafe { ShowWindow(existing, SW_SHOW) };
        let _ = unsafe { SetForegroundWindow(existing) };
        return;
    }

    let base_dpi = unsafe { GetDpiForWindow(tray_hwnd) }.max(96);
    let width = scale(460, base_dpi);
    let height = scale(482, base_dpi);
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
            WS_CAPTION | WS_SYSMENU,
            x,
            y,
            width,
            height,
            Some(tray_hwnd),
            None,
            Some(hinst),
            None,
        )
    }) else {
        return;
    };

    apply_nord_dwm_styling(dlg);
    let dpi = unsafe { GetDpiForWindow(dlg) }.max(96);
    let res = DialogResources::new(dpi);
    let font_title = res.font_title;
    let font_section = res.font_section;
    let font_body = res.font_body;
    let font_sub = res.font_sub;
    let s = |px: i32| scale(px, dpi);

    // Header
    create_label(
        dlg,
        hinst,
        font_title,
        "Keyboard & Shortcut",
        Some(ID_LBL_TITLE),
        s(24),
        s(18),
        s(380),
        s(28),
    );
    create_label(
        dlg,
        hinst,
        font_sub,
        "Pick two installed layouts and your flip key",
        Some(ID_LBL_SUBTITLE),
        s(24),
        s(46),
        s(400),
        s(20),
    );

    // Card 1: Languages
    create_label(
        dlg,
        hinst,
        font_section,
        "LANGUAGES",
        Some(ID_LBL_SECTION_1),
        s(36),
        s(90),
        s(240),
        s(18),
    );
    create_label(
        dlg,
        hinst,
        font_body,
        "Primary",
        None,
        s(36),
        s(118),
        s(95),
        s(22),
    );
    let combo_primary = create_combobox(
        dlg,
        hinst,
        font_body,
        ID_COMBO_PRIMARY,
        s(140),
        s(114),
        s(268),
        s(180),
    );

    create_label(
        dlg,
        hinst,
        font_body,
        "Secondary",
        None,
        s(36),
        s(154),
        s(95),
        s(22),
    );
    let combo_secondary = create_combobox(
        dlg,
        hinst,
        font_body,
        ID_COMBO_SECONDARY,
        s(140),
        s(150),
        s(268),
        s(180),
    );

    // Card 2: Hotkey
    create_label(
        dlg,
        hinst,
        font_section,
        "HOTKEY",
        Some(ID_LBL_SECTION_2),
        s(36),
        s(212),
        s(240),
        s(18),
    );
    create_label(
        dlg,
        hinst,
        font_body,
        "Key",
        None,
        s(36),
        s(239),
        s(95),
        s(22),
    );
    let combo_key = create_combobox(
        dlg,
        hinst,
        font_body,
        ID_COMBO_KEY,
        s(140),
        s(235),
        s(146),
        s(220),
    );

    create_label(
        dlg,
        hinst,
        font_body,
        "Modifiers",
        None,
        s(36),
        s(273),
        s(95),
        s(22),
    );
    let pill_ctrl = create_ownerdraw_button(
        dlg,
        hinst,
        font_body,
        "Ctrl",
        ID_PILL_CTRL,
        s(140),
        s(269),
        s(56),
        s(25),
    );
    let pill_alt = create_ownerdraw_button(
        dlg,
        hinst,
        font_body,
        "Alt",
        ID_PILL_ALT,
        s(204),
        s(269),
        s(52),
        s(25),
    );
    let pill_shift = create_ownerdraw_button(
        dlg,
        hinst,
        font_body,
        "Shift",
        ID_PILL_SHIFT,
        s(264),
        s(269),
        s(60),
        s(25),
    );
    let pill_meta = create_ownerdraw_button(
        dlg,
        hinst,
        font_body,
        "Win",
        ID_PILL_META,
        s(332),
        s(269),
        s(56),
        s(25),
    );

    // Card 3: Startup
    create_label(
        dlg,
        hinst,
        font_section,
        "STARTUP",
        Some(ID_LBL_SECTION_3),
        s(36),
        s(328),
        s(240),
        s(18),
    );
    create_label(
        dlg,
        hinst,
        font_body,
        "Start automatically with Windows",
        None,
        s(36),
        s(353),
        s(290),
        s(22),
    );
    let pill_autostart = create_ownerdraw_button(
        dlg,
        hinst,
        font_body,
        "On",
        ID_PILL_AUTOSTART,
        s(348),
        s(349),
        s(60),
        s(25),
    );

    // Action buttons
    let _btn_save = create_ownerdraw_button(
        dlg,
        hinst,
        font_body,
        "Save",
        ID_BTN_SAVE,
        s(242),
        s(402),
        s(86),
        s(32),
    );
    let _btn_cancel = create_ownerdraw_button(
        dlg,
        hinst,
        font_body,
        "Cancel",
        ID_BTN_CANCEL,
        s(338),
        s(402),
        s(86),
        s(32),
    );

    // Populate clean human-readable installed language names
    let mut primary_idx = 0usize;
    let mut secondary_idx = usize::from(snapshot.installed_layouts.len() > 1);
    for (i, layout) in snapshot.installed_layouts.iter().enumerate() {
        combo_add_string(combo_primary, &layout.display_name);
        combo_add_string(combo_secondary, &layout.display_name);
        if layout.id == snapshot.layouts.primary {
            primary_idx = i;
        }
        if layout.id == snapshot.layouts.secondary {
            secondary_idx = i;
        }
    }
    combo_set_cursel(combo_primary, primary_idx);
    combo_set_cursel(combo_secondary, secondary_idx);

    // Populate hotkey dropdown
    let current_key_name = key_name_for(snapshot.hotkey.key).unwrap_or("Pause");
    let mut key_idx = 0usize;
    for (i, &name) in SUPPORTED_HOTKEY_NAMES.iter().enumerate() {
        combo_add_string(combo_key, name);
        if name.eq_ignore_ascii_case(current_key_name) {
            key_idx = i;
        }
    }
    combo_set_cursel(combo_key, key_idx);

    let initial_pills = PillStates {
        ctrl: snapshot.hotkey.ctrl,
        alt: snapshot.hotkey.alt,
        shift: snapshot.hotkey.shift,
        meta: snapshot.hotkey.meta,
        auto_start: snapshot.auto_start,
    };

    TRAY_STATE.with(|slot| {
        if let Some(state) = slot.borrow_mut().as_mut() {
            if let Some(old) = state.dialog_res.replace(res) {
                old.destroy();
            }
            state.settings_hwnd = Some(dlg);
            state.pills = initial_pills;
            state.combo_primary = combo_primary;
            state.combo_secondary = combo_secondary;
            state.combo_key = combo_key;
            state.pill_ctrl = pill_ctrl;
            state.pill_alt = pill_alt;
            state.pill_shift = pill_shift;
            state.pill_meta = pill_meta;
            state.pill_autostart = pill_autostart;
        }
    });

    let _ = unsafe { ShowWindow(dlg, SW_SHOW) };
    let _ = unsafe { SetForegroundWindow(dlg) };
}

fn toggle_pill(id: i32) {
    let target_hwnd = TRAY_STATE.with(|slot| {
        let mut borrow = slot.borrow_mut();
        let state = borrow.as_mut()?;
        match id {
            ID_PILL_CTRL => {
                state.pills.ctrl = !state.pills.ctrl;
                Some(state.pill_ctrl)
            }
            ID_PILL_ALT => {
                state.pills.alt = !state.pills.alt;
                Some(state.pill_alt)
            }
            ID_PILL_SHIFT => {
                state.pills.shift = !state.pills.shift;
                Some(state.pill_shift)
            }
            ID_PILL_META => {
                state.pills.meta = !state.pills.meta;
                Some(state.pill_meta)
            }
            ID_PILL_AUTOSTART => {
                state.pills.auto_start = !state.pills.auto_start;
                Some(state.pill_autostart)
            }
            _ => None,
        }
    });

    if let Some(hwnd) = target_hwnd {
        let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
    }
}

fn handle_settings_save(dlg: HWND) {
    let controls = TRAY_STATE.with(|slot| {
        let borrow = slot.borrow();
        let state = borrow.as_ref()?;
        Some((
            state.combo_primary,
            state.combo_secondary,
            state.combo_key,
            state.pills,
        ))
    });

    let Some((combo_primary, combo_secondary, combo_key, pills)) = controls else {
        return;
    };

    let p_idx = combo_get_cursel(combo_primary);
    let s_idx = combo_get_cursel(combo_secondary);
    let k_idx = combo_get_cursel(combo_key);

    let result: Result<(), String> = TRAY_STATE.with(|slot| {
        let mut borrow = slot.borrow_mut();
        let Some(state) = borrow.as_mut() else {
            return Err("internal tray state missing".into());
        };

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
            ctrl: pills.ctrl,
            alt: pills.alt,
            shift: pills.shift,
            meta: pills.meta,
        };
        let new_pair = LayoutPair { primary, secondary };

        (state.on_save)(new_pair.clone(), new_hotkey, pills.auto_start)?;
        state.snapshot.layouts = new_pair;
        state.snapshot.hotkey = new_hotkey;
        state.snapshot.auto_start = pills.auto_start;
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
    let taskbar_msg = TASKBAR_CREATED_MSG.load(Ordering::Relaxed);
    if taskbar_msg != 0 && msg == taskbar_msg {
        let icon_opt = TRAY_STATE.with(|s| s.borrow().as_ref().map(|st| (st.tray_hwnd, st.icon)));
        if let Some((tray_hwnd, icon)) = icon_opt {
            let _ = add_tray_icon(tray_hwnd, icon);
        }
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
        WM_PAINT => {
            let paint_info = TRAY_STATE.with(|slot| {
                slot.borrow()
                    .as_ref()
                    .and_then(|s| s.dialog_res.as_ref())
                    .map(|r| (r.dpi, r.window_brush, r.card_brush))
            });
            if let Some((dpi, win_brush, card_brush)) = paint_info {
                let mut ps = PAINTSTRUCT::default();
                let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
                let mut rc = RECT::default();
                let _ = unsafe { GetClientRect(hwnd, &mut rc) };
                unsafe { FillRect(hdc, &rc, win_brush) };

                let s = |px: i32| scale(px, dpi);
                let radius = s(10);
                let pen = unsafe { CreatePen(PS_SOLID, 1, NORD.card_border) };
                let old_pen = unsafe { SelectObject(hdc, pen.into()) };
                let old_brush = unsafe { SelectObject(hdc, card_brush.into()) };

                // Card 1: Languages
                let _ = unsafe { RoundRect(hdc, s(20), s(78), s(424), s(188), radius, radius) };
                // Card 2: Hotkey
                let _ = unsafe { RoundRect(hdc, s(20), s(200), s(424), s(304), radius, radius) };
                // Card 3: Startup
                let _ = unsafe { RoundRect(hdc, s(20), s(316), s(424), s(388), radius, radius) };

                unsafe {
                    SelectObject(hdc, old_brush);
                    SelectObject(hdc, old_pen);
                    let _ = DeleteObject(pen.into());
                    let _ = EndPaint(hwnd, &ps);
                }
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            let hdc = HDC(wparam.0 as *mut std::ffi::c_void);
            let ctrl = HWND(lparam.0 as *mut std::ffi::c_void);
            let ctrl_id = unsafe { GetDlgCtrlID(ctrl) };
            let brushes = TRAY_STATE.with(|slot| {
                slot.borrow()
                    .as_ref()
                    .and_then(|s| s.dialog_res.as_ref())
                    .map(|r| (r.window_brush, r.card_brush))
            });
            if let Some((win_brush, card_brush)) = brushes {
                let is_header = ctrl_id == ID_LBL_TITLE || ctrl_id == ID_LBL_SUBTITLE;
                let text_col = match ctrl_id {
                    ID_LBL_TITLE => NORD.text_title,
                    ID_LBL_SUBTITLE => NORD.text_muted,
                    ID_LBL_SECTION_1 | ID_LBL_SECTION_2 | ID_LBL_SECTION_3 => NORD.section_ice,
                    _ => NORD.text_primary,
                };
                let (bg_col, brush) = if is_header {
                    (NORD.window_bg, win_brush)
                } else {
                    (NORD.card_bg, card_brush)
                };
                unsafe {
                    SetTextColor(hdc, text_col);
                    SetBkColor(hdc, bg_col);
                }
                return LRESULT(brush.0 as isize);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_CTLCOLORLISTBOX | WM_CTLCOLOREDIT => {
            let hdc = HDC(wparam.0 as *mut std::ffi::c_void);
            let input_brush = TRAY_STATE.with(|slot| {
                slot.borrow()
                    .as_ref()
                    .and_then(|s| s.dialog_res.as_ref())
                    .map(|r| r.input_brush)
            });
            if let Some(brush) = input_brush {
                unsafe {
                    SetTextColor(hdc, NORD.text_title);
                    SetBkColor(hdc, NORD.input_bg);
                }
                return LRESULT(brush.0 as isize);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_DRAWITEM => {
            let dis = unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) };
            let info = TRAY_STATE.with(|slot| {
                let borrow = slot.borrow();
                let st = borrow.as_ref()?;
                let r = st.dialog_res.as_ref()?;
                Some((
                    r.dpi,
                    r.font_body,
                    r.font_pill,
                    r.font_pill_bold,
                    r.window_brush,
                    r.card_brush,
                    st.pills,
                ))
            });
            if let Some((dpi, font_body, font_pill, font_pill_bold, win_brush, card_brush, pills)) =
                info
            {
                draw_owner_control(
                    dis,
                    dpi,
                    font_body,
                    font_pill,
                    font_pill_bold,
                    win_brush,
                    card_brush,
                    pills,
                );
                return LRESULT(1);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as i32;
            match id {
                ID_PILL_CTRL | ID_PILL_ALT | ID_PILL_SHIFT | ID_PILL_META | ID_PILL_AUTOSTART => {
                    toggle_pill(id);
                    return LRESULT(0);
                }
                ID_BTN_SAVE => {
                    handle_settings_save(hwnd);
                    return LRESULT(0);
                }
                ID_BTN_CANCEL => {
                    let _ = unsafe { DestroyWindow(hwnd) };
                    return LRESULT(0);
                }
                _ => {}
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
                    if let Some(res) = state.dialog_res.take() {
                        res.destroy();
                    }
                }
            });
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_owner_control(
    dis: &DRAWITEMSTRUCT,
    dpi: u32,
    font_body: HFONT,
    font_pill: HFONT,
    font_pill_bold: HFONT,
    win_brush: HBRUSH,
    card_brush: HBRUSH,
    pills: PillStates,
) {
    let hdc = dis.hDC;
    let rc = dis.rcItem;
    let id = dis.CtlID as i32;
    let pressed = (dis.itemState.0 & ODS_SELECTED.0) != 0;

    let (backdrop, fill_col, border_col, text_col, font, label, radius_px) = match id {
        ID_BTN_SAVE => {
            let bg = if pressed { NORD.save_pressed } else { NORD.save_bg };
            (win_brush, bg, bg, NORD.text_title, font_body, "Save", 7)
        }
        ID_BTN_CANCEL => {
            let bg = if pressed {
                NORD.cancel_pressed
            } else {
                NORD.cancel_bg
            };
            (
                win_brush,
                bg,
                NORD.input_border,
                NORD.text_primary,
                font_body,
                "Cancel",
                7,
            )
        }
        ID_PILL_CTRL | ID_PILL_ALT | ID_PILL_SHIFT | ID_PILL_META | ID_PILL_AUTOSTART => {
            let (active, text) = match id {
                ID_PILL_CTRL => (pills.ctrl, "Ctrl"),
                ID_PILL_ALT => (pills.alt, "Alt"),
                ID_PILL_SHIFT => (pills.shift, "Shift"),
                ID_PILL_META => (pills.meta, "Win"),
                _ => (pills.auto_start, if pills.auto_start { "On" } else { "Off" }),
            };
            if active {
                (
                    card_brush,
                    NORD.pill_on_bg,
                    NORD.pill_on_border,
                    NORD.pill_on_text,
                    font_pill_bold,
                    text,
                    6,
                )
            } else {
                (
                    card_brush,
                    NORD.input_bg,
                    NORD.input_border,
                    NORD.text_muted,
                    font_pill,
                    text,
                    6,
                )
            }
        }
        _ => return,
    };

    unsafe {
        FillRect(hdc, &rc, backdrop);
        let brush = CreateSolidBrush(fill_col);
        let pen = CreatePen(PS_SOLID, 1, border_col);
        let old_b = SelectObject(hdc, brush.into());
        let old_p = SelectObject(hdc, pen.into());
        let old_f = SelectObject(hdc, font.into());

        let r = scale(radius_px, dpi);
        let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, r, r);

        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, text_col);
        let mut text_wide = to_wide_null(label);
        let mut text_rc = rc;
        let _ = DrawTextW(
            hdc,
            &mut text_wide[..label.encode_utf16().count()],
            &mut text_rc,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );

        SelectObject(hdc, old_f);
        SelectObject(hdc, old_p);
        SelectObject(hdc, old_b);
        let _ = DeleteObject(pen.into());
        let _ = DeleteObject(brush.into());
    }
}

fn create_ui_font(px_height: i32, semibold: bool) -> HFONT {
    let face = to_wide_null("Segoe UI");
    let weight = if semibold { FW_SEMIBOLD.0 } else { FW_NORMAL.0 } as i32;
    unsafe {
        CreateFontW(
            -px_height,
            0,
            0,
            0,
            weight,
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
    id: Option<i32>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> HWND {
    let cls = to_wide_null("STATIC");
    let txt = to_wide_null(text);
    let menu = id.map(|v| HMENU(v as isize as *mut std::ffi::c_void));
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
            menu,
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
            Some(HMENU(id as isize as *mut std::ffi::c_void)),
            Some(hinst),
            None,
        )
    }
    .unwrap_or_default();
    apply_font(hwnd, font);
    apply_dark_combobox_theme(hwnd);
    hwnd
}

#[allow(clippy::too_many_arguments)]
fn create_ownerdraw_button(
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
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
            x,
            y,
            w,
            h,
            Some(parent),
            Some(HMENU(id as isize as *mut std::ffi::c_void)),
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

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
