# Feature Specification: LangFlip — Browser Address Bar (Omnibox) Inline-Autocomplete Handling

**Feature Branch**: `002-omnibox-autocomplete`

**Created**: 2026-11-26

**Status**: Draft

**Input**: User description: "Address browser address bar (Edge/Chrome Omnibox) inline-autocomplete behavior where an automatically highlighted suffix consumes Backspace #1 during conversion, leaving a stray character behind on each round-trip."

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Clean Conversion in Browser Address Bars with Inline Autocomplete (Priority: P1)

As a Windows 11 user typing in Edge or Chrome's address bar (Omnibox), when I start typing in the wrong keyboard language and press `Pause/Break` (and optionally press it again to round-trip), I want LangFlip to detect and dismiss any automatically highlighted inline-autocomplete suffix before erasing my typed characters, so zero stray characters are left behind.

**Why this priority**: Directly fixes the Omnibox stray-character bug observed during MVP live testing without affecting any other application.

**Independent Test**:
1. Core TDD tests on Linux verify that when the focus observer reports an active inline selection on a non-empty burst, the conversion plan dismisses the selection before erasing the `N` buffered keystrokes, and when no selection is reported, erasure remains strictly `N`.
2. On Windows 11 in Microsoft Edge's address bar, type a mistyped prefix that triggers inline English autocomplete when converted (e.g., `"עםםעךק"` → `"google"`), press `Pause/Break` multiple times, and verify zero stray characters accumulate.

**Acceptance Scenarios**:

1. **Given** the user is in Edge's address bar, types `"עםםעךק"` (`google`) in Hebrew, and presses `Pause/Break`, **When** Edge switches to English and displays `"google"` with an inline highlighted autocomplete suffix (e.g., `.com`), **Then** pressing `Pause/Break` again dismisses the inline selection first, erases all 6 characters cleanly, and converts back to `"עםםעךק"` with zero stray characters.
2. **Given** the user is in Edge's address bar in English layout, types `"goo"` (triggering inline autocomplete `[gle.com]`), and presses `Pause/Break`, **When** conversion runs, **Then** the inline autocomplete suffix is dismissed, all 3 typed characters are erased cleanly, and the address bar contains `"עום"` (no leftover `"g"`).
3. **Given** the user is in Notepad, Windows Terminal, or a standard web `<textarea>` with no active inline selection, **When** conversion runs, **Then** erasure sends strictly `N` Backspaces (zero extra Backspaces, zero regression).
4. **Given** the OS accessibility query fails or is unsupported by the focused control, **When** conversion runs, **Then** LangFlip safely falls back to `has_inline_selection = false` (strictly `N` Backspaces, never over-erasing).

---

### Edge Cases

- **Inline autocomplete appears AFTER switching layout**: The selection check MUST run right before erasure (after the burst was typed and right before planning erasure) so it reflects the exact current state of the address bar at erasure time.
- **Empty buffer**: If the buffer is empty, inline-selection detection MUST NOT emit any Backspace (empty-buffer toggle remains layout-only).
- **Hook latency**: Accessibility / selection inspection MUST run on the worker thread at trigger time, NEVER inside the low-level keyboard hook callback (`max_hook_us` stays < 1 ms).
- **Privacy**: Selection inspection MUST ONLY check whether a non-empty selection exists (boolean) and MUST NEVER read, log, or store the selected text or URL.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The core OS focus contract MUST expose a non-breaking capability to query whether the currently focused input control has an active non-empty text selection (`has_active_selection() -> bool`, defaulting to `false`).
- **FR-002**: When converting a non-empty burst and `has_active_selection()` returns `true`, the core conversion planner/engine MUST prepend a single selection-dismissal Backspace before the `N` character Backspaces (`erase_count = N + 1`) while still retaining the exact `N` buffered keystrokes for replay and round-trip.
- **FR-003**: When `has_active_selection()` returns `false` (or fails/times out), `erase_count` MUST remain strictly `N`.
- **FR-004**: On Windows 11, the OS adapter MUST implement `has_active_selection()` on the worker thread using UI Automation text-selection inspection (checking only whether any selection range is non-empty, without logging or retaining any text).
- **FR-005**: The low-level keyboard hook callback MUST NOT invoke UI Automation (`max_hook_us` remains < 1 ms).

### Key Entities

- **FocusObserver::has_active_selection(&self) -> bool**: Cross-platform boolean query on the existing focus seam indicating whether the focused control currently holds an active highlighted text selection (such as Omnibox inline autocomplete).

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Core TDD unit & integration tests on Linux verify `has_active_selection() == true` adds 1 dismissal Backspace while replaying `N` keys, `false` sends strictly `N` Backspaces, and empty-buffer toggles emit 0 Backspaces.
- **SC-002**: On Windows 11 Edge address bar, typing `"עםםעךק"` (`google`) and pressing `Pause/Break` 6+ times in a row cleanly round-trips `"עםםעךק"` ↔ `"google"` with zero stray characters.
- **SC-003**: Notepad and Windows Terminal continue to convert cleanly with zero extra Backspaces and `< 1 ms` hook latency.
