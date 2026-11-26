---
description: "TDD-ordered task breakdown for Spec 003 — System Tray Icon, Context Menu & Settings Dialog"
---

# Tasks: LangFlip — System Tray Icon, Context Menu & Settings Dialog

**Input**: [spec.md](spec.md), [plan.md](plan.md)

## Phase 1: Core Serialization, Key Table & Live Reload (Strict TDD on Linux)

- [x] T001 [US2] Write failing unit tests for `key_name_for(PhysicalKey)` and `SUPPORTED_HOTKEY_NAMES` round-tripping with `parse_key_name` in `crates/langflip-core/src/key.rs` (verify failure)
- [x] T002 [US2] Refactor `key.rs` to a single DRY `KEY_NAME_TABLE` implementing `parse_key_name`, `key_name_for`, and `SUPPORTED_HOTKEY_NAMES` until T001 tests pass
- [x] T003 [US2] Write failing unit tests for `AppConfig::to_toml()`, round-trip `from_toml(&cfg.to_toml())`, `resolve_save_path()`, and `save_to_path()` (preserving `max_buffer_len`, `replay_delay_ms`, `toggle_on_empty_buffer`) in `crates/langflip-core/src/config.rs` (verify failure)
- [x] T004 [US2] Implement `to_toml`, `resolve_save_path`, and `save_to_path` in `crates/langflip-core/src/config.rs` until T003 tests pass
- [x] T005 [US2] Write failing integration test for `ConversionEngine::update_config(layouts, hotkey)` (clears active burst, switches active hotkey & layout pair live) in `crates/langflip-core/tests/engine_tests.rs` (verify failure)
- [x] T006 [US2] Implement `ConversionEngine::update_config` in `crates/langflip-core/src/engine.rs` and `HotkeyMatcher::set_spec` in `crates/langflip-core/src/hotkey.rs` until T005 passes

## Phase 2: Windows 11 System Tray, Custom Icon & Settings Dialog

- [x] T007 [US1] Enable `Win32_UI_Shell`, `Win32_Graphics_Gdi`, `Win32_UI_Controls`, `Win32_System_LibraryLoader`, and `Win32_System_Console` in root `Cargo.toml`
- [x] T008 [P] [US1] Implement custom 32×32 RGBA LangFlip tray icon generator in `crates/langflip-platform-win/src/icon.rs`
- [x] T009 [P] [US2] Add lock-free live hotkey reload (`WinShutdownHandle::update_hotkey` via `PostThreadMessageW`) in `crates/langflip-platform-win/src/hook.rs`
- [x] T010 [US1] Implement single-instance mutex, `Shell_NotifyIconW` tray icon (`TaskbarCreated` recovery), right-click context menu (**Settings**, **Exit**), and native Win32 Settings dialog (installed language dropdowns, hotkey dropdown + modifier checkboxes, validation, Save/Cancel) in `crates/langflip-platform-win/src/tray.rs` and export in `lib.rs`
- [x] T011 [US1] Update `crates/langflip-app/src/main.rs` to use `#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]` (no console window; `AttachConsole` when `--metrics`), wire tray UI + Settings save/live-reload + clean Exit teardown

## Phase 3: Verification & Windows 11 Live Check

- [x] T012 Run `./scripts/cargo.sh test --workspace --locked --offline`, Linux + Windows `clippy -- -D warnings`, cross-compile `langflip.exe`, copy to `C:\Users\omril\Downloads\langflip.exe`, and verify tray icon, right-click menu (**Settings** / **Exit**), Settings dialog save & live reload, persistence across restart, single-instance guard, and zero regressions
