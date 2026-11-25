# Data Model & State Machine: LangFlip MVP

**Feature**: `001-mvp-layout-converter` | **Date**: 2026-11-25 (Revised after Design Review)

## Core Domain Types (`langflip-core`)

### 1. `PhysicalKey`
Platform-neutral representation of a physical hardware key:
- `scan_code: u16` — hardware scan code (AT Set 1 on Windows; mapped from OS keycodes on future platforms).
- `extended: bool` — whether the key is an extended key.

### 2. `Modifiers`
Active modifier state at key press/release:
- `shift: bool`
- `ctrl: bool`
- `alt: bool`
- `meta: bool` (Win / Cmd)
- `caps_lock: bool`

### 3. `KeyKind`, `KeyEvent`, `FocusId`, and `InputEvent`
- `FocusId(u64)` — opaque identifier for the foreground target window/control (e.g., `HWND` value on Windows).
- `KeyKind`:
  - `Printable` — produces a straight-line visible character (letters, digits, punctuation, Space).
  - `Backspace` — deletes one prior visible character.
  - `ResetBoundary` — `Enter`, `Tab`, `Escape`, arrows (`Left`/`Right`/`Up`/`Down`), `Home`, `End`, `PageUp`, `PageDown`, `Delete`, `CapsLock`.
  - `Modifier` — `Shift`, `Ctrl`, `Alt`, `Meta` (tracked for held-modifier hygiene, not buffered as characters).
  - `Other` — function keys, media keys, etc. (ignored unless bound as hotkey).
- `InputEvent`:
  - `KeyDown(KeyEvent)` — carries `key`, `kind`, `modifiers`, `is_injected`, `focus_id`, `seq: u64`.
  - `KeyUp(KeyEvent)` — clears held hotkey/modifier state.
  - `MouseClick { focus_id: FocusId, seq: u64 }` — reset trigger in naive MVP.
  - `FocusChanged { focus: FocusContext, seq: u64 }` — reset trigger in MVP; carries `control_kind` in Phase 2.
  - `HotkeyTriggered { focus_id: FocusId, seq: u64, held_modifiers: Modifiers }` — emitted once per press (auto-repeat suppressed).
  - `QueueOverflow` — signals that the bounded event queue dropped events; invalidates the active burst.

### 4. `BufferedKeystroke` & `KeystrokeBuffer`
- `BufferedKeystroke`: `{ key: PhysicalKey, shift: bool }` (Caps Lock is consistent across a burst because `CapsLock` toggle is a `ResetBoundary`).
- `KeystrokeBuffer` (bounded `VecDeque<BufferedKeystroke>`, `max_len: usize`, validated to `1..=512`, default `128`):
  - `push(stroke)`: if `len == max_len`, `pop_front()` then `push_back(stroke)`.
  - `pop_backspace()`: removes last entry (`pop_back()`).
  - `clear()`: empties buffer.
- **Invariants**:
  - Volatile RAM only — never written to disk or release logs.
  - Tracks `burst_layout: Option<LayoutId>` and `burst_focus: Option<FocusId>` so external layout changes or focus changes immediately invalidate the burst.

### 5. `LayoutId` & `LayoutPair`
- `LayoutId(String)` — opaque, case-normalized platform-neutral identifier (e.g., `"00000409"`, `"0000040D"`, `"en-US"`, `"he-IL"`). Windows-specific KLID/HKL resolution lives strictly in `langflip-platform-win`, not in `langflip-core`.
- `LayoutPair { primary: LayoutId, secondary: LayoutId }`:
  - Default: `primary = "00000409"` (US QWERTY), `secondary = "0000040D"` (Israeli Hebrew Standard).
  - `opposite_if_member(&self, current: &LayoutId) -> Option<&LayoutId>`:
    - If `current == primary` → `Some(&secondary)`.
    - If `current == secondary` → `Some(&primary)`.
    - Otherwise (3rd unrelated layout) → `None` (prevents destructive erasure of text that cannot round-trip).
  - `target_for_empty_toggle(&self, current: &LayoutId) -> &LayoutId`:
    - Returns `opposite_if_member(current).unwrap_or(&self.primary)`.

### 6. `ConversionTicket` & `ConversionAction`
- `ConversionTicket { focus_id: FocusId, seq: u64, held_modifiers: Modifiers }` — snapshot captured at hotkey press.
- `ConversionAction`:
  - `None`
  - `SwitchLayoutOnly { target: LayoutId, ticket: ConversionTicket }`
  - `Convert { target_layout: LayoutId, erase_count: usize, replay: Vec<BufferedKeystroke>, ticket: ConversionTicket }`

### 7. `HotkeySpec` & `AppConfig` (`langflip.toml`)
```toml
# Optional configuration file placed next to langflip.exe or in OS config dir.
# All fields are optional; defaults shown below.

[layouts]
# Supports exact OS layout IDs (recommended when multiple variants share a language)
# or BCP-47 tags (e.g., "en-US", "he-IL", "ru-RU", "ar-SA").
primary = "00000409"   # US QWERTY
secondary = "0000040D" # Hebrew (Israeli Standard)

[hotkey]
key = "Pause"          # e.g., "Pause", "F9", "ScrollLock", "L"
ctrl = false
alt = false
shift = false
meta = false

[behavior]
max_buffer_len = 128          # Allowed range: 1..=512
replay_delay_ms = 0           # Per-keystroke delay in ms (0..=10; default 0 for <150ms speed)
toggle_on_empty_buffer = true
```

## State Transitions (Naive MVP)

| Current State | Incoming Event | Resulting State | Output Action |
|---|---|---|---|
| Any | `KeyDown` with `is_injected == true` | Unchanged | `None` |
| Any | `QueueOverflow` | Clear buffer | `None` |
| Any | Active layout changed externally since last stroke | Clear buffer before handling event | `None` |
| Any | `KeyDown(Printable)` without `Ctrl`/`Alt`/`Meta` | Push stroke (bounded) | `None` |
| Any | `KeyDown(Printable)` with `Ctrl`/`Alt`/`Meta` (incl. AltGr) | Clear buffer | `None` |
| Any | `KeyDown(Backspace)` | Pop last stroke if non-empty | `None` |
| Any | `KeyDown(ResetBoundary)` (incl. `CapsLock`) | Clear buffer | `None` |
| Any | `MouseClick` or `FocusChanged` | Clear buffer | `None` |
| Hotkey held | Repeated `HotkeyTriggered` (auto-repeat) | Unchanged | `None` |
| Empty buffer | `HotkeyTriggered` | Empty buffer | `SwitchLayoutOnly` (if enabled) else `None` |
| Non-empty, 3rd layout | `HotkeyTriggered` | Clear buffer (no destructive erase) | `None` |
| Non-empty, pair member | `HotkeyTriggered` | Execute `Switch -> Confirm -> CheckFreshness -> Erase -> Replay`; retain buffer ONLY on full success (else clear) | `Convert` |
