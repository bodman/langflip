# Feature Specification: Auto-Start with Windows Setting

**Feature Branch**: `006-autostart-setting`  
**Created**: 2026-11-26  
**Status**: Implemented  

## Summary

Adds a built-in **Start with Windows** setting (`behavior.auto_start`, enabled by default) controllable from the native Nord Calm Settings dialog and persisted in `langflip.toml`.

## Behavior

1. **Default Enabled (`auto_start = true`)**:
   - `AppConfig::default().auto_start` is `true`, and omitting `behavior.auto_start` in `langflip.toml` defaults to `true`.
2. **Per-User Windows Startup Registration (No Admin Required)**:
   - Uses `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value name `"LangFlip"`.
   - When enabled, writes `"\"<current_exe_path>\""` as `REG_SZ`.
   - Synced on every startup (automatically keeping the path up to date if the user moves `langflip.exe`) and whenever Settings are saved.
   - When disabled, deletes the `"LangFlip"` value (idempotent if already absent).
3. **Settings UI**:
   - Adds a third Nord Calm card (`STARTUP`) in the Settings dialog with a `Launch LangFlip when Windows starts` row and an `On` / `Off` Nord ice toggle pill.
