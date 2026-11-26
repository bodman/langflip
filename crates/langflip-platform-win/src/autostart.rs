//! Per-user Windows startup registration (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`).
//!
//! Requires no administrator privileges and keeps the registered executable path
//! up to date if `langflip.exe` is moved.

use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
    RegSetValueExW,
};
use windows::core::PCWSTR;

const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "LangFlip";

/// Enables or disables automatic launch of the current `langflip.exe` at Windows logon.
pub fn sync_auto_start(enabled: bool) -> Result<(), String> {
    let subkey = to_wide_null(RUN_SUBKEY);
    let value_name = to_wide_null(VALUE_NAME);
    let mut hkey = HKEY::default();

    let open_res = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            KEY_SET_VALUE,
            &mut hkey,
        )
    };
    if let Err(err) = open_res.ok() {
        return Err(format!("failed to open Windows startup registry key: {err}"));
    }

    let op_res = if enabled {
        let exe = std::env::current_exe()
            .map_err(|e| format!("failed to locate current executable: {e}"))?;
        let quoted = format!("\"{}\"", exe.display());
        let wide = to_wide_null(&quoted);
        let bytes: &[u8] =
            unsafe { std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2) };
        unsafe {
            RegSetValueExW(
                hkey,
                PCWSTR(value_name.as_ptr()),
                Some(0),
                REG_SZ,
                Some(bytes),
            )
        }
        .ok()
        .map_err(|e| format!("failed to register auto-start: {e}"))
    } else {
        let status = unsafe { RegDeleteValueW(hkey, PCWSTR(value_name.as_ptr())) };
        if status.is_ok() || status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(format!("failed to remove auto-start entry: {status:?}"))
        }
    };

    let _ = unsafe { RegCloseKey(hkey) };
    op_res
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
