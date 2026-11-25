# Data Model & State Machine: LangFlip MVP

**Feature**: `001-mvp-layout-converter` | **Date**: 2026-11-25 (Revision 3 after Multi-Agent Design Review)

## Core Domain Types (`langflip-core`)

### 1. `PhysicalKey` & `KeyName`
- `PhysicalKey { scan_code: u16, extended: bool }` — hardware scan code (AT Set 1 on Windows; mapped from OS keycodes on future platforms).
- `KeyName` — canonical config key names (`Pause`, `F1`–`F12`, `ScrollLock`, `A`–`Z`, etc.) mapped deterministically to `PhysicalKey` in `langflip-core::key` so hotkey matching is physical-key-based (works identically across QWERTY, AZERTY, Hebrew, Cyrillic).
  - Note: `Pause` vs. `NumLock` (both scan code `0x45`) are disambiguated by the extended/E1 flag in the OS hook before constructing `PhysicalKey`.

### 2. `Modifiers` (Left/Right Aware + Lock States)
```rust
pub struct Modifiers {
    pub left_shift: bool,
    pub right_shift: bool,
    pub left_ctrl: bool,
    pub right_ctrl: bool,
    pub left_alt: bool,
    pub right_alt: bool,
    pub left_meta: bool,
    pub right_meta: bool,
    pub caps_lock: bool,
    pub num_lock: bool,
}
```
Helper methods: `any_shift()`, `any_ctrl()`, `any_alt()`, `any_meta()`, `has_shortcut_modifier()` (`any_ctrl() || any_alt() || any_meta()`).

### 3. `InjectedBy`, `KeyKind`, `KeyEvent`, and `InputEvent`
- `InjectedBy`: `Physical | SelfMarker | External`
- `KeyKind` (produced by pure `classify_key(key: PhysicalKey, modifiers: Modifiers) -> KeyKind`):
  - `Printable` — straight-line visible character (letters, digits, punctuation, Space, and numpad digits when `num_lock == true`).
  - `Backspace` — deletes one prior visible character.
  - `ResetBoundary` — `Enter`, `Tab`, `Escape`, `Insert`, arrows (`Left`/`Right`/`Up`/`Down`), `Home`, `End`, `PageUp`, `PageDown`, `Delete`, `CapsLock`, and numpad navigation keys when `num_lock == false`.
  - `Modifier` — `Left/Right Shift`, `Ctrl`, `Alt`, `Meta`.
  - `Other` — function keys, media keys, etc. (ignored unless bound as hotkey).
- `KeyEvent`: `{ key: PhysicalKey, modifiers: Modifiers, injected: InjectedBy, active_layout: LayoutId, focus_id: FocusId, seq: u64 }`
- `InputEvent`:
  - `KeyDown(KeyEvent)`
  - `KeyUp(KeyEvent)`
  - `MouseClick { focus_id: FocusId, seq: u64 }`
  - `FocusChanged { focus: FocusContext, seq: u64 }`
  - `QueueOverflow { seq: u64 }`
- **Mutating sequence (`seq`) rule**: Only non-injected non-hotkey `KeyDown`, `MouseClick`, `FocusChanged`, and `QueueOverflow` advance `latest_seq`. Releasing the trigger hotkey or its combo modifiers (`KeyUp`) does **NOT** advance `latest_seq`.

### 4. `BufferedKeystroke` & `KeystrokeBuffer` (Privacy-Redacted `Debug`)
- `BufferedKeystroke`: `{ key: PhysicalKey, shift: bool }` (custom `Debug` prints `"BufferedKeystroke(<redacted>)"`).
- `KeystrokeBuffer` (bounded `VecDeque<BufferedKeystroke>`, `max_len: usize`, validated `1..=512`, default `128`; custom `Debug` prints `"KeystrokeBuffer { len, max_len }"` without contents):
  - `push(stroke)`: if `len == max_len`, `pop_front()` then `push_back(stroke)` (overflow drops oldest; conversion safely converts the retained tail).
  - `pop_backspace()`: removes last entry (`pop_back()`).
  - `clear()`: empties buffer.
- Tracks `burst_layout: Option<LayoutId>` and `burst_focus: Option<FocusId>`.

### 5. `LayoutId`, `InstalledLayout`, `LayoutPair`
- `LayoutId(String)` — opaque, case-normalized platform-neutral identifier (e.g., `"en-US"`, `"he-IL"`, `"00000409"`, `"0000040D"`, `"0002040D"`, `"0003040D"`).
- `InstalledLayout { id: LayoutId, language_tag: Option<String>, display_name: String }` — populated by the OS adapter and passed into `langflip-core::layout::resolve_layouts()` for pure, testable resolution and ambiguity detection.
- `LayoutPair { primary: LayoutId, secondary: LayoutId }`:
  - Default: `primary = "en-US"`, `secondary = "he-IL"`.
  - `opposite_if_member(&self, current: &LayoutId) -> Option<&LayoutId>` (`None` for 3rd unrelated layout).
  - `target_for_empty_toggle(&self, current: &LayoutId) -> &LayoutId`.

### 6. `InjectionStep` & `ConversionPlan`
Produced by pure `langflip-core::planner::plan_injection(...)`:
```rust
pub enum InjectionStep {
    MaskKey,
    ReleaseModifier(PhysicalKey), // exact left/right modifier that was physically held
    KeyTap { key: PhysicalKey, shift: bool },
    DelayMs(u64),
}
```

### 7. `HotkeySpec`, `HotkeyMatcher`, & `AppConfig` (`langflip.toml`)
- `HotkeyMatcher` (single source of truth in `langflip-core::hotkey`): tracks `hotkey_held: bool`, matches `KeyDown`/`KeyUp` by `PhysicalKey` + required modifiers, suppresses auto-repeat while held, and emits `HotkeyDecision::Trigger | SwallowRepeat | PassThrough`.
- Config lookup order: (1) `<exe_dir>/langflip.toml`, (2) `<os_config_dir>/langflip/langflip.toml`, (3) built-in defaults. Invalid syntax or out-of-range values fail fast at startup.

```toml
# Optional configuration file (langflip.toml).
# All fields are optional; defaults shown below.

[layouts]
# Accepts primary language tags ("en", "he", "ru"), BCP-47 tags ("en-US", "en-IN",
# "he-IL"), or exact OS layout IDs (e.g., Windows KLIDs: "00000409" US, "00004009"
# India, "0000040D" Hebrew, "0002040D" Hebrew Standard, "0003040D" Hebrew Standard 2018).
primary = "en"
secondary = "he"

[hotkey]
key = "Pause"          # Physical key name: "Pause", "F9", "ScrollLock", "L", etc.
ctrl = false
alt = false
shift = false
meta = false

[behavior]
max_buffer_len = 128          # Valid range: 1..=512 (rejected if out of range)
replay_delay_ms = 1           # Valid range: 0..=10 ms per keystroke
toggle_on_empty_buffer = true
```

## State Transitions (Naive MVP)

| Current State | Incoming Event | Resulting State | Action |
|---|---|---|---|
| Any | `KeyDown`/`KeyUp` with `InjectedBy::SelfMarker` | Unchanged | None (ignore) |
| Any | `KeyDown` with `InjectedBy::External` | Clear buffer | None |
| Any | `QueueOverflow` | Clear buffer | None |
| Any | `KeyDown` whose `active_layout != burst_layout` | Clear buffer before processing key | None |
| Any | `KeyDown(Printable)` without shortcut modifiers | Push stroke (tail-bounded) | None |
| Any | `KeyDown(Printable)` with `Ctrl`/`Alt`/`Meta` (incl. AltGr) | Clear buffer | None |
| Any | `KeyDown(Backspace)` | Pop last stroke if non-empty | None |
| Any | `KeyDown(ResetBoundary)` (`Enter`, `Tab`, `Esc`, `Insert`, nav, `CapsLock`, numpad w/o NumLock) | Clear buffer | None |
| Any | `MouseClick` or `FocusChanged` | Clear buffer | None |
| Hotkey held | Repeated hotkey `KeyDown` | Unchanged | Swallow repeat |
| Empty buffer | Trigger hotkey `KeyDown` | Empty buffer | Switch layout (if enabled) |
| Non-empty, 3rd layout | Trigger hotkey `KeyDown` | Clear buffer | None (no destructive erase) |
| Non-empty, pair member | Trigger hotkey `KeyDown` | `Check Freshness -> Switch & Confirm -> Re-check Freshness -> Plan & Execute Injection -> Retain Buffer on Full Success` (on cancel after switch: best-effort restore layout; on any failure: clear buffer) | Execute `ConversionPlan` |
