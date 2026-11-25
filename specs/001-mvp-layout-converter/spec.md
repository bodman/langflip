# Feature Specification: LangFlip MVP — In-Place Keyboard Layout Converter

**Feature Branch**: `001-mvp-layout-converter`

**Created**: 2026-11-25

**Status**: Draft (Design Review Revision 2)

**Input**: User description: "A cross-platform-architected Rust utility (initially targeting Windows 11) that converts accidentally mistyped text between configurable keyboard languages (defaulting to English ↔ Hebrew) in-place across any application (browsers, native apps, command-line terminals) and switches the active keyboard layout on a configurable hotkey (default: Pause/Break). Starts with the naive low-dependency keystroke-buffer case while designing clear extension seams for larger focus-aware buffers, automatic OS layout discovery, and future OS targets. Includes setting up an isolated project-local Rust dev environment and Windows cross-compilation toolchain."

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Isolated Rust Dev Environment & Windows Cross-Build (Priority: P1)

As a developer on Linux targeting Windows 11, I want all host and toolchain dependencies declaratively configured in the repository and a repeatable setup that installs a pinned Rust toolchain and Windows cross-compilation target inside a project-local isolated directory (`.toolchain/`) without modifying global `~/.rustup` or `~/.cargo`, so a fresh clone can audit/install every prerequisite, run unit tests natively on Linux, and produce a standalone Windows 11 `.exe` cleanly.

**Why this priority**: Foundation for all subsequent development and testing; ensures zero pollution of the host machine's global environment.

**Independent Test**: Run the project's environment setup script on a Linux machine with no global Rust installation, verify `~/.rustup` and `~/.cargo` are not created/modified, run unit tests for the platform-independent core, and compile a locked release binary for Windows 11.

**Acceptance Scenarios**:

1. **Given** a fresh clone on a Linux host without a global Rust installation, **When** the developer runs the dependency audit and isolated setup script, **Then** every required host tool is checked/installable from project configuration, and the pinned Rust toolchain + Windows target are installed strictly under `<repo>/.toolchain/`.
2. **Given** the isolated toolchain is installed, **When** the developer runs the project's test and build wrapper commands, **Then** core unit tests pass on Linux and a standalone `langflip.exe` binary is produced for Windows 11 using the committed lockfile.
3. **Given** the developer deletes `<repo>/.toolchain/`, **When** inspecting the host home directory, **Then** no leftover Rust/Cargo state remains in `~/.rustup` or `~/.cargo`.

---

### User Story 2 - Instant Mistype Conversion & Layout Switch in Any App (Priority: P1)

As a bilingual Windows 11 user, when I start typing straight-line text in the wrong keyboard language inside any text input (browser, desktop app, or command-line terminal) and realize my mistake, I want to press a single hotkey (`Pause/Break` by default) so the utility safely switches my active keyboard layout to the other configured language, confirms the switch succeeded, erases the mistyped burst, replays the text in the new language, and lets me continue typing seamlessly.

**Why this priority**: This is the core MVP value proposition. Solving the naive unbroken-burst case safely (never deleting text unless the layout switch is confirmed and target focus/sequence is intact) immediately eliminates daily friction without risking data loss.

**Independent Test**: Launch `langflip.exe` on Windows 11, type a mistyped English phrase while in Hebrew layout (and vice versa) in Notepad, a browser text field, and Windows Terminal/PowerShell, press `Pause/Break`, and verify the text is replaced in-place, the Windows keyboard layout has toggled, and subsequent keystrokes appear in the new language.

**Acceptance Scenarios**:

1. **Given** Windows 11 is in Hebrew layout and the user intends to type `"hello world"` in a browser input, **When** the user types `"יקךךם 'םרךג"` and presses `Pause/Break`, **Then** the mistyped characters are replaced in-place with `"hello world"`, the active keyboard layout switches to English (US), and further typing produces English.
2. **Given** Windows 11 is in English layout and the user intends to type `"שלום עולם"` in Notepad or Windows Terminal, **When** the user types `"akuo guko"` and presses `Pause/Break`, **Then** the mistyped characters are replaced in-place with `"שלום עולם"`, the active keyboard layout switches to Hebrew, and further typing produces Hebrew.
3. **Given** the user typed `"hello"` with a typo, pressed `Backspace` once, and typed `"o"` before pressing `Pause/Break`, **When** conversion runs, **Then** the buffer accurately reflects the edited text (`"hello"`), not the deleted character.
4. **Given** the user typed a string containing Shift-modified keys or punctuation (e.g., `"Hello, World!"` or Hebrew punctuation on `/` and `'`), **When** conversion runs, **Then** Shift state and physical-key punctuation map accurately to the target layout.
5. **Given** the user pressed `Enter`, clicked the mouse, pressed an arrow key, changed keyboard layouts externally, or switched windows before typing a new burst, **When** the user presses `Pause/Break` after the new burst, **Then** ONLY the new burst after the reset boundary is erased and converted (prior text is never touched).
6. **Given** the buffer is empty (nothing typed since the last reset or conversion), **When** the user presses `Pause/Break`, **Then** no Backspaces are sent and the utility toggles the active keyboard layout per `FR-007` (when `toggle_on_empty_buffer` is enabled, the default).
7. **Given** the user triggers conversion and immediately types another key or changes window focus before erasure begins, **When** the conversion worker validates the trigger snapshot, **Then** conversion is cancelled cleanly without erasing text in the wrong position or wrong window.
8. **Given** the target application rejects or ignores the layout switch request (e.g., an elevated window), **When** the user presses `Pause/Break`, **Then** no Backspaces are sent, the user's existing text is preserved intact, and the buffer is cleared.

---

### User Story 3 - Configurable Hotkey, Language Pair & Safe Runtime Lifecycle (Priority: P2)

As a user who may use a different keyboard language pair (e.g., English ↔ Russian, English ↔ Arabic, or a specific layout variant such as US Dvorak) or whose keyboard lacks a `Pause/Break` key, I want to configure the language/layout pair, trigger hotkey, and basic behavior via a simple local config file, and start/stop the utility cleanly — even while idle — without leaving stuck hooks or modifiers.

**Why this priority**: Scan-code replay naturally supports any non-IME OS keyboard layout pair without hardcoding character tables, and compact keyboards often omit `Pause/Break`.

**Independent Test**: Edit the config file to bind an alternative hotkey (e.g., `Ctrl+Shift+L` or `F9`) and/or an alternative layout pair, restart `langflip.exe`, verify the configured hotkey and layout pair are used, and verify terminating the utility while completely idle cleanly unhooks input and exits immediately.

**Acceptance Scenarios**:

1. **Given** no config file exists on first launch, **When** `langflip.exe` starts, **Then** it runs with sensible defaults (`Pause/Break` hotkey, bounded buffer of 128 keystrokes, US QWERTY ↔ Israeli Hebrew Standard layout pair).
2. **Given** a valid user config file specifying a custom hotkey and/or a custom pair of installed OS keyboard layouts (by language tag or exact OS layout ID), **When** `langflip.exe` starts, **Then** the custom hotkey toggles and converts between those configured layouts while the previous default key behaves normally.
3. **Given** `langflip.exe` is running and completely idle (no user input occurring), **When** the user terminates it (e.g., `Ctrl+C`), **Then** the input listener wakes up immediately, all OS input hooks are cleanly removed, and the process exits cleanly.

---

### User Story 4 - Cross-Platform Core Architecture & Extension Seams (Priority: P2)

As the project maintainer, I want the core engine (buffering, reset policy, conversion orchestration, and configuration) to be strictly OS-independent and decoupled behind clean platform interfaces, so that:
1. All core behavior is thoroughly unit-tested on Linux without Windows.
2. Future phases (focus/entry-area-aware larger buffers, UIA caret reading, selection-based fallback, and macOS/Linux backends) can be added without rewriting the core.

**Why this priority**: Directly fulfills the cross-platform design goal and prevents the naive MVP from becoming throwaway code when scaling to larger buffers and additional platforms.

**Independent Test**: Compile and run the core library crate (`langflip-core`) on Linux with mocked platform interfaces exercising full conversion, reset, backspace, Shift/CapsLock, race-cancellation, overflow, and empty-buffer scenarios.

**Acceptance Scenarios**:

1. **Given** the `langflip-core` crate with zero OS-specific dependencies, **When** unit and integration tests run on Linux, **Then** all buffer, reset, replay-plan, race-safety, and config tests execute and pass natively.
2. **Given** a future developer wants to add a macOS or Linux backend or a focus-aware buffer, **When** inspecting the core interfaces, **Then** platform I/O (`InputListener`, `InputInjector`, `LayoutController`, `FocusObserver`) and buffer policy are cleanly separated via narrow traits.

---

### Edge Cases

- **Failed or rejected layout switch (including elevated/Administrator windows)**: If the OS or target app rejects or does not confirm the layout change within the confirmation window, LangFlip MUST abort before sending any Backspaces so no user text is ever lost.
- **Partial synthetic key injection**: If the OS reports fewer injected events than requested during erasure or replay, LangFlip MUST abort the remaining sequence, clear the buffer (not retaining it for round-trip), and surface a non-sensitive diagnostic warning.
- **Rapid typing or focus change during conversion**: If another non-injected keystroke or focus change arrives after the hotkey is pressed and before erasure begins, conversion MUST cancel cleanly so Backspaces never delete newly typed text or edit a different window.
- **Self-injected keystrokes (infinite loop prevention)**: Synthetic keystrokes emitted during erasure and replay MUST be identified and ignored by the input listener so they are never re-recorded or re-trigger the hotkey.
- **Auto-repeat on held hotkey**: Holding down the trigger hotkey MUST fire only once until the key is released (`KeyUp`). Pressing, releasing, and pressing again triggers a second conversion (clean round-trip back to the original language and text).
- **Caps Lock & combo modifiers (`Ctrl+Shift+L`)**: Toggling `CapsLock` mid-burst MUST reset the naive buffer so replay casing never desyncs. When a modifier-combo hotkey is used, held combo modifiers MUST be cleanly released/reconciled before replay without leaving virtual modifiers stuck down.
- **Dead keys, AltGr, IMEs, and autocomplete**: MVP is scoped to straight-line, non-IME, non-dead-key text entry. `Ctrl`/`Alt`/`Meta` combinations (including AltGr) and external layout switches invalidate the active burst rather than guessing character counts.
- **Bounded memory across both event queue and keystroke buffer**: Both the raw input event queue and the keystroke buffer MUST enforce hard upper bounds (configurable buffer cap bounded to `1..=512`). If the event queue overflows, the active burst MUST be invalidated so conversion never runs on an incomplete history.
- **Third/unrelated installed layout**: If the user is currently typing in a 3rd layout outside the configured pair, pressing the hotkey on a non-empty burst MUST NOT destructively erase text that cannot round-trip to its original layout (it clears the burst and skips conversion; on an empty buffer it switches to the primary configured layout).
- **Ambiguous or missing configured layouts**: If a configured language tag matches multiple installed layouts (e.g., US QWERTY and US Dvorak), the OS adapter MUST select deterministically and emit a startup warning recommending an exact OS layout ID. If a configured layout is not installed, LangFlip MUST warn clearly at startup and never corrupt input.

---

## Requirements *(mandatory)*

### Functional Requirements

#### Isolated Toolchain & Build
- **FR-001**: The repository MUST declare **all** build and development dependencies (both host tools already present and not yet installed, the exact pinned Rust release and components, target triples, and Cargo crates) in version-controlled project configuration (`toolchain/deps.conf`, `rust-toolchain.toml`, `Cargo.toml`, `Cargo.lock`) so a fresh clone knows every prerequisite required to build and test.
- **FR-002**: The repository MUST provide an idempotent bootstrap script (`scripts/setup-env.sh`) that reads the declared dependency configuration, audits and optionally installs missing host packages (`--check` and `--install-system-deps`), installs the pinned Rust toolchain and Windows target strictly inside `<repo>/.toolchain/` without touching `~/.rustup` or `~/.cargo`, and pre-fetches Cargo crates.
- **FR-003**: The repository MUST provide helper wrapper command(s) (`scripts/cargo.sh`) that execute native host tests, linting, and locked Windows cross-builds strictly using the isolated `.toolchain/` environment.

#### Core Conversion Engine (Cross-Platform)
- **FR-004**: The core engine MUST maintain a bounded, RAM-only buffer of the current unbroken burst of physical keystrokes (default max 128, hard-capped at `1..=512`), fed from a bounded raw-input event queue whose overflow invalidates the active burst.
- **FR-005**: The core engine MUST update the buffer on `Backspace` (removing the last buffered keystroke if non-empty) and MUST reset the buffer on naive context boundaries: `Enter`, `Tab`, `Escape`, navigation keys (`Left`, `Right`, `Up`, `Down`, `Home`, `End`, `PageUp`, `PageDown`, `Delete`), `CapsLock` toggle, `Ctrl`/`Alt`/`Meta` shortcuts, mouse clicks, window/focus changes, and external keyboard layout changes.
- **FR-006**: When the trigger hotkey is pressed (ignoring key auto-repeat while held) and the buffer is non-empty, the engine MUST bind the conversion to the active focus target and input sequence generation, and execute in safe order:
  1. Verify the active layout belongs to the configured layout pair (if it is an unrelated 3rd layout, clear the burst and abort without erasing).
  2. Request a keyboard layout switch to the opposite layout in the configured pair and **confirm the switch succeeded** before any erasure; if unconfirmed or rejected, abort cleanly with zero erasure and clear the buffer.
  3. Verify focus target and input sequence have not changed since trigger; if stale, abort cleanly and clear the buffer.
  4. Emit `N` Backspace actions matching the buffered visible keystroke count, verifying complete injection.
  5. Replay the buffered physical keystrokes under the new layout, verifying complete injection.
  6. Retain the converted buffer (marked with the new layout) **only after full success** so a subsequent press-release-press cleanly reverses the conversion. On any partial injection failure, abort remaining steps and clear the buffer.
- **FR-007**: When the trigger hotkey is pressed and the buffer is empty, the engine MUST NOT emit any Backspaces and MUST toggle the active keyboard layout to the opposite configured layout (or to the primary layout if currently on an unrelated 3rd layout) when `toggle_on_empty_buffer` is enabled (default: `true`).
- **FR-008**: The engine and OS adapter MUST suppress/ignore self-injected synthetic events so Backspaces and replayed keys are never re-captured or re-trigger the hotkey.
- **FR-009**: The core engine MUST reside in a platform-independent library crate with zero OS-specific dependencies, communicating with OS backends exclusively through narrow platform traits (`InputListener`, `InputInjector`, `LayoutController`, `FocusObserver`).

#### Windows 11 Adapter (v1 Target)
- **FR-010**: The Windows adapter MUST capture global keyboard and mouse-click reset events non-blockingly into the bounded event queue, suppress the trigger hotkey from reaching foreground apps, and provide an explicit wake-up path so graceful shutdown completes immediately even while completely idle.
- **FR-011**: The Windows adapter MUST inject Backspaces and replayed physical keystrokes (reconciling held combo modifiers cleanly without leaving virtual modifiers stuck) and report exact injection success/partial-failure back to the core engine.
- **FR-012**: The Windows adapter MUST detect the foreground target's active keyboard layout, resolve configured layout identifiers (exact OS layout IDs first, then language tags with ambiguity warnings), switch reliably between the configured pair, and confirm the layout change before erasure proceeds.
- **FR-013**: The Windows adapter MUST remove all OS input hooks cleanly upon graceful shutdown.

#### Configuration & Privacy
- **FR-014**: The utility MUST support an optional configuration file (`langflip.toml`) allowing customization of:
  - Configured keyboard layout pair (opaque layout IDs or BCP-47 tags; default: US QWERTY `"00000409"` / `"en-US"` ↔ Israeli Hebrew Standard `"0000040D"` / `"he-IL"`).
  - Trigger hotkey (default: `Pause/Break`).
  - Max buffer size (default: `128`, clamped/validated to `1..=512`).
  - Per-keystroke replay delay in milliseconds (default: `0` ms; configurable up to `10` ms for slower remote terminals).
  - Toggle-on-empty-buffer behavior (default: `true`).
- **FR-015**: Captured keystrokes MUST remain strictly in volatile memory and MUST NEVER be written to disk, sent over the network, or emitted to logs in release builds.

### Key Entities

- **KeyEvent**: Platform-normalized physical key press/release (`PhysicalKey`, `KeyKind`, `Modifiers`, `is_injected`).
- **KeystrokeBuffer**: Bounded RAM-only sequence of typed keystrokes for the active burst, behind a pluggable `BufferPolicy` seam.
- **LayoutId / LayoutPair**: Platform-neutral layout identifier (supporting exact OS layout IDs or BCP-47 tags) and the configured pair to toggle between; resolved to OS-native layout handles inside the active `LayoutController`.
- **ConversionTicket / ConversionPlan**: Trigger-time snapshot binding a conversion to `(focus_id, input_sequence)` and the deterministic `SwitchLayout -> Confirm -> Erase -> Replay` plan.
- **AppConfig**: User configuration loaded from `langflip.toml` with validated defaults.

---

## Planned Growth Path *(Out of Scope for MVP, Designed For)*

- **Phase 2 — Entry-Area & Focus-Aware Larger Buffers + Automatic Layout Discovery**:
  - Implement `FocusObserver` on Windows via UI Automation to distinguish single-line vs. multi-line controls (`Enter` survives in multi-line editors) and upgrade `KeystrokeBuffer` to a cursor-aware buffer supporting `Left`/`Right`/`Home`/`End`/`Delete` without resetting.
  - Add automatic OS keyboard-layout discovery via `LayoutController::list_installed_layouts()` for zero-config bilingual setups.
- **Phase 3 — Direct Text Reading & Selection Fallback**:
  - Add UIA text/caret reading for surviving mouse clicks inside supported editors, plus a clipboard-based selection fallback for converting arbitrary highlighted text typed earlier.
- **Phase 4 — UX Polish & Additional Platforms**:
  - Language-boundary auto-detection / tap-to-expand, Windows system tray icon & logon auto-start, and macOS / Linux platform adapters reusing `langflip-core`.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Running the isolated toolchain bootstrap on Linux completes with zero files created or modified in `~/.rustup` or `~/.cargo`, and produces a valid `langflip.exe` Windows 11 binary using a committed `Cargo.lock`.
- **SC-002**: 100% of core engine scenarios (typing, backspace, shift/punctuation, every reset boundary in FR-005, empty-buffer toggle, round-trip double-tap, auto-repeat suppression, queue/buffer overflow, failed layout switch, partial injection, 3rd-layout safety, and rapid-typing/focus-change race cancellation) are covered by automated tests passing natively on Linux.
- **SC-003**: On Windows 11, pressing `Pause/Break` converts a mistyped straight-line burst of up to 50 characters and confirms the layout switch in under 150 ms (measured end-to-end with default `replay_delay_ms = 0`) in Notepad, Chrome/Edge, and Windows Terminal/PowerShell.
- **SC-004**: Normal typing latency is imperceptible (hook callback completes in < 1 ms without blocking), idle `Ctrl+C` shutdown exits cleanly within 500 ms, and no keystrokes are ever persisted to disk.
- **SC-005**: Pressing, releasing, and pressing `Pause/Break` a second time cleanly round-trips the converted burst back to the original characters and original layout with zero missing or extra characters; holding `Pause/Break` triggers only once.

---

## Assumptions

- The target Windows 11 system has the configured keyboard layouts installed in Windows Language Settings (default: US QWERTY and Israeli Hebrew Standard).
- Cross-compilation from Linux uses `x86_64-pc-windows-gnu` (requiring `mingw-w64` on the Linux host) to produce a self-contained `.exe` with no MSVC SDK dependency.
- MVP is scoped to straight-line, non-IME, non-dead-key text entry (IME-based languages like CJK and dead-key composition sequences are out of scope for v1).
- In v1 (MVP), `langflip.exe` runs as a lightweight console/background binary without a system tray UI or installer; tray UI and auto-start are deferred to Phase 4.
- Elevated (Administrator) windows on Windows 11 reject input injection from non-elevated processes; LangFlip aborts safely without erasing text, and running elevated is documented as an opt-in option.
