# Research & Technical Decisions: LangFlip MVP

**Feature**: `001-mvp-layout-converter` | **Date**: 2026-11-25 (Revised after Design Review)

## 1. Declarative Dependencies, Pinned Isolated Rust Toolchain & Cross-Compilation

- **Decision**: Declare **all** project dependencies in version-controlled configuration so any fresh clone is self-describing and reproducible:
  - `toolchain/deps.conf`: Declarative list of required host binaries and package names across common Linux package managers (`apt`, `dnf`, `pacman`) — covering `curl`, `gcc` (`build-essential`), `x86_64-w64-mingw32-gcc` (`mingw-w64`), and `ca-certificates`.
  - `rust-toolchain.toml`: Pin an **exact** Rust release (e.g., `channel = "1.85.0"`, verified at bootstrap time), explicitly include `components = ["clippy", "rustfmt"]`, and targets `["x86_64-unknown-linux-gnu", "x86_64-pc-windows-gnu"]`.
  - `Cargo.toml` + committed `Cargo.lock`: Pinned Rust crate dependencies verified with `--locked`.
  - `scripts/setup-env.sh`: Idempotent bootstrap script supporting `--check` (audit) and `--install-system-deps` (install missing host packages), installing `rustup`/Rust strictly under `<repo>/.toolchain/rustup` and `<repo>/.toolchain/cargo`, and pre-fetching crates into `.toolchain/cargo`.
- **Cargo config**: `.cargo/config.toml` sets `[target.x86_64-pc-windows-gnu] linker = "x86_64-w64-mingw32-gcc"`.

## 2. Crate & Workspace Architecture (Cross-Platform Core + Thin OS Adapters)

- **Decision**: Three-crate Cargo workspace:
  1. `crates/langflip-core`: Pure Rust library (`serde`, `toml` only). Owns `PhysicalKey`, `Modifiers`, `KeyEvent`, `InputEvent`, `LayoutId` (opaque string), `LayoutPair`, `HotkeySpec`, `AppConfig`, `KeystrokeBuffer` (behind `BufferPolicy`), `ConversionEngine`, and OS adapter traits (`InputListener`, `InputInjector`, `LayoutController`, `FocusObserver`). Zero Windows-specific code or LANGID tables.
  2. `crates/langflip-platform-win`: Thin Windows 11 adapter (`#[cfg(target_os = "windows")]`) using Microsoft's official `windows` crate. Owns Windows-specific `KLID`/`HKL` resolution, `WH_KEYBOARD_LL`/`WH_MOUSE_LL` hooks, `SendInput` injection, and foreground window tracking.
  3. `crates/langflip-app`: Thin binary entry point (`langflip.exe` on Windows; compile-checked stub on Linux) wiring config, startup layout validation, worker loop, and graceful shutdown.

## 3. Safe Conversion Ordering & Partial-Injection Recovery

- **Decision**: Execute conversion strictly in **Switch → Confirm → Validate Snapshot → Erase → Replay** order:
  1. **Validate layout**: Read current foreground `HKL`; if it is not in the configured `LayoutPair`, abort without erasing and clear the buffer.
  2. **Switch & confirm BEFORE erasure**: Post `WM_INPUTLANGCHANGEREQUEST` to the foreground window with the target `HKL`, then poll `GetKeyboardLayout(foreground_tid)` (e.g., every 2 ms up to 40 ms). If the active layout does NOT change to the target `HKL`, **abort immediately** — zero Backspaces are sent and user text is untouched.
  3. **Validate trigger snapshot**: Confirm the foreground `HWND` and input sequence counter have not changed since the hotkey was pressed. If either changed (user kept typing or switched windows), abort before erasing and clear the buffer.
  4. **Erase & replay with return-count verification**: Check the return value of every `SendInput` call (`sent == expected`). If fewer events were inserted (e.g., UIPI block or OS error), abort remaining steps immediately, clear the buffer (do NOT retain for round-trip), and log a non-sensitive diagnostic warning.
  5. **Retain buffer ONLY on full success**: Only after erasure and replay both succeed completely is the buffer retained (tagged with the new layout) for round-trip double-tap.
- **Early Windows PoC**: Validate `WM_INPUTLANGCHANGEREQUEST` confirmation + `SendInput` scan-code replay early on Windows 11 across Notepad, Chrome/Edge, and Windows Terminal/PowerShell (Phase 5 PoC gate).

## 4. Bounded Event Queue, Non-Blocking Hooks & Idle Shutdown Wake-Up

- **Decision**:
  - Use a **bounded** `std::sync::mpsc::SyncSender<InputEvent>` (`sync_channel(256)`) between the hook thread and the worker thread, plus an `Arc<AtomicBool>` `queue_overflowed` flag.
  - Inside `WH_KEYBOARD_LL` / `WH_MOUSE_LL`:
    - Check `dwExtraInfo == LANGFLIP_INJECTED_MARKER` (`0x4C46_4C50`); if matched, pass through immediately.
    - Call non-blocking `tx.try_send(event)`. If `TrySendError::Full`, set `queue_overflowed = true` and return immediately (< 1 ms) so the hook never blocks and memory stays strictly bounded. When the worker sees `queue_overflowed`, it clears the buffer so conversion never runs on an incomplete history.
  - **Idle Shutdown Wake-Up**: Store the hook thread's Win32 thread ID (`GetCurrentThreadId()`). The `InputListener` returns/exposes a `ShutdownHandle` whose `stop()` sets the atomic flag **and** calls `PostThreadMessageW(hook_tid, WM_QUIT, WPARAM(0), LPARAM(0))`, immediately waking an idle `GetMessageW` loop so hooks are unhooked and the process exits cleanly within milliseconds.

## 5. Windows 11 Layout Resolution (Exact KLID First, BCP-47 Fallback)

- **Decision**:
  - Keep `LayoutId` in `langflip-core` as an opaque normalized identifier (e.g., `"00000409"`, `"0000040D"`, `"en-US"`, `"he-IL"`).
  - Put Windows-specific layout resolution inside `crates/langflip-platform-win/src/layout.rs`:
    1. Enumerate installed layouts via `GetKeyboardLayoutList` and query each layout's KLID / language identifier.
    2. Match exact 8-hex-digit KLIDs first (e.g., `"00000409"` = US QWERTY, `"00010409"` = US Dvorak, `"0000040D"` = Hebrew Standard, `"0002040D"` = Hebrew Standard 2018) so layouts sharing a language are never collapsed.
    3. If the user configured a BCP-47 tag (e.g., `"en-US"`, `"he-IL"`), match by LANGID against installed layouts; if multiple installed layouts share that LANGID, pick the first deterministically and emit a startup warning advising the user to pin the exact KLID in `langflip.toml`.

## 6. Hotkey Auto-Repeat, Caps Lock & Combo Modifier Hygiene

- **Decision**:
  - **Auto-repeat suppression**: Track `hotkey_held: bool`. When the trigger hotkey is pressed while `hotkey_held == true`, swallow the repeat event and do NOT trigger conversion again until `KeyUp` clears `hotkey_held`.
  - **Caps Lock**: Record Caps Lock state at burst start; if `CapsLock` is pressed mid-burst, treat it as a `ResetBoundary` (clears buffer) so replay never has mixed Caps Lock state.
  - **Combo hotkeys (e.g., `Ctrl+Shift+L`)**: Before injecting Backspaces/replay, synthesize `KEYEVENTF_KEYUP` for any currently held modifier keys (`Ctrl`, `Alt`, `Shift`, `Meta`, tagged with `LANGFLIP_INJECTED_MARKER`) so injected Backspaces aren't interpreted as `Ctrl+Backspace` or `Shift+Backspace`. Wrap each shifted replayed key with synthetic `LShift` down/up. Never leave virtual modifiers stuck down on exit or error.
  - **Replay pacing**: Default `replay_delay_ms = 0` (batch `SendInput` calls where delay is 0, keeping 50-char conversion well under the 150 ms target). When `replay_delay_ms > 0` (configurable up to `10` ms for slow remote terminals), sleep once **per keystroke** (not per half down/up event).

## 7. Dependency Footprint

- `langflip-core`: `serde`, `toml`.
- `langflip-platform-win`: `windows` (minimal Win32 feature flags: `Win32_Foundation`, `Win32_UI_WindowsAndMessaging`, `Win32_UI_Input_KeyboardAndMouse`, `Win32_System_Threading`).
- `langflip-app`: `langflip-core`, `langflip-platform-win` (on Windows), `ctrlc`.
