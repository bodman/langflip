---
description: "TDD-ordered task breakdown for LangFlip MVP (001-mvp-layout-converter, Revision 3)"
---

# Tasks: LangFlip MVP — In-Place Keyboard Layout Converter

**Input**: Design documents from `specs/001-mvp-layout-converter/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [platform-traits.md](contracts/platform-traits.md)

**Strict TDD Policy (Constitution Principle IV — NON-NEGOTIABLE)**:
Every pure core module (`key`, `hotkey`, `layout`, `config`, `buffer`, `planner`, `engine`) MUST begin with failing unit/integration tests verified to fail for a known reason *before* writing production code, then evolve the implementation until tests turn green, then refactor (DRY) while keeping tests green.

---

## Phase 1: User Story 1 — Isolated Rust Dev Environment & Workspace Setup (Priority: P1) 🎯 Foundation

**Goal**: Declaratively configure all host & Rust dependencies, bootstrap exact Rust `1.100.0` strictly in `.toolchain/` (zero global or shell-profile mutation), and scaffold the 3-crate Cargo workspace.

**Independent Test**: Run `./scripts/setup-env.sh --check` and `./scripts/setup-env.sh`, verify `~/.rustup`, `~/.cargo`, `~/.profile`, and `~/.bashrc` are untouched, and verify `./scripts/cargo.sh test --workspace` executes cleanly on Linux.

- [x] T001 [P] [US1] Create declarative host dependency manifest in `toolchain/deps.conf` listing all required host tools (`curl`, `gcc`/`build-essential`, `x86_64-w64-mingw32-gcc`/`mingw-w64`, `ca-certificates`) mapped across `apt`, `dnf`, `pacman`, plus pinned `rustup-init` URL and SHA-256 checksum
- [x] T002 [P] [US1] Create exact pinned Rust toolchain config in `rust-toolchain.toml` (`channel = "1.100.0"`, `components = ["clippy", "rustfmt"]`, `targets = ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-gnu"]`) and Windows GNU linker config in `.cargo/config.toml`
- [x] T003 [US1] Create idempotent bootstrap script `scripts/setup-env.sh` supporting `--check` and `--install-system-deps`, verifying `rustup-init` SHA-256, running `rustup-init -y --no-modify-path` strictly into `.toolchain/rustup` and `.toolchain/cargo`, and pre-fetching Cargo crates
- [x] T004 [P] [US1] Create isolated Cargo wrapper `scripts/cargo.sh` exporting `RUSTUP_HOME`, `CARGO_HOME`, and `.toolchain/cargo/bin` on `PATH`
- [x] T005 [US1] Scaffold 3-crate Cargo workspace (`Cargo.toml`, `crates/langflip-core/Cargo.toml` with `serde`+`toml`, `crates/langflip-platform-win/Cargo.toml` with `[target.'cfg(windows)'.dependencies] windows` incl. `Registry`+`Globalization`+`Accessibility`, `crates/langflip-app/Cargo.toml` with `ctrlc`) and generate/commit `Cargo.lock`
- [x] T006 [US1] Run `./scripts/setup-env.sh` and verify `~/.rustup`, `~/.cargo`, `~/.profile`, and `~/.bashrc` are untouched and `./scripts/cargo.sh test --workspace --locked` compiles and runs on Linux

**Checkpoint**: Isolated, pinned Rust `1.100.0` toolchain and 3-crate workspace verified on Linux with committed `Cargo.lock`.

---

## Phase 1.5: Early Windows 11 Proof-of-Concept Spike (De-risk Win32 Assumptions)

**Goal**: Validate the riskiest Windows 11 behaviors *before* building the full engine: `GetGUIThreadInfo` + `WM_INPUTLANGCHANGEREQUEST` confirmation, registry `HKL` → `KLID` lookup, `SendInput` scan-code replay + mask key, and `Pause`/`NumLock` (`0x45`) disambiguation.

**Independent Test**: Cross-compile `win11_poc.exe` and verify it switches/confirms EN↔HE layouts and replays `"hello world"` / `"שלום עולם"` in Notepad, Chrome/Edge, and Windows Terminal.

- [x] T007 [US2] Create minimal Windows 11 spike binary `crates/langflip-platform-win/examples/win11_poc.rs` testing `GetGUIThreadInfo`, registry `HKL` → `KLID` resolution, `WM_INPUTLANGCHANGEREQUEST` confirmation poll, and `SendInput` scan-code + mask-key injection; cross-compile via `./scripts/cargo.sh build --release --locked --target x86_64-pc-windows-gnu -p langflip-platform-win --example win11_poc`
- [x] T008 [US2] Run `win11_poc.exe` on Windows 11 in Notepad, Chrome/Edge, and Windows Terminal; record confirmed behaviors or quirks in `specs/001-mvp-layout-converter/research.md`

**Checkpoint**: Riskiest Windows 11 APIs empirically verified before full implementation.

---

## Phase 2: Foundational Core Value Types, Hotkey, Layout, Config & Ports (US4 + US3, Strict TDD)

**Goal**: Build and TDD-verify all OS-independent value types, `classify_key`, `HotkeyMatcher`, `resolve_layouts`, `AppConfig`, and `ports.rs` contracts in strict compile order.

**Independent Test**: `./scripts/cargo.sh test -p langflip-core --locked` passes all unit tests natively on Linux.

- [ ] T009 [US4] Write failing unit tests for `PhysicalKey`, left/right `Modifiers`, `KeyName` → `PhysicalKey` scan-code lookup, `classify_key` (incl. `Insert`, `CapsLock`, numpad with `num_lock` on vs. off), and redacted `Debug` on `BufferedKeystroke` in `crates/langflip-core/src/key.rs` (verify failure)
- [ ] T010 [US4] Implement `crates/langflip-core/src/key.rs` until T009 tests pass
- [ ] T011 [US3] Write failing unit tests for `HotkeySpec` and `HotkeyMatcher` (single-key `Pause`, combo `Ctrl+Shift+L` with left/right modifiers, `hotkey_held` auto-repeat suppression, `KeyUp` re-arming) in `crates/langflip-core/src/hotkey.rs` (verify failure)
- [ ] T012 [US3] Implement `crates/langflip-core/src/hotkey.rs` until T011 tests pass
- [ ] T013 [US3] Write failing unit tests for opaque `LayoutId`, `LayoutPair` (`opposite_if_member`, `target_for_empty_toggle`, 3rd-layout `None` safety), and `resolve_layouts(installed, configured)` (exact KLID match, BCP-47 fallback, ambiguity warning, missing layout warning) in `crates/langflip-core/src/layout.rs` (verify failure)
- [ ] T014 [US3] Implement `crates/langflip-core/src/layout.rs` until T013 tests pass
- [ ] T015 [US3] Write failing unit tests for `AppConfig` defaults (`en-US` ↔ `he-IL`, `Pause`, `max_buffer_len = 128`, `replay_delay_ms = 0`, `toggle_on_empty_buffer = true`), lookup precedence (exe dir → OS config dir → defaults), fail-fast rejection of invalid syntax / out-of-range values (`max_buffer_len` outside `1..=512`, `replay_delay_ms` outside `0..=10`), and parsing `langflip.example.toml` in `crates/langflip-core/src/config.rs` (verify failure)
- [ ] T016 [US3] Create `langflip.example.toml` and implement `crates/langflip-core/src/config.rs` until T015 tests pass
- [ ] T017 [US4] Define OS adapter & extension traits (`FocusId`, `ControlKind`, `FocusContext`, `FocusObserver`, `InjectionStep`, `InjectError`, `InputInjector`, `LayoutSwitchOutcome`, `LayoutController`, `ListenerShutdown`, `InputListener`) in `crates/langflip-core/src/ports.rs`, wire `crates/langflip-core/src/lib.rs`, and verify `./scripts/cargo.sh test -p langflip-core --locked`

**Checkpoint**: All core value types, classifiers, hotkey matcher, layout resolver, config loader, and OS trait contracts are green on Linux.

---

## Phase 3: User Story 2 — Buffer, Replay Planner & Conversion Engine (Priority: P1, Strict TDD) 🎯 MVP Core

**Goal**: Build and TDD-verify `KeystrokeBuffer`, pure `plan_injection`, and `ConversionEngine` in focused Red → Green → Refactor increments.

**Independent Test**: `./scripts/cargo.sh test -p langflip-core --locked` passes all unit and mocked integration tests natively on Linux.

- [ ] T018 [US2] Write failing unit tests for `KeystrokeBuffer` & `NaiveBurstPolicy` (push, backspace incl. empty pop, tail-bounded overflow keeping newest `max_len`, clear, redacted `Debug` never exposing typed keys) in `crates/langflip-core/src/buffer.rs` (verify failure)
- [ ] T019 [US2] Implement `BufferPolicy`, `NaiveBurstPolicy`, and `KeystrokeBuffer` in `crates/langflip-core/src/buffer.rs` until T018 tests pass
- [ ] T020 [US2] Write failing unit tests for pure `plan_injection(held_modifiers, erase_count, strokes, replay_delay_ms)` verifying mask key insertion, exact left/right modifier release, Backspace count, Shift-wrapped scan-code taps, and per-keystroke delay (`0` ms omits delays) in `crates/langflip-core/src/planner.rs` (verify failure)
- [ ] T021 [US2] Implement `crates/langflip-core/src/planner.rs` until T020 tests pass
- [ ] T022 [US2] Write failing integration tests (Group A — core flows & all reset boundaries) in `crates/langflip-core/tests/engine_tests.rs`: EN→HE & HE→EN conversion (verifying Switch→Confirm before Erase), Shift/punctuation, Backspace, round-trip press-release-press vs. auto-repeat hold, empty-buffer toggle (`true`/`false`), and **every** FR-005 reset boundary (`Enter`, `Tab`, `Escape`, `Insert`, `Left`/`Right`/`Up`/`Down`, `Home`/`End`, `PageUp`/`PageDown`, `Delete`, numpad w/o NumLock, `CapsLock`, `Ctrl`/`Alt`/`Meta` shortcuts, externally injected input, mouse click, focus change, external layout change) (verify failure)
- [ ] T023 [US2] Write failing integration tests (Group B — safety, races & overflow) in `crates/langflip-core/tests/engine_tests.rs`: rejected/unconfirmed layout switch (zero Backspaces, buffer cleared), `PartialInjection` failure (abort & clear, not retained), rapid post-hotkey typing or focus change before erasure (cancel safely + best-effort layout restore if already switched), hotkey/modifier `KeyUp` NOT advancing `latest_seq`, `QueueOverflow` invalidating burst, buffer tail overflow converting retained tail, 3rd-layout non-destructive skip, and `SelfMarker` suppression (verify failure)
- [ ] T024 [US2] Implement `ConversionEngine` in `crates/langflip-core/src/engine.rs` until T022 and T023 tests pass
- [ ] T025 [US2] DRY refactor pass across `crates/langflip-core/src/` (Constitution Principle VI) and verify `./scripts/cargo.sh test -p langflip-core --locked` stays green

**Checkpoint**: Full cross-platform core engine is green and 100% verified natively on Linux.

---

## Phase 4: User Story 2 & 3 — Thin Windows 11 Adapter & App Binary (Priority: P1) 🎯 MVP Deliverable

**Goal**: Implement the thin Windows 11 Win32 adapter calling `langflip-core`'s pure functions, wire `langflip-app`, and cross-compile `langflip.exe` with `--locked`.

**Independent Test**: `./scripts/cargo.sh build --release --locked --target x86_64-pc-windows-gnu -p langflip-app` produces `target/x86_64-pc-windows-gnu/release/langflip.exe` with zero warnings.

- [ ] T026 [P] [US2] Implement thin Windows `InputInjector` translating `InjectionStep` to `SendInput` (`KEYEVENTF_SCANCODE`, `KEYEVENTF_EXTENDEDKEY`, `VK_E8` mask key, `dwExtraInfo = 0x4C46_4C50`, `sent < expected` → `PartialInjection`) in `crates/langflip-platform-win/src/injector.rs`
- [ ] T027 [P] [US2] Implement thin Windows `LayoutController` (`GetGUIThreadInfo`, `GetKeyboardLayoutList` + registry `KLID` lookup, `WM_INPUTLANGCHANGEREQUEST` + confirmation poll) in `crates/langflip-platform-win/src/layout.rs`
- [ ] T028 [P] [US2] Implement foreground `HWND` `FocusObserver` in `crates/langflip-platform-win/src/focus.rs`
- [ ] T029 [US2] Implement non-blocking `WH_KEYBOARD_LL` + `WH_MOUSE_LL` + `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` `InputListener` delegating hotkey/repeat decisions to `langflip-core::hotkey::HotkeyMatcher`, tagging each key event with `active_layout` + `InjectedBy`, advancing `latest_seq` only on mutating events, using bounded `SyncSender::try_send`, and waking idle `GetMessageW` via `PostThreadMessageW(WM_QUIT)` in `crates/langflip-platform-win/src/hook.rs`
- [ ] T030 [US2] Wire public exports in `crates/langflip-platform-win/src/lib.rs` (gated for Windows target)
- [ ] T031 [US3] Implement `crates/langflip-app/src/main.rs`: load config (exe dir → OS config dir → defaults, fail-fast on invalid), run `resolve_layouts` startup warnings, run bounded-channel `ConversionEngine` loop, support `--metrics` timing output (never logging keystrokes), and wire `Ctrl+C` to `ListenerShutdown::stop()`
- [ ] T032 [US1] Verify/update `Cargo.lock` if needed and cross-compile `langflip.exe` via `./scripts/cargo.sh build --release --locked --target x86_64-pc-windows-gnu -p langflip-app`

**Checkpoint**: Standalone `langflip.exe` built from Linux using the isolated pinned toolchain and committed `Cargo.lock`.

---

## Phase 5: Polish, Cross-Target Linting & Windows 11 Verification

- [ ] T033 [P] Run `./scripts/cargo.sh test --workspace --locked`, `./scripts/cargo.sh clippy --workspace --all-targets --locked -- -D warnings`, and `./scripts/cargo.sh clippy --target x86_64-pc-windows-gnu --workspace --locked -- -D warnings`
- [ ] T034 [P] Add concise root `README.md` linking to `specs/001-mvp-layout-converter/quickstart.md`, `langflip.example.toml`, and the Phase 2–4 growth roadmap
- [ ] T035 Execute the full `specs/001-mvp-layout-converter/quickstart.md` Windows 11 verification checklist (Notepad, Chrome/Edge, Windows Terminal/PowerShell, round-trip double-tap vs. hold, reset boundaries, custom combo hotkey `Ctrl+Shift+L`, missing/ambiguous layout warnings & fail-fast invalid config, elevated PowerShell non-interception, `< 150 ms` conversion & `< 1 ms` hook callback via `--metrics`, and idle `Ctrl+C` shutdown < 500 ms)

---

## Dependencies & Execution Order

Strict sequential phase order (zero compile-order gaps):
1. **Phase 1 (US1)**: Isolated pinned `1.100.0` toolchain, declarative deps, 3-crate workspace, committed `Cargo.lock`.
2. **Phase 1.5 (Spike)**: Early Windows 11 PoC validating `GetGUIThreadInfo`, registry `KLID`, `WM_INPUTLANGCHANGEREQUEST`, and `SendInput` + mask key.
3. **Phase 2 (US4 + US3)**: Strict TDD for `key.rs` → `hotkey.rs` → `layout.rs` → `config.rs` (+ `langflip.example.toml` parse test) → `ports.rs`.
4. **Phase 3 (US2 Core)**: Strict TDD for `buffer.rs` → `planner.rs` → `engine.rs` (split into Group A & Group B tests) → DRY refactor.
5. **Phase 4 (Windows Adapter + App)**: `injector.rs`, `layout.rs`, and `focus.rs` in parallel (`[P]`), then `hook.rs`, `main.rs`, and locked release cross-compile.
6. **Phase 5 (Polish & Verification)**: Host + Windows cross-target Clippy, `README.md`, and full Windows 11 checklist.
