# Feature Specification: LangFlip MVP — In-Place Keyboard Layout Converter

**Feature Branch**: `001-mvp-layout-converter`

**Created**: 2026-11-25

**Status**: Draft (Design Review Revision 3)

**Input**: User description: "A cross-platform-architected Rust utility (initially targeting Windows 11) that converts accidentally mistyped text between configurable keyboard languages (defaulting to English ↔ Hebrew) in-place across any application (browsers, native apps, command-line terminals) and switches the active keyboard layout on a configurable hotkey (default: Pause/Break). Starts with the naive low-dependency keystroke-buffer case while designing clear extension seams for larger focus-aware buffers, automatic OS layout discovery, and future OS targets. Includes setting up an isolated project-local Rust dev environment and Windows cross-compilation toolchain."

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Isolated Rust Dev Environment & Windows Cross-Build (Priority: P1)

As a developer on Linux targeting Windows 11, I want all host and toolchain dependencies declaratively configured in the repository and a repeatable setup that installs an exact pinned Rust toolchain and Windows cross-compilation target inside a project-local isolated directory without modifying global user directories or shell startup files, so a fresh clone can audit/install every prerequisite, run unit tests natively on Linux, and produce a standalone Windows 11 executable cleanly.

**Why this priority**: Foundation for all subsequent development and testing; ensures zero pollution of the host machine's global environment.

**Independent Test**: Run the project's environment setup script on a Linux machine with no global Rust installation, verify global Rust directories and shell profiles (`~/.rustup`, `~/.cargo`, `~/.profile`, `~/.bashrc`) are not created or modified, run unit tests for the platform-independent core, and compile a locked release binary for Windows 11.

**Acceptance Scenarios**:

1. **Given** a fresh clone on a Linux host without a global Rust installation, **When** the developer runs the dependency audit and isolated setup script, **Then** every required host tool is checked/installable from project configuration, and the exact pinned Rust toolchain + Windows target are installed strictly inside the repository's isolated toolchain directory without editing shell profile files.
2. **Given** the isolated toolchain is installed, **When** the developer runs the project's test, lint, and build wrapper commands, **Then** core unit tests pass on Linux, cross-target static checks pass, and a standalone Windows 11 executable is produced using the committed lockfile.
3. **Given** the developer deletes the project-local toolchain directory, **When** inspecting the host home directory, **Then** no leftover Rust/Cargo state or shell profile edits remain.

---

### User Story 2 - Instant Mistype Conversion & Layout Switch in Any App (Priority: P1)

As a bilingual Windows 11 user, when I start typing straight-line text in the wrong keyboard language inside any standard text input (browser, desktop app, or command-line terminal) and realize my mistake, I want to press a single hotkey (`Pause/Break` by default) so the utility safely switches my active keyboard layout to the other configured language, confirms the switch succeeded, erases the mistyped burst, replays the text in the new language, and lets me continue typing seamlessly.

**Why this priority**: This is the core MVP value proposition. Solving the naive unbroken-burst case safely (never deleting text unless the layout switch is confirmed and target focus/sequence is intact) immediately eliminates daily friction without risking data loss.

**Independent Test**: Launch the utility on Windows 11, type a mistyped English phrase while in Hebrew layout (and vice versa) in Notepad, a browser text field, and Windows Terminal/PowerShell, press `Pause/Break`, and verify the text is replaced in-place, the Windows keyboard layout has toggled, and subsequent keystrokes appear in the new language.

**Acceptance Scenarios**:

1. **Given** Windows 11 is in Hebrew layout and the user intends to type `"hello world"` in a browser input, **When** the user types `"יקךךם 'םרךג"` and presses `Pause/Break`, **Then** the mistyped characters are replaced in-place with `"hello world"`, the active keyboard layout switches to English (US), and further typing produces English.
2. **Given** Windows 11 is in English layout and the user intends to type `"שלום עולם"` in Notepad or Windows Terminal, **When** the user types `"akuo guko"` and presses `Pause/Break`, **Then** the mistyped characters are replaced in-place with `"שלום עולם"`, the active keyboard layout switches to Hebrew, and further typing produces Hebrew.
3. **Given** the user typed `"hello"` with a typo, pressed `Backspace` once, and typed `"o"` before pressing `Pause/Break`, **When** conversion runs, **Then** the buffer accurately reflects the edited text (`"hello"`), not the deleted character.
4. **Given** the user typed a string containing Shift-modified keys or punctuation (e.g., `"Hello, World!"` or Hebrew punctuation on `/` and `'`), **When** conversion runs, **Then** Shift state and physical-key punctuation map accurately to the target layout.
5. **Given** the user pressed `Enter`, clicked the mouse, pressed an arrow key, toggled `Insert` or `CapsLock`, changed keyboard layouts externally, or switched windows before typing a new burst, **When** the user presses `Pause/Break` after the new burst, **Then** ONLY the new burst after the reset boundary is erased and converted (prior text is never touched).
6. **Given** the buffer is empty (nothing typed since the last reset or conversion), **When** the user presses `Pause/Break`, **Then** no Backspaces are sent and the utility toggles the active keyboard layout per `FR-007` (when `toggle_on_empty_buffer` is enabled, the default).
7. **Given** the user triggers conversion and immediately types a new non-hotkey key or changes window focus before erasure begins, **When** the conversion engine validates the trigger snapshot, **Then** conversion is cancelled cleanly without erasing text (and if the layout was already switched, the engine restores the prior layout best-effort). *(Note: keys typed during the brief millisecond window after erasure has already started cannot be prevented atomically by an asynchronous hook; this limitation is explicitly acknowledged.)*
8. **Given** the target application rejects or ignores the layout switch request, **When** the user presses `Pause/Break`, **Then** no Backspaces are sent, the user's existing text is preserved intact, and the buffer is cleared.

---

### User Story 3 - Configurable Hotkey, Language Pair & Safe Runtime Lifecycle (Priority: P2)

As a user who may use a different 1:1 keyboard language pair (e.g., English ↔ Russian, English ↔ Ukrainian, English ↔ Greek, or a specific layout variant such as US Dvorak or Hebrew Standard 2018) or whose keyboard lacks a `Pause/Break` key, I want to configure the language/layout pair, trigger hotkey, and basic behavior via a simple local config file, and start/stop the utility cleanly — even while idle — without leaving stuck hooks or modifiers.

**Why this priority**: Physical scan-code replay naturally supports any 1:1 non-IME keyboard layout pair without hardcoding character tables, and compact keyboards often omit `Pause/Break`.

**Independent Test**: Edit the config file to bind an alternative hotkey (e.g., `Ctrl+Shift+L` or `F9`) and/or an alternative layout pair, restart the utility, verify the configured hotkey and layout pair are used without stuck modifiers or accidental menu/layout misfires, and verify terminating the utility while completely idle cleanly unhooks input and exits immediately.

**Acceptance Scenarios**:

1. **Given** no config file exists on first launch, **When** the utility starts, **Then** it runs with sensible defaults (`Pause/Break` hotkey, bounded buffer of 128 keystrokes, English-US ↔ Hebrew layout pair).
2. **Given** a valid user config file specifying a custom hotkey and/or a custom pair of installed OS keyboard layouts (by language tag or exact OS layout ID), **When** the utility starts, **Then** the custom hotkey toggles and converts between those configured layouts while the previous default key behaves normally.
3. **Given** an invalid user config file (syntax error or out-of-range value), **When** the utility starts, **Then** it fails fast with a clear error message identifying the invalid setting and exits with a non-zero status rather than silently ignoring the mistake.
4. **Given** the utility is running and completely idle (no user input occurring), **When** the user terminates it (e.g., `Ctrl+C`), **Then** the input listener wakes up immediately, all OS input hooks are cleanly removed, and the process exits cleanly.

---

### User Story 4 - Cross-Platform Core Architecture & Extension Seams (Priority: P2)

As the project maintainer, I want all domain logic (key classification, hotkey matching & auto-repeat suppression, buffering, reset policy, replay sequence planning, layout-pair resolution, conversion orchestration, and configuration) to be strictly OS-independent and decoupled behind narrow OS boundaries, so that:
1. All core logic is thoroughly TDD-tested on Linux without Windows.
2. Future phases (focus/entry-area-aware larger buffers, direct caret/text reading, selection-based fallback, and macOS/Linux backends) can be added without rewriting the core.

**Why this priority**: Directly fulfills the cross-platform design goal and strict TDD constitution principle.

**Independent Test**: Run the core test suite natively on Linux with mocked OS boundaries exercising key classification, hotkey matching, replay planning, layout resolution, buffer/reset rules, race cancellation, overflow, and config parsing.

**Acceptance Scenarios**:

1. **Given** the OS-independent core library with zero platform-specific dependencies, **When** unit and integration tests run on Linux, **Then** all domain, planning, safety, and config tests execute and pass natively.
2. **Given** a future developer wants to add a macOS/Linux backend or a focus-aware multi-line buffer, **When** inspecting the core interfaces, **Then** platform I/O and buffer policy are cleanly separated behind narrow boundaries.

---

### Edge Cases

- **Failed or rejected layout switch**: If the OS or target app rejects or does not confirm the layout change within the confirmation window, LangFlip MUST abort before sending any Backspaces so no user text is lost.
- **Elevated (Administrator) target windows on Windows 11**: A non-elevated input hook does not receive keystrokes directed to an elevated window at all; keystrokes (including the hotkey) pass straight to the elevated window untouched. Elevated-window support requires running LangFlip elevated (opt-in).
- **Partial synthetic key injection**: If the OS reports fewer injected events than requested during erasure or replay, LangFlip MUST abort the remaining sequence, clear the buffer (not retaining it for round-trip), and surface a non-sensitive diagnostic warning.
- **Rapid typing or focus change during conversion**: Releasing the trigger hotkey or its combo modifiers MUST NOT cancel conversion. However, if a new non-hotkey keystroke or focus change arrives before erasure begins, conversion MUST cancel cleanly (and best-effort restore the prior layout if it already switched).
- **Self-injected vs. externally injected keystrokes**: Synthetic keystrokes emitted by LangFlip itself MUST be ignored; synthetic keystrokes injected by *other* software (on-screen keyboard, password-manager auto-type, macro tools) MUST invalidate/reset the active burst so Backspace counts never desync.
- **Auto-repeat on held hotkey**: Holding down the trigger hotkey MUST fire only once until the key is released. Pressing, releasing, and pressing again triggers a second conversion (clean round-trip back to the original language and text).
- **Left/right combo modifiers & menu/layout misfires**: When a modifier-combo hotkey (e.g., `Right-Ctrl+Shift+L` or `Alt+Shift+L`) is triggered, LangFlip MUST release the exact left/right modifiers that are held and insert a harmless mask key so releasing `Alt` does not open application menus and releasing `Ctrl+Shift` does not trigger the OS's own layout switch.
- **Caps Lock, Insert, and NumLock-off numpad keys**: Toggling `CapsLock` or `Insert`, or pressing numpad navigation keys while `NumLock` is off, MUST reset the naive burst so replay never desyncs casing, overwrite mode, or cursor position.
- **Dead keys, multi-character ligature keys, IMEs, and autocomplete**: MVP is scoped to straight-line, 1-keystroke-to-1-character, non-IME, non-dead-key layouts (e.g., EN-US, Hebrew, Russian, Ukrainian, Greek). Layouts with multi-character ligature keys (such as Arabic lam-alef) or unmapped no-op keys are out of scope for MVP v1.
- **Buffer overflow vs. event queue overflow**:
  - When the keystroke buffer reaches its configured maximum (default 128, valid range `1..=512`), it drops the oldest keystroke and retains the most recent tail; pressing the hotkey converts that retained tail safely.
  - When the bounded raw-input event queue overflows (events dropped before reaching the engine), the entire active burst MUST be invalidated so conversion never runs on an incomplete event history.
- **Third/unrelated installed layout**: If the user is currently typing in a 3rd layout outside the configured pair, pressing the hotkey on a non-empty burst MUST NOT destructively erase text that cannot round-trip (it clears the burst and skips conversion; on an empty buffer it switches to the primary configured layout).
- **Ambiguous or missing configured layouts**: If a configured language tag matches multiple installed layouts (e.g., multiple Hebrew or English variants), LangFlip MUST select deterministically and emit a startup warning recommending an exact OS layout ID. If a configured layout is not installed, LangFlip MUST warn clearly at startup and never corrupt input.

---

## Requirements *(mandatory)*

### Functional Requirements

#### Isolated Toolchain & Build
- **FR-001**: The repository MUST declare **all** build and development dependencies (both host tools already present and not yet installed, an exact pinned Rust release with lint/format components, target triples, pinned installer checksum, and locked crate dependencies) in version-controlled configuration so a fresh clone knows every prerequisite required to build and test.
- **FR-002**: The repository MUST provide an idempotent bootstrap workflow that audits and optionally installs missing host packages, installs the pinned Rust toolchain and Windows target strictly inside a project-local directory without touching global user directories or shell startup files, and pre-fetches crates.
- **FR-003**: The repository MUST provide isolated command wrappers to run native host tests, host + cross-target lints, and locked Windows cross-builds strictly using the project-local toolchain.

#### Core Conversion Engine (Cross-Platform)
- **FR-004**: The core engine MUST maintain a bounded, RAM-only buffer of the current unbroken burst of physical keystrokes (default max 128, allowed range `1..=512`, dropping oldest entries on buffer overflow so conversion safely operates on the retained tail), fed from a bounded raw-input event queue whose overflow invalidates the active burst.
- **FR-005**: The core engine MUST update the buffer on `Backspace` (removing the last buffered keystroke if non-empty) and MUST reset the buffer on naive context boundaries: `Enter`, `Tab`, `Escape`, `Insert`, navigation keys (`Left`, `Right`, `Up`, `Down`, `Home`, `End`, `PageUp`, `PageDown`, `Delete`), numpad navigation keys when `NumLock` is off, `CapsLock` toggle, `Ctrl`/`Alt`/`Meta` shortcuts (including AltGr), externally injected keystrokes, mouse clicks, window/focus changes, and external keyboard layout changes.
- **FR-006**: When the trigger hotkey is pressed (ignoring key auto-repeat while held) and the buffer is non-empty, the engine MUST bind the conversion to the active focus target and mutating-input sequence (where releasing the hotkey or its modifiers does NOT advance the sequence), and execute in safe order:
  1. Verify the active layout belongs to the configured layout pair (if it is an unrelated 3rd layout, clear the burst and abort without erasing).
  2. Verify focus target and mutating-input sequence are fresh; if stale, abort cleanly and clear the buffer.
  3. Request a keyboard layout switch to the opposite layout in the configured pair and **confirm the switch succeeded** before any erasure; if unconfirmed or rejected, abort cleanly with zero erasure and clear the buffer.
  4. Re-verify focus target and mutating-input sequence are still fresh; if stale, abort cleanly without erasing, best-effort restore the prior layout, and clear the buffer.
  5. Plan and execute the injection sequence (mask key + exact left/right held-modifier release + `N` Backspaces + physical keystroke replay under the new layout), verifying complete injection.
  6. Retain the converted buffer (marked with the new layout) **only after full success** so a subsequent press-release-press cleanly reverses the conversion. On any partial injection failure, abort remaining steps and clear the buffer.
- **FR-007**: When the trigger hotkey is pressed and the buffer is empty, the engine MUST NOT emit any Backspaces and MUST toggle the active keyboard layout to the opposite configured layout (or to the primary layout if currently on an unrelated 3rd layout) when `toggle_on_empty_buffer` is enabled (default: `true`).
- **FR-008**: The system MUST distinguish its own self-injected synthetic events (ignored) from externally injected synthetic events (reset active burst).
- **FR-009**: All domain logic (key classification, hotkey matching & auto-repeat, buffering, layout-pair resolution, replay sequence planning, conversion orchestration, and config validation) MUST reside in an OS-independent core module with zero platform-specific dependencies.

#### Windows 11 Adapter (v1 Target)
- **FR-010**: The Windows adapter MUST capture global keyboard, mouse-click, and foreground focus-change events non-blockingly into the bounded event queue, tag each key event with the active foreground layout at capture time, suppress the trigger hotkey from reaching foreground apps, and provide an explicit wake-up path so graceful shutdown completes immediately even while completely idle.
- **FR-011**: The Windows adapter MUST execute the core's planned injection steps (including mask key, exact left/right modifier release, Backspaces, and physical scan-code replay) and report OS-reported short-write failures back to the core engine.
- **FR-012**: The Windows adapter MUST detect the foreground target's active keyboard layout, resolve configured layout identifiers (exact OS layout IDs first, then language tags with ambiguity warnings), switch reliably between the configured pair, and confirm the layout change before erasure proceeds.
- **FR-013**: The Windows adapter MUST remove all OS input and focus hooks cleanly upon graceful shutdown.

#### Configuration & Privacy
- **FR-014**: The utility MUST support an optional local configuration file searched in deterministic order: (1) alongside the executable (portable override), then (2) the OS user config directory, falling back to built-in defaults if neither file exists. Invalid syntax or out-of-range values MUST fail fast at startup with a clear error message. Configurable settings:
  - Keyboard layout pair (language tags or exact OS layout IDs; default: `en-US` ↔ `he-IL`).
  - Trigger hotkey matched by physical key (default: `Pause`).
  - Max buffer size (default: `128`, valid range `1..=512`).
  - Per-keystroke replay delay in milliseconds (default: `0` ms, valid range `0..=10` ms).
  - Toggle-on-empty-buffer behavior (default: `true`).
- **FR-015**: Captured keystrokes MUST remain strictly in volatile memory and MUST NEVER be written to disk, sent over the network, emitted to logs (debug or release), or exposed in debug/panic output.

### Key Entities

- **Input Event**: Normalized representation of a user action (key press/release with physical key, left/right modifiers, active layout, injection origin, focus target, and mutating sequence number; mouse click; focus change; queue overflow).
- **Keystroke Buffer**: Bounded, RAM-only, debug-redacted sequence of typed physical keystrokes for the active burst.
- **Layout Pair**: Configured pair of keyboard layout identifiers (language tags or exact OS layout IDs) resolved against installed OS layouts by the platform adapter.
- **Conversion Plan**: Deterministic sequence of safety checks, layout switch, and low-level injection steps (mask key, modifier release, Backspaces, keystroke replay) produced by the core engine.
- **Application Configuration**: Validated user settings loaded from the optional config file with built-in defaults.

---

## Planned Growth Path *(Out of Scope for MVP, Designed For)*

- **Phase 2 — Entry-Area & Focus-Aware Larger Buffers + Automatic Layout Discovery**:
  - Upgrade Windows focus observation to UI Automation to distinguish single-line vs. multi-line controls (`Enter` survives in multi-line editors) and upgrade the buffer to a cursor-aware buffer supporting `Left`/`Right`/`Home`/`End`/`Delete` without resetting.
  - Add automatic OS keyboard-layout discovery for zero-config bilingual setups.
- **Phase 3 — Direct Text Reading, Multi-Char Keys & Selection Fallback**:
  - Add UIA text/caret reading for surviving mouse clicks inside supported editors, accurate character-count tracking for multi-character ligature keys (e.g., Arabic), and a clipboard-based selection fallback for converting arbitrary highlighted text typed earlier.
- **Phase 4 — UX Polish & Additional Platforms**:
  - Language-boundary auto-detection / tap-to-expand, Windows system tray icon & logon auto-start, and macOS / Linux platform adapters reusing the OS-independent core.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Running the isolated toolchain bootstrap on Linux completes with zero files created or modified in `~/.rustup`, `~/.cargo`, `~/.profile`, or `~/.bashrc`, and produces a valid Windows 11 executable using a committed lockfile.
- **SC-002**: 100% of core engine scenarios (key classification, hotkey matching & auto-repeat, replay sequence planning with mask key & left/right modifiers, layout resolution & ambiguity warnings, typing, backspace, shift/punctuation, every reset boundary in FR-005, empty-buffer toggle, round-trip double-tap, buffer tail overflow, queue overflow, failed layout switch, partial injection, 3rd-layout safety, and rapid-typing/focus-change race cancellation) are covered by automated tests passing natively on Linux.
- **SC-003**: On Windows 11, pressing `Pause/Break` converts a mistyped straight-line burst of up to 50 characters and confirms the layout switch in under 150 ms (measured end-to-end with default `0` ms delay) in Notepad, Chrome/Edge, and Windows Terminal/PowerShell.
- **SC-004**: Normal typing latency is imperceptible (hook callback completes in < 1 ms without blocking), idle `Ctrl+C` shutdown exits cleanly within 500 ms, and no keystrokes are ever logged or persisted to disk.
- **SC-005**: Pressing, releasing, and pressing `Pause/Break` a second time cleanly round-trips the converted burst back to the original characters and original layout with zero missing or extra characters; holding `Pause/Break` triggers only once.

---

## Assumptions

- The target Windows 11 system has the configured keyboard layouts installed in Windows Language Settings (default: English-US and Hebrew).
- Cross-compilation from Linux uses `x86_64-pc-windows-gnu` (requiring `mingw-w64` on the Linux host) to produce a self-contained executable with no MSVC SDK dependency.
- MVP is scoped to straight-line, 1-keystroke-to-1-character, non-IME, non-dead-key layouts (e.g., EN-US, Hebrew, Russian, Ukrainian, Greek).
- In v1 (MVP), LangFlip runs as a lightweight console/background binary without a system tray UI or installer; tray UI and auto-start are deferred to Phase 4.
- Non-elevated LangFlip does not observe elevated (Administrator) Windows apps; running elevated is an opt-in user choice if conversion inside elevated terminals is desired.
