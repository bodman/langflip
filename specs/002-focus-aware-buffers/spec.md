# Feature Specification: LangFlip Phase 2 — Focus-Aware & Cursor-Tracked Buffers, Omnibox Handling & Auto Layout Discovery

**Feature Branch**: `002-focus-aware-buffers`

**Created**: 2026-11-26

**Status**: Draft

**Input**: User description: "Extend LangFlip beyond the naive single-burst MVP with: (1) cursor-tracked buffer editing across keyboard navigation (Left/Right/Home/End/Delete), (2) focus-aware entry-area tracking that distinguishes multi-line editors (surviving Enter) from single-line inputs and terminals, (3) clean browser address bar (Omnibox) conversion when inline autocomplete highlights a suffix, and (4) automatic OS keyboard-layout discovery for zero-config bilingual setups."

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Cursor-Aware Editing Within a Burst (Priority: P1)

As a typist who occasionally fixes a typo mid-word using keyboard navigation (`Left`, `Right`, `Home`, `End`, `Delete`, `Backspace`) before realizing I am in the wrong keyboard language, I want LangFlip to track my cursor position inside the active burst instead of discarding the buffer, so pressing the hotkey still converts the entire edited phrase cleanly.

**Why this priority**: Fixing a typo with an arrow key or `Delete` is one of the most common actions while typing; preserving the burst across horizontal keyboard edits dramatically increases how often LangFlip "just works" without adding fragile OS dependencies.

**Independent Test**: Run core unit and integration tests natively on Linux simulating typing, moving with `Left`/`Right`/`Home`/`End`, inserting mid-word, deleting with `Delete`/`Backspace`, and triggering conversion — verifying exact erasure (both before and after the cursor) and exact replayed keystroke order. Then verify on Windows 11 in Notepad, Edge, and PowerShell.

**Acceptance Scenarios**:

1. **Given** the user is in Hebrew layout and types `"יקךם"` (`helo`), presses `Left` once, types `"ך"` (`l`) to fix it to `"יקךךם"` (`hello`), and presses `Pause/Break`, **When** conversion runs, **Then** all 5 characters (`יקךךם`) are erased cleanly (accounting for 1 character to the right of the cursor) and replaced with `"hello"`, leaving the cursor at the end ready to continue typing.
2. **Given** the user types `"קךךם"` (`ello`), presses `Home`, types `"י"` (`h`), and presses `Pause/Break`, **When** conversion runs, **Then** all 5 characters are replaced cleanly with `"hello"` and the cursor is placed at the end.
3. **Given** the user types `"יקXךךם"`, presses `Left` three times, presses `Delete` to remove `X`, and presses `Pause/Break`, **When** conversion runs, **Then** the edited 5-character word converts cleanly to `"hello"`.
4. **Given** the user presses `Left` past the beginning of the buffered burst (or `Right` past the end), **When** cursor tracking detects an out-of-buffer jump, **Then** the buffer resets safely so LangFlip never erases unrelated text outside the tracked burst.

---

### User Story 2 - Focus-Aware Multi-Line vs. Single-Line & Terminal Buffers (Priority: P1)

As a user typing a multi-line message or note in a multi-line editor (e.g., Notepad, web `<textarea>`, chat/email body), I want pressing `Enter` to stay inside the active buffer so I can convert a multi-line mistyped paragraph with one hotkey press — while pressing `Enter` in a single-line field (search bar, address bar) or command-line terminal still resets the buffer because `Enter` submits the input.

**Why this priority**: Multi-line mistyping (two short lines before noticing) is a frequent pain point in editors and chat apps, and safely distinguishing multi-line vs. single-line/terminal controls requires knowing what kind of entry area has focus.

**Independent Test**: Verify via mocked focus-context tests on Linux that `Enter` is buffered when the focused control is `MultiLineText` and resets the buffer when `SingleLineText`, `Terminal`, or `Unknown`; verify on Windows 11 by typing two lines in Notepad (`יקךךם` + `Enter` + `'םרךג`) and converting both lines with a single `Pause/Break`, while verifying `Enter` in PowerShell and Edge's address bar still resets the buffer.

**Acceptance Scenarios**:

1. **Given** focus is in a multi-line editor (e.g., Notepad or a web `<textarea>`) and the user types `"יקךךם"`, presses `Enter`, types `"'םרךג"`, and presses `Pause/Break`, **When** conversion runs, **Then** both lines (including the newline) are replaced in-place with `"hello"` + newline + `"world"`, and the layout switches to English.
2. **Given** focus is in a single-line input (e.g., browser address bar or search input) or a terminal (PowerShell / Windows Terminal), **When** the user presses `Enter`, **Then** the buffer resets immediately so previously submitted text is never erased.
3. **Given** focus moves from one input control to a different input control within the *same* window (e.g., tabbing from Subject to Body in an email client, or from address bar to page content), **When** the focus observer reports a new control identity, **Then** the buffer resets cleanly.
4. **Given** the OS cannot determine the focused control type (`Unknown`), **When** the user presses `Enter`, **Then** LangFlip conservatively resets the buffer (safe fallback, zero risk of over-erasure).

---

### User Story 3 - Browser Address Bar (Omnibox) Inline-Autocomplete Handling (Priority: P2)

As a user typing in a browser address bar (Edge/Chrome Omnibox), when I type in the wrong language and convert to English, I want LangFlip to dismiss any highlighted inline autocomplete suggestion before erasing my typed text so converting does not leave stray characters behind.

**Why this priority**: Directly resolves the Edge address-bar quirk observed during MVP live testing without breaking any other application.

**Independent Test**: In Edge's address bar, type a mistyped prefix that triggers inline English autocomplete when converted (e.g., `יקךךם` or `עםםעךק` → `google`), press `Pause/Break` repeatedly, and verify zero stray characters accumulate across conversions.

**Acceptance Scenarios**:

1. **Given** the user is in Edge/Chrome's address bar, types `"עםםעךק"` (`google`) in Hebrew, and presses `Pause/Break`, **When** conversion runs, **Then** the address bar contains `"google"` (with any inline autocomplete handled cleanly) and pressing `Pause/Break` again cleanly converts back without leaving stray trailing or leading characters.
2. **Given** the user is in a standard web input or terminal without inline autocomplete selection, **When** conversion runs, **Then** normal erasure behavior is unchanged (no extra Backspaces are ever sent).

---

### User Story 4 - Automatic OS Keyboard Layout Discovery (Priority: P2)

As a bilingual user of *any* 1:1 keyboard language pair (e.g., English/Hebrew, English/Russian, English/Greek, French/Hebrew) who has exactly two keyboard layouts installed on Windows 11, I want LangFlip to automatically discover and toggle between my two installed layouts out of the box when I haven't explicitly configured layouts in `langflip.toml`.

**Why this priority**: Eliminates configuration friction for every bilingual user regardless of language pair, while preserving explicit `langflip.toml` overrides when 3+ layouts are installed.

**Independent Test**: Run core unit tests natively on Linux verifying auto-discovery with 2 installed layouts (e.g., `fr-FR` + `ru-RU`), explicit config override precedence, and clear fallback/warning behavior when 1 or 3+ layouts are installed.

**Acceptance Scenarios**:

1. **Given** no `[layouts]` section is configured in `langflip.toml` and the OS reports **exactly 2** installed keyboard layouts (e.g., `en-IN` and `he-IL`, or `de-DE` and `uk-UA`), **When** LangFlip starts, **Then** it automatically selects those two layouts as the active toggle pair with zero warnings.
2. **Given** `[layouts]` *is* explicitly specified in `langflip.toml`, **When** LangFlip starts, **Then** the user's explicit configuration always takes precedence over auto-discovery.
3. **Given** no `[layouts]` section is configured and the OS reports **3 or more** installed layouts, **When** LangFlip starts, **Then** it falls back to matching `en` ↔ `he` (if present) or warns clearly to configure `[layouts]` in `langflip.toml`.
4. **Given** the OS reports fewer than 2 installed layouts, **When** LangFlip starts, **Then** it prints a clear warning explaining that at least two installed keyboard layouts are required.

---

### Edge Cases

- **Mouse click inside the same control**: Still resets the active buffer in Phase 2 (determining exact character index from a mouse click requires full caret-range reading, deferred to Phase 3).
- **Vertical navigation (`Up` / `Down` / `PageUp` / `PageDown`)**: Soft line wrapping in multi-line editors makes vertical cursor displacement unknowable from keystrokes alone, so `Up`/`Down`/`PageUp`/`PageDown` MUST reset the buffer safely.
- **Word jumps and text selections (`Ctrl+Left`, `Ctrl+Right`, `Shift+Left`, `Shift+Right`, `Shift+Home`, `Shift+End`, `Ctrl+A`)**: MUST reset the buffer safely since selection lengths and word boundaries are app-defined.
- **Cursor at middle of multi-line buffer on trigger**: When converting while the cursor is mid-burst (`cursor < len`), LangFlip MUST erase characters to the right of the cursor (`Delete` × `len - cursor`) as well as characters to the left (`Backspace` × `cursor`) — or move to the end first and Backspace `len` — so zero tail characters are left behind.
- **Multi-line `Home`/`End` semantics**: In a multi-line buffer containing `Enter` (`\n`), `Home` moves the virtual cursor to the start of the **current line** (just after the previous `Enter`, or index 0 on line 1), and `End` moves to the end of the **current line** (just before the next `Enter`, or `len` on the last line) — matching real editor behavior. Pressing `Home` on a line that started *before* the tracked burst (i.e., `Home` when line 1 has untracked prefix text) is handled conservatively: if `cursor == 0` and user presses `Left`, reset; `Home`/`End` on a tracked multi-line buffer operate within tracked lines, while `Ctrl+Home`/`Ctrl+End` reset.
- **Focus observer latency**: Control-type inspection (e.g., querying accessibility metadata) MUST run asynchronously off the low-level input hook callback so hook latency stays strictly < 1 ms and typing never lags.
- **Buffer overflow with cursor tracking**: If a burst exceeds `max_buffer_len` (default `128`, range `1..=512`), oldest entries drop from the front and `cursor` adjusts accordingly (never underflowing). If `cursor` itself would be evicted, the buffer resets safely.
- **Privacy invariant**: Cursor-tracked and multi-line buffers remain strictly RAM-only with redacted `Debug` and zero keystroke logging in all builds.

---

## Requirements *(mandatory)*

### Functional Requirements

#### Cursor-Aware Buffer Policy
- **FR-001**: The core engine MUST provide a cursor-aware buffer policy tracking both the bounded sequence of buffered keystrokes (`len <= max_buffer_len`, valid range `1..=512`) and the virtual cursor index (`0 <= cursor <= len`).
- **FR-002**: Horizontal keyboard edits MUST update the virtual cursor and buffer in-place without resetting:
  - Printable keystroke: inserts at `cursor` and advances `cursor += 1`.
  - `Backspace`: if `cursor > 0`, removes entry at `cursor - 1` and decrements `cursor -= 1`; if `cursor == 0` on a non-empty buffer, resets the buffer (cursor exited left of tracked burst).
  - `Delete`: if `cursor < len`, removes entry at `cursor` (leaving `cursor` unchanged); if `cursor == len` on a non-empty buffer, resets the buffer (erased untracked text to the right).
  - `Left`: if `cursor > 0`, decrements `cursor -= 1`; if `cursor == 0`, resets the buffer.
  - `Right`: if `cursor < len`, increments `cursor += 1`; if `cursor == len`, resets the buffer.
  - `Home` / `End`: moves `cursor` to the start/end of the current line within the tracked buffer (resetting if `Shift`, `Ctrl`, `Alt`, or `Meta` is held).
- **FR-003**: When conversion triggers with `cursor < len`, the injection plan MUST cleanly erase the entire tracked burst (both left and right of `cursor`) before replaying all buffered keystrokes, leaving `cursor = len` on full success.
- **FR-004**: `Up`, `Down`, `PageUp`, `PageDown`, `Tab`, `Escape`, `Insert`, `CapsLock`, any modifier-combo navigation (`Shift+Arrow`, `Ctrl+Arrow`), shortcuts (`Ctrl`/`Alt`/`Meta`), mouse clicks, external synthetic input, external layout changes, and control/window focus changes MUST still reset the buffer safely.

#### Focus-Aware Entry-Area Tracking
- **FR-005**: The OS adapter MUST observe control-level focus changes (not only top-level window changes) off the input-hook callback thread and report each focused control's identity and `ControlKind` (`MultiLineText`, `SingleLineText`, `Terminal`, or `Unknown`).
- **FR-006**: When the active `ControlKind` is `MultiLineText`, pressing `Enter` MUST be recorded as a buffered keystroke (inserting a newline at `cursor`) and replayed cleanly during conversion. When `ControlKind` is `SingleLineText`, `Terminal`, or `Unknown`, pressing `Enter` MUST reset the buffer.
- **FR-007**: Moving focus to a different control identity within the same window MUST reset the active buffer.

#### Omnibox Inline-Autocomplete Handling
- **FR-008**: Before erasing a burst inside a single-line browser address bar where an active inline-autocomplete selection exists after the cursor, the OS adapter MUST report the presence of that inline selection so the conversion plan dismisses it cleanly before erasing the `N` buffered keystrokes (without ever over-erasing when no selection exists).

#### Automatic OS Layout Discovery
- **FR-009**: When `[layouts]` is omitted from `langflip.toml` and the OS reports **exactly 2** installed keyboard layouts, LangFlip MUST automatically select those two layouts as the active toggle pair with zero warnings.
- **FR-010**: Explicit `[layouts]` in `langflip.toml` MUST always override auto-discovery. When `[layouts]` is omitted and the OS reports 3+ layouts, LangFlip MUST fall back to matching `en` ↔ `he` (if installed) or emit a clear startup warning advising explicit configuration.

#### Continuity of Safety & Privacy Guarantees
- **FR-011**: All Phase 1 safety guarantees MUST remain intact: Switch → Confirm before any erasure, trigger-snapshot freshness checks, bounded raw-input queue with overflow invalidation, WinUI 3 Notepad paced injection, idle shutdown wake-up, and RAM-only redacted keystroke privacy.

### Key Entities

- **CursorBuffer / CursorAwarePolicy**: Bounded, RAM-only, debug-redacted buffer with a tracked `cursor: usize` supporting insert, delete-left (`Backspace`), delete-right (`Delete`), horizontal movement (`Left`/`Right`/`Home`/`End`), and multi-line `Enter` entries.
- **ControlFocusInfo**: Focus metadata reported asynchronously by the OS focus observer: `(window/control focus_id, control_kind, has_inline_selection)`.
- **LayoutDiscoveryMode**: Distinguishes explicit user-configured layout pairs from auto-discovered 2-layout OS setups.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of Phase 2 core scenarios (cursor-aware insert/Backspace/Delete/Left/Right/Home/End, out-of-bounds cursor reset, mid-burst conversion erasure, multi-line `Enter` buffering vs. single-line/terminal/unknown `Enter` reset, control-switch reset, Omnibox inline-selection dismissal, and 2-layout auto-discovery vs. 3-layout fallback vs. explicit override) pass natively on Linux via strict TDD.
- **SC-002**: On Windows 11 Notepad, typing a 2-line mistyped Hebrew paragraph (`יקךךם` + `Enter` + `'םרךג`) and pressing `Pause/Break` once converts both lines cleanly to `"hello"` + newline + `"world"`, and fixing a mid-word typo with `Left`/`Delete` before pressing `Pause/Break` converts the exact edited word with zero leftover characters.
- **SC-003**: On Windows 11 Edge address bar, converting a mistyped string that triggers inline English autocomplete leaves zero stray characters across repeated conversions.
- **SC-004**: Control-level focus observation adds zero blocking latency to the low-level input hook callback (`max_hook_us` remains < 1 ms).
- **SC-005**: On a Windows 11 system with any 2 installed layouts and no `langflip.toml`, LangFlip starts cleanly and toggles between both installed layouts automatically with zero configuration warnings.

---

## Assumptions

- Windows UI Automation (`IUIAutomation`) is used off the keyboard-hook thread (on a dedicated COM STA thread) to detect control-level focus changes, classify `MultiLineText` vs. `SingleLineText` vs. `Terminal`, and detect active inline autocomplete selections in Edge/Chrome's address bar.
- If UI Automation is unavailable or returns no metadata for a custom-rendered control, `ControlKind::Unknown` is reported and LangFlip safely falls back to Phase 1 single-line behavior.
- Mouse clicks inside the same control and arbitrary highlighted-text conversion remain deferred to Phase 3.
