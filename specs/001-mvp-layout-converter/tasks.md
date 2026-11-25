---
description: "TDD-ordered task breakdown for LangFlip MVP (001-mvp-layout-converter, Revised after Design Review)"
---

# Tasks: LangFlip MVP — In-Place Keyboard Layout Converter

**Input**: Design documents from `/specs/001-mvp-layout-converter/`

**Prerequisites**: [plan.md](file:///home/omri/projects/langflip/specs/001-mvp-layout-converter/plan.md), [spec.md](file:///home/omri/projects/langflip/specs/001-mvp-layout-converter/spec.md), [research.md](file:///home/omri/projects/langflip/specs/001-mvp-layout-converter/research.md), [data-model.md](file:///home/omri/projects/langflip/specs/001-mvp-layout-converter/data-model.md), [platform-traits.md](file:///home/omri/projects/langflip/specs/001-mvp-layout-converter/contracts/platform-traits.md)

**Strict TDD Policy (Constitution Principle IV — NON-NEGOTIABLE)**:
Every core module (`layout`, `config`, `buffer`, `engine`) MUST begin with failing unit/integration tests verified to fail for a known reason *before* writing production code, then evolve the implementation until tests turn green, then refactor (DRY) while keeping tests green.

---

## Phase 1: User Story 1 — Isolated Rust Dev Environment & Workspace Setup (Priority: P1) 🎯 Foundation

**Goal**: Declaratively configure all host & Rust dependencies so a fresh clone can audit/install every prerequisite, bootstrap an exact pinned Rust toolchain in `.toolchain/` (zero global mutation), run native Linux `cargo test` + `clippy`, and cross-compile `x86_64-pc-windows-gnu` with a committed `Cargo.lock`.

**Independent Test**: Run `./scripts/setup-env.sh --check` and `./scripts/setup-env.sh`, verify `~/.rustup` and `~/.cargo` are untouched, and verify `./scripts/cargo.sh test --workspace --locked` executes cleanly on Linux.

- [ ] T001 [P] [US1] Create declarative host dependency manifest in `toolchain/deps.conf` listing all required host tools (`curl`, `gcc`/`build-essential`, `x86_64-w64-mingw32-gcc`/`mingw-w64`, `ca-certificates`) mapped across `apt`, `dnf`, and `pacman`
- [ ] T002 [P] [US1] Create exact pinned Rust toolchain config in `rust-toolchain.toml` (exact release e.g. `1.85.0`, `components = ["clippy", "rustfmt"]`, targets `["x86_64-unknown-linux-gnu", "x86_64-pc-windows-gnu"]`) and Windows GNU linker config in `.cargo/config.toml`
- [ ] T003 [US1] Create idempotent bootstrap script `scripts/setup-env.sh` supporting `--check` and `--install-system-deps`, installing `rustup`/Rust strictly into `.toolchain/rustup` and `.toolchain/cargo`, and pre-fetching Cargo crates
- [ ] T004 [P] [US1] Create isolated Cargo wrapper `scripts/cargo.sh` that exports `RUSTUP_HOME`, `CARGO_HOME`, and `.toolchain/cargo/bin` on `PATH`
- [ ] T005 [US1] Scaffold 3-crate Cargo workspace (`Cargo.toml`, `crates/langflip-core/`, `crates/langflip-platform-win/`, `crates/langflip-app/`) and generate/commit `Cargo.lock`
- [ ] T006 [US1] Run `./scripts/setup-env.sh` and verify `~/.rustup` / `~/.cargo` are untouched and `./scripts/cargo.sh test --workspace --locked` compiles and runs on Linux

**Checkpoint**: Isolated, pinned Rust toolchain and empty 3-crate workspace verified on Linux with committed `Cargo.lock`.

---

## Phase 2: Foundational Core Value Types (US4 — Cross-Platform Core, Compile-Order Safe)

**Goal**: Define all shared value types (`key.rs`, `layout.rs` types, `config.rs` types) *before* defining `ports.rs`, eliminating compile-order gaps and keeping `langflip-core` 100% OS-independent.

**Independent Test**: `./scripts/cargo.sh check -p langflip-core` compiles cleanly on Linux with zero OS-specific dependencies.

- [ ] T007 [P] [US4] Define normalized input & focus types (`PhysicalKey`, `Modifiers`, `KeyKind`, `KeyEvent`, `FocusId`, `ControlKind`, `FocusContext`, `InputEvent`, `BufferedKeystroke`, `ConversionTicket`) in `crates/langflip-core/src/key.rs`
- [ ] T008 [P] [US3] Write failing unit tests for opaque `LayoutId` normalization and `LayoutPair` (`opposite_if_member`, `target_for_empty_toggle`, 3rd-layout `None` safety) in `crates/langflip-core/src/layout.rs` (verify failure)
- [ ] T009 [P] [US3] Write failing unit tests for `HotkeySpec` and `AppConfig` defaults (`00000409` ↔ `0000040D`, `Pause`, `max_buffer_len = 128` clamped/validated `1..=512`, `replay_delay_ms = 0` validated `0..=10`, `toggle_on_empty_buffer = true`), partial/full TOML overrides, custom combo hotkeys, and invalid config errors in `crates/langflip-core/src/config.rs` (verify failure)
- [ ] T010 [P] [US3] Implement `LayoutId` and `LayoutPair` in `crates/langflip-core/src/layout.rs` until T008 tests pass
- [ ] T011 [P] [US3] Implement `HotkeySpec`, `AppConfig`, and TOML loader/validator in `crates/langflip-core/src/config.rs` until T009 tests pass
- [ ] T012 [US4] Define OS adapter & extension traits (`InputListener` with bounded `SyncSender` + `ListenerShutdown`, `InputInjector` with `InjectError::PartialInjection`, `LayoutController` with `switch_and_confirm` + `validate_pair`, `FocusObserver`) in `crates/langflip-core/src/ports.rs`, wire `crates/langflip-core/src/lib.rs`, and verify `./scripts/cargo.sh test -p langflip-core`
- [ ] T013 [P] [US3] Add documented example configuration file `langflip.example.toml` at repository root

**Checkpoint**: All core value types, layout/config logic, and OS-isolation trait contracts are green and tested on Linux.

---

## Phase 3: User Story 2 — Core Conversion Engine & Keystroke Buffer (Priority: P1) 🎯 MVP Core

**Goal**: Implement the platform-independent `KeystrokeBuffer` (behind a `BufferPolicy` seam) and `ConversionEngine` orchestrator with safe **Switch → Confirm → Validate Snapshot → Erase → Replay** ordering, race cancellation, bounded queue overflow handling, auto-repeat suppression, and round-trip reversibility.

**Independent Test**: `./scripts/cargo.sh test -p langflip-core` passes all buffer unit tests and end-to-end `ConversionEngine` integration tests using in-memory mock `InputInjector`, `LayoutController`, and `FocusObserver`.

### Tests First (Red) ⚠️
- [ ] T014 [P] [US2] Write failing unit tests for `KeystrokeBuffer` & `NaiveBurstPolicy` in `crates/langflip-core/src/buffer.rs`: push printable, backspace pop (including empty pop), bounded overflow dropping oldest entry, and clear on reset (verify failure)
- [ ] T015 [US2] Write failing integration tests for `ConversionEngine` in `crates/langflip-core/tests/engine_tests.rs` using mocked `InputInjector`, `LayoutController`, and `FocusObserver` covering every SC-002 requirement:
  - EN→HE & HE→EN conversion sequence (verifying **Switch → Confirm** happens strictly *before* any Backspaces)
  - Shift & punctuation preservation, Backspace editing
  - **Every** FR-005 reset boundary: `Enter`, `Tab`, `Escape`, `Left`, `Right`, `Up`, `Down`, `Home`, `End`, `PageUp`, `PageDown`, `Delete`, `CapsLock`, `Ctrl`/`Alt`/`Meta` shortcuts (incl. AltGr), mouse click, focus change, external layout change
  - Rejected/unconfirmed layout switch → zero Backspaces emitted, buffer cleared
  - Partial injection failure (`InjectError::PartialInjection`) → abort remaining steps, buffer cleared (not retained)
  - Rapid post-hotkey typing or focus change before erasure → conversion cancelled safely
  - Queue overflow event → burst invalidated
  - Auto-repeat while hotkey held → triggers once; press-release-press → clean round-trip
  - 3rd unrelated layout → non-empty burst skips destructive erasure; empty burst toggles to primary
  - Empty-buffer toggle (`toggle_on_empty_buffer = true` and `false`) and self-injected event suppression
  (verify failure)

### Implementation (Green → Refactor)
- [ ] T016 [US2] Implement `BufferPolicy` trait, `NaiveBurstPolicy`, and bounded RAM-only `KeystrokeBuffer` in `crates/langflip-core/src/buffer.rs` until T014 tests pass
- [ ] T017 [US2] Implement `ConversionAction` and `ConversionEngine` (including safe `Switch → Confirm → Freshness Check → Release Modifiers → Erase → Replay` executor) in `crates/langflip-core/src/engine.rs` until T015 tests pass
- [ ] T018 [US2] DRY refactor pass across `crates/langflip-core/src/` (Constitution Principle VI) and verify `./scripts/cargo.sh test -p langflip-core` stays green

**Checkpoint**: Full cross-platform conversion engine is green and 100% verified natively on Linux.

---

## Phase 4: User Story 2 & 3 — Thin Windows 11 Adapter & App Binary (Priority: P1) 🎯 MVP Deliverable

**Goal**: Implement the thin Windows 11 Win32 adapter (`langflip-platform-win`) and binary entry point (`langflip-app`), cross-compile `langflip.exe` from Linux, and validate early on Windows 11.

**Independent Test**: `./scripts/cargo.sh build --release --locked --target x86_64-pc-windows-gnu -p langflip-app` produces `target/x86_64-pc-windows-gnu/release/langflip.exe`, and all Windows 11 verification checks in `quickstart.md` pass.

- [ ] T019 [P] [US2] Implement Windows `InputInjector` (`SendInput` with `KEYEVENTF_SCANCODE`, `KEYEVENTF_EXTENDEDKEY`, `dwExtraInfo = 0x4C46_4C50`, held-modifier `KEYUP` hygiene, batched 0 ms default or per-keystroke `replay_delay_ms`, and `sent != expected` `PartialInjection` detection) in `crates/langflip-platform-win/src/injector.rs`
- [ ] T020 [P] [US2] Implement Windows `LayoutController` (`GetForegroundWindow`, `GetWindowThreadProcessId`, `GetKeyboardLayout`, `GetKeyboardLayoutList`, exact KLID-first + BCP-47 fallback resolution with ambiguity/missing warnings, `PostMessageW(WM_INPUTLANGCHANGEREQUEST)` + confirmation poll up to timeout) in `crates/langflip-platform-win/src/layout.rs`
- [ ] T021 [P] [US2] Implement foreground `HWND` `FocusObserver` stub (Phase 2 UIA seam) in `crates/langflip-platform-win/src/focus.rs`
- [ ] T022 [US2] Implement non-blocking `WH_KEYBOARD_LL` + `WH_MOUSE_LL` `InputListener` (bounded `SyncSender::try_send` + `overflow_flag`, self-injection marker filter, `hotkey_held` auto-repeat suppression, `PostThreadMessageW(WM_QUIT)` idle shutdown wake-up, clean `UnhookWindowsHookEx` teardown) in `crates/langflip-platform-win/src/hook.rs`
- [ ] T023 [US2] Wire public exports in `crates/langflip-platform-win/src/lib.rs` (gated cleanly for Windows target)
- [ ] T024 [US3] Implement `crates/langflip-app/src/main.rs`: load optional `langflip.toml`, run `validate_pair` startup warnings, wire bounded channel + `ConversionEngine` worker loop + optional `--metrics` timing output, and wire `Ctrl+C` to `ListenerShutdown::stop()`
- [ ] T025 [US1] Cross-compile `langflip.exe` via `./scripts/cargo.sh build --release --locked --target x86_64-pc-windows-gnu -p langflip-app` and verify zero compiler warnings

**Checkpoint**: Standalone `langflip.exe` built from Linux using the isolated toolchain and committed `Cargo.lock`.

---

## Phase 5: Polish, Verification & Windows 11 Smoke/Timing Validation

- [ ] T026 [P] Run `./scripts/cargo.sh test --workspace --locked` and `./scripts/cargo.sh clippy --workspace --all-targets --locked -- -D warnings`
- [ ] T027 [P] Add concise root `README.md` linking to `quickstart.md`, `langflip.example.toml`, and the Phase 2–4 growth roadmap
- [ ] T028 Run full `quickstart.md` Windows 11 verification checklist (Notepad, Chrome/Edge, Windows Terminal/PowerShell, round-trip double-tap vs. hold, reset boundaries, custom hotkey `Ctrl+Shift+L`, missing/ambiguous layout warning, elevated PowerShell non-destructive abort, `< 150 ms` conversion & `< 1 ms` hook callback timing, and idle `Ctrl+C` shutdown within 500 ms)

---

## Dependencies & Execution Order

Strict sequential phase order (zero compile-order gaps):
1. **Phase 1 (US1)**: Isolated toolchain, declarative deps, 3-crate workspace, committed `Cargo.lock`.
2. **Phase 2 (US4 + US3)**: Core value types (`key.rs`), strict TDD for `layout.rs` & `config.rs`, then `ports.rs` contracts.
3. **Phase 3 (US2 Core)**: Depends on Phase 2 — strict TDD for `buffer.rs` and `engine.rs` + DRY refactor.
4. **Phase 4 (Windows Adapter + App)**: Depends on Phase 3 — `injector.rs`, `layout.rs`, and `focus.rs` run in parallel (`[P]`), followed by `hook.rs`, `main.rs`, and locked release cross-compile.
5. **Phase 5 (Polish & Verification)**: Depends on Phase 4.
