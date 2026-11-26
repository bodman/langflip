//! Modern Windows 11 system tray icon, Fluent-themed right-click context menu
//! (Settings / Exit), single-instance mutex guard, and styled Settings dialog.

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
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CreateFontW, CreatePen, CreateSolidBrush, DEFAULT_CHARSET,
    DEFAULT_PITCH, DeleteObject, DrawTextW, DT_CENTER, DT_SINGLELINE, DT_VCENTER, EndPaint,
    FF_SWISS, FW_NORMAL, FW_SEMIBOLD, FillRect, HBRUSH, HDC, HFONT, OUT_DEFAULT_PRECIS,
    PAINTSTRUCT, PS_SOLID, RoundRect, SelectObject, SetBkColor, SetBkMode, SetTextColor,
    TRANSPARENT,
};
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, REG_DWORD, RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Controls::{
    BST_CHECKED, BST_UNCHECKED, DRAWITEMSTRUCT, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX,
    InitCommonControlsEx, ODS_SELECTED,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_OWNERDRAW, CB_ADDSTRING,
    CB_GETCURSEL, CB_SETCURSEL, CBS_DROPDOWNLIST, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, GetClientRect, GetCursorPos,
    GetMessageW, GetSystemMetrics, HICON, HMENU, IDC_ARROW, LoadCursorW, MB_ICONWARNING, MB_OK,
    MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, MessageBoxW, PostMessageW, PostQuitMessage,
    RegisterClassW, RegisterWindowMessageW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, SendMessageW,
    SetForegroundWindow, ShowWindow, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON,
    TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND,
    WM_CTLCOLORBTN, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DRAWITEM, WM_NULL, WM_PAINT, WM_RBUTTONUP,
    WM_SETFONT, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
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
const ID_CHK_CTRL: i32 = 2004;
const ID_CHK_ALT: i32 = 2005;
const ID_CHK_SHIFT: i32 = 2006;
const ID_CHK_META: i32 = 2007;
const ID_BTN_SAVE: i32 = 2008;
const ID_BTN_CANCEL: i32 = 2009;
const ID_LBL_SUBTITLE: i32 = 2010;
const ID_LBL_SECTION_1: i32 = 2011;
const ID_LBL_SECTION_2: i32 = 2012;
const ID_LBL_TITLE: i32 = 2013;

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

#[derive(Clone, Copy)]
struct ThemePalette {
    dark: bool,
    window_bg: COLORREF,
    card_bg: COLORREF,
    card_border: COLORREF,
    text_primary: COLORREF,
    text_muted: COLORREF,
    accent: COLORREF,
    accent_pressed: COLORREF,
    secondary_btn_bg: COLORREF,
    secondary_btn_pressed: COLORREF,
    secondary_btn_border: COLORREF,
}

impl ThemePalette {
    fn for_system() -> Self {
        if is_windows_dark_mode() {
            Self {
                dark: true,
                window_bg: rgb(28, 28, 30),
                card_bg: rgb(40, 40, 44),
                card_border: rgb(62, 62, 68),
                text_primary: rgb(243, 244, 246),
                text_muted: rgb(156, 163, 175),
                accent: rgb(59, 130, 246),
                accent_pressed: rgb(37, 99, 235),
                secondary_btn_bg: rgb(50, 50, 56),
                secondary_btn_pressed: rgb(64, 64, 72),
                secondary_btn_border: rgb(82, 82, 91),
            }
        } else {
            Self {
                dark: false,
                window_bg: rgb(243, 244, 246),
                card_bg: rgb(255, 255, 255),
                card_border: rgb(226, 232, 240),
                text_primary: rgb(15, 23, 42),
                text_muted: rgb(100, 116, 139),
                accent: rgb(37, 99, 235),
                accent_pressed: rgb(29, 78, 216),
                secondary_btn_bg: rgb(255, 255, 255),
                secondary_btn_pressed: rgb(241, 245, 249),
                secondary_btn_border: rgb(203, 213, 225),
            }
        }
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}

type SaveCallback = Box<dyn FnMut(LayoutPair, HotkeySpec) -> Result<(), String>>;
type ExitCallback = Box<dyn FnOnce()>;

static TASKBAR_CREATED_MSG: AtomicU32 = AtomicU32::new(0);

struct DialogResources {
    palette: ThemePalette,
    dpi: u32,
    window_brush: HBRUSH,
    card_brush: HBRUSH,
    font_title: HFONT,
    font_section: HFONT,
    font_body: HFONT,
    font_sub: HFONT,
}

impl DialogResources {
    fn new(palette: ThemePalette, dpi: u32) -> Self {
        Self {
            palette,
            dpi,
            window_brush: unsafe { CreateSolidBrush(palette.window_bg) },
            card_brush: unsafe { CreateSolidBrush(palette.card_bg) },
            font_title: create_ui_font(scale(20, dpi), true),
            font_section: create_ui_font(scale(13, dpi), true),
            font_body: create_ui_font(scale(14, dpi), false),
            font_sub: create_ui_font(scale(13, dpi), false),
        }
    }

    fn destroy(self) {
        unsafe {
            let _ = DeleteObject(self.window_brush.into());
            let _ = DeleteObject(self.card_brush.into());
            let _ = DeleteObject(self.font_title.into());
            let _ = DeleteObject(self.font_section.into());
            let _ = DeleteObject(self.font_body.into());
            let _ = DeleteObject(self.font_sub.into());
        }
    }
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

/// Enables Windows 10/11 dark-mode support for native popup menus via undocumented
/// `uxtheme.dll` ordinals 135 (`SetPreferredAppMode(AllowDark)`) and 136 (`FlushMenuThemes`).
/// Fails silently on older Windows builds.
fn enable_uxtheme_dark_menus() {
    let dll = to_wide_null("uxtheme.dll");
    let Ok(module) = (unsafe { LoadLibraryW(PCWSTR(dll.as_ptr())) }) else {
        return;
    };
    unsafe {
        if let Some(f) = GetProcAddress(module, PCSTR(135usize as *const u8)) {
            let set_preferred: extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            let _ = set_preferred(1); // AllowDark
        }
        if let Some(f) = GetProcAddress(module, PCSTR(136usize as *const u8)) {
            let flush: extern "system" fn() = std::mem::transmute(f);
            flush();
        }
    }
}

fn is_windows_dark_mode() -> bool {
    let subkey = to_wide_null("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    let mut key = HKEY::default();
    if unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            KEY_READ,
            &mut key,
        )
    }
    .is_err()
    {
        return false;
    }

    let val_name = to_wide_null("AppsUseLightTheme");
    let mut reg_type = REG_DWORD;
    let mut data = 1u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let ok = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(val_name.as_ptr()),
            None,
            Some(&mut reg_type),
            Some((&mut data as *mut u32).cast()),
            Some(&mut size),
        )
        .is_ok()
    };
    let _ = unsafe { RegCloseKey(key) };
    ok && data == 0
}

fn apply_dwm_window_styling(hwnd: HWND, dark: bool) {
    let dark_val: i32 = i32::from(dark);
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&dark_val as *const i32).cast(),
            std::mem::size_of::<i32>() as u32,
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

    let palette = ThemePalette::for_system();
    let base_dpi = unsafe { GetDpiForWindow(tray_hwnd) }.max(96);
    let width = scale(460, base_dpi);
    let height = scale(390, base_dpi);
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

    apply_dwm_window_styling(dlg, palette.dark);
    let dpi = unsafe { GetDpiForWindow(dlg) }.max(96);
    let res = DialogResources::new(palette, dpi);
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
        "LangFlip Settings",
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
        "Configure your keyboard language pair and trigger shortcut",
        Some(ID_LBL_SUBTITLE),
        s(24),
        s(46),
        s(400),
        s(20),
    );

    // Card 1: Keyboard Languages (card painted at x=20..424, y=78..186)
    create_label(
        dlg,
        hinst,
        font_section,
        "KEYBOARD LANGUAGES",
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

    // Card 2: Trigger Hotkey (card painted at x=20..424, y=200..294)
    create_label(
        dlg,
        hinst,
        font_section,
        "TRIGGER HOTKEY",
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
        s(238),
        s(95),
        s(22),
    );
    let combo_key = create_combobox(
        dlg,
        hinst,
        font_body,
        ID_COMBO_KEY,
        s(140),
        s(234),
        s(150),
        s(220),
    );

    create_label(
        dlg,
        hinst,
        font_body,
        "Modifiers",
        None,
        s(36),
        s(270),
        s(95),
        s(22),
    );
    let chk_ctrl = create_checkbox(
        dlg,
        hinst,
        font_body,
        "Ctrl",
        ID_CHK_CTRL,
        s(140),
        s(268),
        s(56),
        s(22),
    );
    let chk_alt = create_checkbox(
        dlg,
        hinst,
        font_body,
        "Alt",
        ID_CHK_ALT,
        s(202),
        s(268),
        s(52),
        s(22),
    );
    let chk_shift = create_checkbox(
        dlg,
        hinst,
        font_body,
        "Shift",
        ID_CHK_SHIFT,
        s(260),
        s(268),
        s(62),
        s(22),
    );
    let chk_meta = create_checkbox(
        dlg,
        hinst,
        font_body,
        "Win",
        ID_CHK_META,
        s(328),
        s(268),
        s(60),
        s(22),
    );

    // Action buttons (owner-drawn modern rounded pills)
    let _btn_save = create_ownerdraw_button(
        dlg,
        hinst,
        font_body,
        "Save",
        ID_BTN_SAVE,
        s(234),
        s(308),
        s(90),
        s(32),
    );
    let _btn_cancel = create_ownerdraw_button(
        dlg,
        hinst,
        font_body,
        "Cancel",
        ID_BTN_CANCEL,
        s(334),
        s(308),
        s(90),
        s(32),
    );

    // Populate installed languages
    let mut primary_idx = 0usize;
    let mut secondary_idx = usize::from(snapshot.installed_layouts.len() > 1);
    for (i, layout) in snapshot.installed_layouts.iter().enumerate() {
        let label = format!(
            "{} ({})",
            layout.display_name,
            layout.id.as_str().to_ascii_uppercase()
        );
        combo_add_string(combo_primary, &label);
        combo_add_string(combo_secondary, &label);
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

    set_checked(chk_ctrl, snapshot.hotkey.ctrl);
    set_checked(chk_alt, snapshot.hotkey.alt);
    set_checked(chk_shift, snapshot.hotkey.shift);
    set_checked(chk_meta, snapshot.hotkey.meta);

    TRAY_STATE.with(|slot| {
        if let Some(state) = slot.borrow_mut().as_mut() {
            if let Some(old) = state.dialog_res.replace(res) {
                old.destroy();
            }
            state.settings_hwnd = Some(dlg);
            state.combo_primary = combo_primary;
            state.combo_secondary = combo_secondary;
            state.combo_key = combo_key;
            state.chk_ctrl = chk_ctrl;
            state.chk_alt = chk_alt;
            state.chk_shift = chk_shift;
            state.chk_meta = chk_meta;
        }
    });

    let _ = unsafe { ShowWindow(dlg, SW_SHOW) };
    let _ = unsafe { SetForegroundWindow(dlg) };
}

fn handle_settings_save(dlg: HWND) {
    let controls = TRAY_STATE.with(|slot| {
        let borrow = slot.borrow();
        let state = borrow.as_ref()?;
        Some((
            state.combo_primary,
            state.combo_secondary,
            state.combo_key,
            state.chk_ctrl,
            state.chk_alt,
            state.chk_shift,
            state.chk_meta,
        ))
    });

    let Some((combo_primary, combo_secondary, combo_key, chk_ctrl, chk_alt, chk_shift, chk_meta)) =
        controls
    else {
        return;
    };

    let p_idx = combo_get_cursel(combo_primary);
    let s_idx = combo_get_cursel(combo_secondary);
    let k_idx = combo_get_cursel(combo_key);
    let ctrl = is_checked(chk_ctrl);
    let alt = is_checked(chk_alt);
    let shift = is_checked(chk_shift);
    let meta = is_checked(chk_meta);

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
            ctrl,
            alt,
            shift,
            meta,
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
                    .map(|r| (r.palette, r.dpi, r.window_brush, r.card_brush))
            });
            if let Some((palette, dpi, win_brush, card_brush)) = paint_info {
                let mut ps = PAINTSTRUCT::default();
                let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
                let mut rc = RECT::default();
                let _ = unsafe { GetClientRect(hwnd, &mut rc) };
                unsafe { FillRect(hdc, &rc, win_brush) };

                let s = |px: i32| scale(px, dpi);
                let radius = s(10);
                let pen = unsafe { CreatePen(PS_SOLID, 1, palette.card_border) };
                let old_pen = unsafe { SelectObject(hdc, pen.into()) };
                let old_brush = unsafe { SelectObject(hdc, card_brush.into()) };

                // Card 1: Keyboard Languages
                let _ = unsafe { RoundRect(hdc, s(20), s(78), s(424), s(188), radius, radius) };
                // Card 2: Trigger Hotkey
                let _ = unsafe { RoundRect(hdc, s(20), s(200), s(424), s(296), radius, radius) };

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
            let ctrl_id = unsafe { windows::Win32::UI::WindowsAndMessaging::GetDlgCtrlID(ctrl) };
            let colors = TRAY_STATE.with(|slot| {
                slot.borrow()
                    .as_ref()
                    .and_then(|s| s.dialog_res.as_ref())
                    .map(|r| (r.palette, r.window_brush, r.card_brush))
            });
            if let Some((palette, win_brush, card_brush)) = colors {
                let is_header = ctrl_id == ID_LBL_TITLE || ctrl_id == ID_LBL_SUBTITLE;
                let is_muted = ctrl_id == ID_LBL_SUBTITLE
                    || ctrl_id == ID_LBL_SECTION_1
                    || ctrl_id == ID_LBL_SECTION_2;
                let text_col = if is_muted {
                    palette.text_muted
                } else {
                    palette.text_primary
                };
                let (bg_col, brush) = if is_header {
                    (palette.window_bg, win_brush)
                } else {
                    (palette.card_bg, card_brush)
                };
                unsafe {
                    SetTextColor(hdc, text_col);
                    SetBkColor(hdc, bg_col);
                }
                return LRESULT(brush.0 as isize);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_DRAWITEM => {
            let dis = unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) };
            let info = TRAY_STATE.with(|slot| {
                slot.borrow()
                    .as_ref()
                    .and_then(|s| s.dialog_res.as_ref())
                    .map(|r| (r.palette, r.dpi, r.font_body, r.window_brush))
            });
            if let Some((palette, dpi, font, win_brush)) = info {
                draw_modern_button(dis, palette, dpi, font, win_brush);
                return LRESULT(1);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
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

fn draw_modern_button(
    dis: &DRAWITEMSTRUCT,
    palette: ThemePalette,
    dpi: u32,
    font: HFONT,
    win_brush: HBRUSH,
) {
    let hdc = dis.hDC;
    let rc = dis.rcItem;
    let is_save = dis.CtlID as i32 == ID_BTN_SAVE;
    let pressed = (dis.itemState.0 & ODS_SELECTED.0) != 0;

    let (fill_col, border_col, text_col, label) = if is_save {
        let bg = if pressed {
            palette.accent_pressed
        } else {
            palette.accent
        };
        (bg, bg, rgb(255, 255, 255), "Save")
    } else {
        let bg = if pressed {
            palette.secondary_btn_pressed
        } else {
            palette.secondary_btn_bg
        };
        (bg, palette.secondary_btn_border, palette.text_primary, "Cancel")
    };

    unsafe {
        FillRect(hdc, &rc, win_brush);
        let brush = CreateSolidBrush(fill_col);
        let pen = CreatePen(PS_SOLID, 1, border_col);
        let old_b = SelectObject(hdc, brush.into());
        let old_p = SelectObject(hdc, pen.into());
        let old_f = SelectObject(hdc, font.into());

        let r = scale(7, dpi);
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
            Some(HMENU(id as isize as *mut std::ffi::c_void)),
            Some(hinst),
            None,
        )
    }
    .unwrap_or_default();
    apply_font(hwnd, font);
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
