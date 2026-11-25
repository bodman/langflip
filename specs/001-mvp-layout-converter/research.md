# Research & Technical Decisions: LangFlip MVP

**Feature**: `001-mvp-layout-converter` | **Date**: 2026-11-25 (Revision 3 after Multi-Agent Design Review)

## 1. Declarative Dependencies, Exact Pinned Isolated Rust Toolchain & Cross-Compilation

- **Decision**: Declare **all** project dependencies in version-controlled configuration so any fresh clone is self-describing and reproducible:
  - `toolchain/deps.conf`: Declarative manifest of required host binaries and package names across common Linux package managers (`apt`, `dnf`, `pacman`) — covering `curl`, `gcc` (`build-essential`), `x86_64-w64-mingw32-gcc` (`mingw-w64`), `ca-certificates` — plus pinned `rustup-init` URL/version and SHA-256 checksum.
  - `rust-toolchain.toml`: Pin exact Rust release **`1.100.0`** (latest stable as of Nov 2026), explicitly including `components = ["clippy", "rustfmt"]` and `targets = ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-gnu"]`.
  - `Cargo.toml` + committed `Cargo.lock`: Verified with `--locked` once crate dependencies are declared.
  - `scripts/setup-env.sh`: Idempotent bootstrap script supporting `--check` and `--install-system-deps`, downloading `rustup-init`, verifying its SHA-256 checksum, running `rustup-init -y --no-modify-path` strictly with `RUSTUP_HOME="$REPO_ROOT/.toolchain/rustup"` and `CARGO_HOME="$REPO_ROOT/.toolchain/cargo"` (zero mutation of `~/.rustup`, `~/.cargo`, `~/.profile`, or `~/.bashrc`), and pre-fetching crates.
- **Cargo config**: `.cargo/config.toml` sets `[target.x86_64-pc-windows-gnu] linker = "x86_64-w64-mingw32-gcc"`.
- **Cross-target Clippy**: `windows` crate is declared under `[target.'cfg(windows)'.dependencies]` in `langflip-platform-win`, and verification runs Clippy for **both** Linux host and `--target x86_64-pc-windows-gnu` so the Windows adapter is statically checked on Linux.

## 2. Crate & Workspace Architecture (Pure Core Logic + Thin OS Adapters)

- **Decision**: Three-crate Cargo workspace where **all** pure decision/planning logic lives in `langflip-core` (strictly TDD-tested on Linux):
  1. `crates/langflip-core` (`serde`, `toml` only; zero OS crates):
     - `key.rs`: `PhysicalKey`, `Modifiers` (with **left/right** modifier bits + `caps_lock` + `num_lock`), `InjectedBy`, `KeyKind`, `classify_key()`, `KeyName` → `PhysicalKey` scan-code table, `KeyEvent`, `InputEvent`, `BufferedKeystroke` (with redacted `Debug`).
     - `hotkey.rs`: `HotkeySpec` + `HotkeyMatcher` (single source of truth for scan-code hotkey matching and `hotkey_held` auto-repeat suppression — NOT duplicated in `hook.rs`).
     - `layout.rs`: Opaque `LayoutId`, `InstalledLayout`, `LayoutPair`, and pure `resolve_layouts(installed, configured)` (exact ID match first, language-tag fallback with ambiguity warnings).
     - `config.rs`: `AppConfig`, config file discovery order (1. executable dir, 2. OS config dir, 3. defaults), strict fail-fast TOML parsing & range validation.
     - `ports.rs`: `FocusId`, `ControlKind`, `FocusContext`, `InjectionStep`, `InjectError`, `LayoutSwitchOutcome`, `InputListener`, `ListenerShutdown`, `InputInjector`, `LayoutController`, `FocusObserver`.
     - `buffer.rs`: `BufferPolicy`, `NaiveBurstPolicy`, bounded RAM-only `KeystrokeBuffer` (redacted `Debug`).
     - `planner.rs`: Pure `plan_injection(held_modifiers, erase_count, strokes) -> Vec<InjectionStep>` (mask key, exact left/right modifier `KeyUp`, Backspaces, Shift-wrapped scan-code down/up).
     - `engine.rs`: `ConversionEngine` orchestrating `Check Layout -> Check Freshness -> Switch & Confirm -> Re-check Freshness -> Plan & Execute Injection -> Retain Buffer on Full Success`.
  2. `crates/langflip-platform-win` (`[target.'cfg(windows)'.dependencies] windows`):
     - Thin Win32 translator calling `langflip-core`'s pure functions.
  3. `crates/langflip-app`:
     - Thin binary wiring config, startup layout validation, bounded channel, worker loop, and `Ctrl+C` shutdown.

## 3. Early Windows 11 Proof-of-Concept Spike (Scheduled Right After Phase 1)

- **Decision**: Before building the full Windows adapter, run a small Windows 11 PoC spike (`T006b`) verifying the 3 riskiest Win32 assumptions in Notepad, Chrome/Edge, and Windows Terminal/PowerShell:
  1. **Foreground thread & layout switch**: Using `GetGUIThreadInfo` on the foreground window's thread (not just `GetForegroundWindow` directly, which can miss UWP/console child threads) to query `GetKeyboardLayout`, post `WM_INPUTLANGCHANGEREQUEST`, and confirm the layout actually changed.
  2. **HKL → KLID resolution**: Enumerating `GetKeyboardLayoutList` and resolving each `HKL`'s KLID via Windows registry lookup (`HKLM\SYSTEM\CurrentControlSet\Control\Keyboard Layouts`) + `Win32_System_Registry` & `Win32_Globalization` crate features. Verified Microsoft KLIDs:
     - `00000409` — US (QWERTY)
     - `0000040D` — Hebrew (`KBDHEB.DLL`)
     - `0002040D` — Hebrew (Standard) (`KBDHEBS.DLL`)
     - `0003040D` — Hebrew (Standard, 2018) (`KBDHEBSI.DLL`)
     Default config uses `"en-US"` ↔ `"he-IL"` so Windows 11 users on any single Hebrew variant work out of the box (with a clear startup warning to pin an exact KLID if 2+ variants are installed).
  3. **`SendInput` + mask key**: Verifying `SendInput` scan-code replay, `KEYEVENTF_EXTENDEDKEY`, `Pause` vs `NumLock` (`0x45`) disambiguation, and mask key (`VK_E8`) preventing Alt-menu / Ctrl+Shift OS-switch misfires.

## 4. Safe Conversion Ordering, `latest_seq` Semantics & UIPI Reality

- **Mutating sequence (`latest_seq`)**:
  - Advanced **only** by mutating non-injected user events (`KeyDown` of non-hotkey keys, `MouseClick`, `FocusChanged`, `QueueOverflow`).
  - Releasing the trigger hotkey or its combo modifiers (`KeyUp`) does **NOT** advance `latest_seq`, preventing false cancellations when the user lets go of `Pause` or `Ctrl+Shift+L` during layout-switch confirmation.
  - Checked **before** layout switch AND **again** before erasure. If cancelled after layout switch, best-effort restore the prior layout.
  - Known inherent limitation: a key typed during the ~1 ms micro-window *after* erasure has started can still interleave with synthetic Backspaces.
- **UIPI / Elevated Windows (Accurate Behavior)**:
  - On Windows 11, a non-elevated `WH_KEYBOARD_LL` hook does **not** receive keystrokes going to an elevated window at all; keys (including `Pause`) simply reach the elevated window untouched.
  - Furthermore, `SendInput` blocked by UIPI does not reliably report a short write or `GetLastError`. Therefore `PartialInjection` (`sent < expected`) catches genuine OS-reported short writes, while elevated-window safety comes from non-elevated hooks never intercepting elevated windows in the first place.

## 5. Bounded Event Queue, Focus Producer & Idle Shutdown Wake-Up

- **Bounded queue**: `std::sync::mpsc::sync_channel::<InputEvent>(256)` with non-blocking `try_send` inside hook callbacks. On `TrySendError::Full`, set `overflow_flag` + advance `latest_seq` and return immediately (< 1 ms); worker clears the burst on overflow.
- **Active layout on every key event**: Hook callback reads `GetKeyboardLayout(gui_tid)` (cheap in-process read) and attaches it to `KeyEvent.active_layout`, eliminating worker-side layout query races and enabling synchronous external-layout-change detection.
- **Focus change producer**: Hook thread also registers `SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, ..., WINEVENT_OUTOFCONTEXT)` on the same `GetMessageW` message pump, emitting `InputEvent::FocusChanged` reliably on every window switch.
- **External synthetic input**: Hook distinguishes `dwExtraInfo == LANGFLIP_INJECTED_MARKER` (`InjectedBy::SelfMarker` → ignore) from `LLKHF_INJECTED` without our marker (`InjectedBy::External` → reset burst).
- **Idle shutdown wake-up**: `ListenerShutdown::stop()` sets the atomic stop flag AND calls `PostThreadMessageW(hook_tid, WM_QUIT, WPARAM(0), LPARAM(0))`, immediately waking an idle `GetMessageW` loop to unhook `WH_KEYBOARD_LL`, `WH_MOUSE_LL`, and `HWINEVENTHOOK`.

## 6. Dependency Footprint

- `langflip-core`: `serde`, `toml`.
- `langflip-platform-win`: `[target.'cfg(windows)'.dependencies] windows` with features: `Win32_Foundation`, `Win32_UI_WindowsAndMessaging`, `Win32_UI_Input_KeyboardAndMouse`, `Win32_System_Threading`, `Win32_System_Registry`, `Win32_Globalization`, `Win32_UI_Accessibility`.
- `langflip-app`: `langflip-core`, `langflip-platform-win` (on Windows), `ctrlc`.
