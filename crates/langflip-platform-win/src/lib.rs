//! Thin Windows 11 Win32 adapter for LangFlip.
//!
//! All Win32 FFI lives behind `#[cfg(target_os = "windows")]` so the workspace
//! compiles cleanly on non-Windows hosts during native TDD runs.
