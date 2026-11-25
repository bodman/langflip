# Implementation Plan: LangFlip — System Tray Icon, Context Menu & Settings Dialog

**Branch**: `003-system-tray-settings` | **Date**: 2026-11-26 | **Spec**: [spec.md](spec.md)

## Summary

Upgrade `langflip.exe` into a native Windows 11 background system-tray utility with zero open console window, a custom crisp RGBA tray icon, a right-click context menu (**Settings**, **Exit**), a native Settings dialog for choosing the two installed keyboard languages and trigger hotkey, live runtime reload without restart, and `langflip.toml` persistence. Implemented with pure Win32 APIs via the already-cached `windows` crate (zero new external crate dependencies).

## Technical Context

- **Language/Version**: Rust `1.100.0` (isolated `.toolchain/`, `--locked`)
- **Win32 Features Added**: `Win32_UI_Shell`, `Win32_Graphics_Gdi`, `Win32_UI_Controls`, `Win32_System_LibraryLoader`, `Win32_System_Console` (all present in cached `windows` 0.61.3 — zero `Cargo.lock` churn).
- **Architecture**:
  - `langflip-core` (100% Linux TDD):
    - `key::SUPPORTED_HOTKEY_NAMES` + `key::key_name_for(PhysicalKey)` (DRY single table shared by `parse_key_name`, `to_toml`, and the Settings UI).
    - `AppConfig::to_toml()`, `resolve_save_path()`, `save_to_path()`.
    - `ConversionEngine::update_config(layouts, hotkey)` (live reload, clears active burst).
  - `langflip-platform-win`:
    - `icon.rs`: Crisp 32×32 RGBA custom LangFlip tray icon (`CreateIconIndirect` / `CreateDIBSection` with alpha channel).
    - `tray.rs`: Single-instance mutex (`CreateMutexW`), `Shell_NotifyIconW` (`TaskbarCreated` recovery), right-click popup menu (**Settings**, **Exit**), and native Win32 Settings dialog (Primary/Secondary installed-layout ComboBoxes, Hotkey ComboBox + `Ctrl`/`Alt`/`Shift`/`Win` checkboxes, `Save`/`Cancel`).
    - `hook.rs`: Live hotkey update via `PostThreadMessageW(WM_APP_UPDATE_HOTKEY)` (zero mutex locking inside `keyboard_proc`).
  - `langflip-app`:
    - `#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]` (no console window; optional `AttachConsole(ATTACH_PARENT_PROCESS)` when `--metrics` is passed).
    - Main thread runs Win32 tray + Settings dialog message loop; hook thread runs `WH_KEYBOARD_LL`/`WH_MOUSE_LL`/`SetWinEventHook`; worker thread runs `ConversionEngine`.

## Constitution Check

- **I. Cross-Platform Core**: PASS — Config serialization, key-name table, validation, and live engine reload live in `langflip-core`; Win32 tray/dialog lives strictly in `langflip-platform-win`.
- **II. Incremental Complexity**: PASS — Zero external GUI framework bloat; pure Win32 native controls.
- **III. Isolated Tooling**: PASS — Zero new external crates; `--locked --offline` stays green.
- **IV. Strict TDD**: PASS — Core serialization, reverse key lookup, save-path resolution, and live engine reload written Red → Green first.
- **V. Privacy & Responsiveness**: PASS — Hook callback remains lock-free and < 1 ms; no keystrokes logged or persisted.
- **VI. DRY**: PASS — Single `KEY_NAME_TABLE` in `langflip-core::key` powers parsing, serialization, and UI dropdown.
