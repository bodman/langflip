//! Thin Windows 11 Win32 adapter for LangFlip (`T030`).
//!
//! All Win32 FFI lives behind `#[cfg(target_os = "windows")]` so the workspace
//! compiles and tests cleanly on non-Windows hosts during native TDD runs.

#[cfg(target_os = "windows")]
pub mod focus;
#[cfg(target_os = "windows")]
pub mod hook;
#[cfg(target_os = "windows")]
pub mod injector;
#[cfg(target_os = "windows")]
pub mod layout;
