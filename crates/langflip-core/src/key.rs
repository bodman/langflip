//! Platform-neutral physical key types, left/right modifiers, key classification,
//! and privacy-redacted buffered keystrokes.

use std::fmt;

use crate::layout::LayoutId;
use crate::ports::{FocusContext, FocusId};

/// Hardware physical key (AT Set 1 scan code + extended flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicalKey {
    pub scan_code: u16,
    pub extended: bool,
}

impl PhysicalKey {
    pub const fn new(scan_code: u16, extended: bool) -> Self {
        Self {
            scan_code,
            extended,
        }
    }
}

/// Standard AT Set 1 scan codes used across `langflip-core` (DRY single source of truth).
pub mod scan {
    use super::PhysicalKey;

    pub const ESCAPE: PhysicalKey = PhysicalKey::new(0x01, false);
    pub const BACKSPACE: PhysicalKey = PhysicalKey::new(0x0E, false);
    pub const TAB: PhysicalKey = PhysicalKey::new(0x0F, false);
    pub const ENTER: PhysicalKey = PhysicalKey::new(0x1C, false);
    pub const NUMPAD_ENTER: PhysicalKey = PhysicalKey::new(0x1C, true);
    pub const LEFT_CTRL: PhysicalKey = PhysicalKey::new(0x1D, false);
    pub const RIGHT_CTRL: PhysicalKey = PhysicalKey::new(0x1D, true);
    pub const LEFT_SHIFT: PhysicalKey = PhysicalKey::new(0x2A, false);
    pub const RIGHT_SHIFT: PhysicalKey = PhysicalKey::new(0x36, false);
    pub const LEFT_ALT: PhysicalKey = PhysicalKey::new(0x38, false);
    pub const RIGHT_ALT: PhysicalKey = PhysicalKey::new(0x38, true);
    pub const CAPS_LOCK: PhysicalKey = PhysicalKey::new(0x3A, false);
    pub const PAUSE: PhysicalKey = PhysicalKey::new(0x45, false);
    pub const NUM_LOCK: PhysicalKey = PhysicalKey::new(0x45, true);
    pub const SCROLL_LOCK: PhysicalKey = PhysicalKey::new(0x46, false);
    pub const LEFT_META: PhysicalKey = PhysicalKey::new(0x5B, true);
    pub const RIGHT_META: PhysicalKey = PhysicalKey::new(0x5C, true);
    pub const INSERT: PhysicalKey = PhysicalKey::new(0x52, true);
    pub const DELETE: PhysicalKey = PhysicalKey::new(0x53, true);
    pub const HOME: PhysicalKey = PhysicalKey::new(0x47, true);
    pub const END: PhysicalKey = PhysicalKey::new(0x4F, true);
    pub const PAGE_UP: PhysicalKey = PhysicalKey::new(0x49, true);
    pub const PAGE_DOWN: PhysicalKey = PhysicalKey::new(0x51, true);
    pub const UP: PhysicalKey = PhysicalKey::new(0x48, true);
    pub const DOWN: PhysicalKey = PhysicalKey::new(0x50, true);
    pub const LEFT: PhysicalKey = PhysicalKey::new(0x4B, true);
    pub const RIGHT: PhysicalKey = PhysicalKey::new(0x4D, true);
    pub const SPACE: PhysicalKey = PhysicalKey::new(0x39, false);
}

/// Active left/right modifier and lock key state.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
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

impl Modifiers {
    pub fn any_shift(&self) -> bool {
        self.left_shift || self.right_shift
    }

    pub fn any_ctrl(&self) -> bool {
        self.left_ctrl || self.right_ctrl
    }

    pub fn any_alt(&self) -> bool {
        self.left_alt || self.right_alt
    }

    pub fn any_meta(&self) -> bool {
        self.left_meta || self.right_meta
    }

    pub fn has_shortcut_modifier(&self) -> bool {
        self.any_ctrl() || self.any_alt() || self.any_meta()
    }

    /// Returns the exact physically held modifier keys (left/right distinct).
    pub fn held_modifier_keys(&self) -> Vec<PhysicalKey> {
        let mut keys = Vec::new();
        if self.left_shift {
            keys.push(scan::LEFT_SHIFT);
        }
        if self.right_shift {
            keys.push(scan::RIGHT_SHIFT);
        }
        if self.left_ctrl {
            keys.push(scan::LEFT_CTRL);
        }
        if self.right_ctrl {
            keys.push(scan::RIGHT_CTRL);
        }
        if self.left_alt {
            keys.push(scan::LEFT_ALT);
        }
        if self.right_alt {
            keys.push(scan::RIGHT_ALT);
        }
        if self.left_meta {
            keys.push(scan::LEFT_META);
        }
        if self.right_meta {
            keys.push(scan::RIGHT_META);
        }
        keys
    }
}

/// Origin of a key event (physical hardware, LangFlip self-injection, or external synthetic tool).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectedBy {
    Physical,
    SelfMarker,
    External,
}

/// Classification of a physical key for buffer/reset rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    Printable,
    Backspace,
    ResetBoundary,
    Modifier,
    Other,
}

/// Pure classifier mapping `(PhysicalKey, Modifiers)` to `KeyKind`.
pub fn classify_key(key: PhysicalKey, modifiers: Modifiers) -> KeyKind {
    if key == scan::BACKSPACE {
        return KeyKind::Backspace;
    }

    if matches!(
        key,
        scan::LEFT_SHIFT
            | scan::RIGHT_SHIFT
            | scan::LEFT_CTRL
            | scan::RIGHT_CTRL
            | scan::LEFT_ALT
            | scan::RIGHT_ALT
            | scan::LEFT_META
            | scan::RIGHT_META
    ) {
        return KeyKind::Modifier;
    }

    if matches!(
        key,
        scan::ENTER
            | scan::NUMPAD_ENTER
            | scan::TAB
            | scan::ESCAPE
            | scan::INSERT
            | scan::DELETE
            | scan::CAPS_LOCK
            | scan::HOME
            | scan::END
            | scan::PAGE_UP
            | scan::PAGE_DOWN
            | scan::UP
            | scan::DOWN
            | scan::LEFT
            | scan::RIGHT
    ) {
        return KeyKind::ResetBoundary;
    }

    // Numpad navigation/digit block (0x47..=0x53, extended=false, excluding '-' 0x4A and '+' 0x4E)
    if !key.extended && matches!(key.scan_code, 0x47..=0x49 | 0x4B..=0x4D | 0x4F..=0x53) {
        return if modifiers.num_lock {
            KeyKind::Printable
        } else {
            KeyKind::ResetBoundary
        };
    }

    // Standard printable AT Set 1 scan codes:
    // - Number row + symbols: 0x02..=0x0D
    // - Top letter row + brackets: 0x10..=0x1B
    // - Home row + quotes + grave: 0x1E..=0x29
    // - Backslash + bottom row + slash: 0x2B..=0x35
    // - Numpad '*', '-', '+': 0x37, 0x4A, 0x4E; Numpad '/' (0x35, ext=true)
    // - Space: 0x39; ISO extra backslash key: 0x56
    if !key.extended
        && matches!(
            key.scan_code,
            0x02..=0x0D
                | 0x10..=0x1B
                | 0x1E..=0x29
                | 0x2B..=0x35
                | 0x37
                | 0x39
                | 0x4A
                | 0x4E
                | 0x56
        )
    {
        return KeyKind::Printable;
    }
    if key.extended && key.scan_code == 0x35 {
        return KeyKind::Printable;
    }

    KeyKind::Other
}

/// Single DRY table of canonical hotkey names and their hardware `PhysicalKey`s.
const KEY_NAME_TABLE: &[(&str, PhysicalKey)] = &[
    ("Pause", scan::PAUSE),
    ("ScrollLock", scan::SCROLL_LOCK),
    ("Space", scan::SPACE),
    ("F1", PhysicalKey::new(0x3B, false)),
    ("F2", PhysicalKey::new(0x3C, false)),
    ("F3", PhysicalKey::new(0x3D, false)),
    ("F4", PhysicalKey::new(0x3E, false)),
    ("F5", PhysicalKey::new(0x3F, false)),
    ("F6", PhysicalKey::new(0x40, false)),
    ("F7", PhysicalKey::new(0x41, false)),
    ("F8", PhysicalKey::new(0x42, false)),
    ("F9", PhysicalKey::new(0x43, false)),
    ("F10", PhysicalKey::new(0x44, false)),
    ("F11", PhysicalKey::new(0x57, false)),
    ("F12", PhysicalKey::new(0x58, false)),
    ("A", PhysicalKey::new(0x1E, false)),
    ("B", PhysicalKey::new(0x30, false)),
    ("C", PhysicalKey::new(0x2E, false)),
    ("D", PhysicalKey::new(0x20, false)),
    ("E", PhysicalKey::new(0x12, false)),
    ("F", PhysicalKey::new(0x21, false)),
    ("G", PhysicalKey::new(0x22, false)),
    ("H", PhysicalKey::new(0x23, false)),
    ("I", PhysicalKey::new(0x17, false)),
    ("J", PhysicalKey::new(0x24, false)),
    ("K", PhysicalKey::new(0x25, false)),
    ("L", PhysicalKey::new(0x26, false)),
    ("M", PhysicalKey::new(0x32, false)),
    ("N", PhysicalKey::new(0x31, false)),
    ("O", PhysicalKey::new(0x18, false)),
    ("P", PhysicalKey::new(0x19, false)),
    ("Q", PhysicalKey::new(0x10, false)),
    ("R", PhysicalKey::new(0x13, false)),
    ("S", PhysicalKey::new(0x1F, false)),
    ("T", PhysicalKey::new(0x14, false)),
    ("U", PhysicalKey::new(0x16, false)),
    ("V", PhysicalKey::new(0x2F, false)),
    ("W", PhysicalKey::new(0x11, false)),
    ("X", PhysicalKey::new(0x2D, false)),
    ("Y", PhysicalKey::new(0x15, false)),
    ("Z", PhysicalKey::new(0x2C, false)),
];

/// Canonical physical key names exposed in config files and the Settings UI.
pub const SUPPORTED_HOTKEY_NAMES: &[&str] = &[
    "Pause",
    "ScrollLock",
    "Space",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
];

/// Reverse-maps a hardware `PhysicalKey` back to its canonical config key name.
pub fn key_name_for(key: PhysicalKey) -> Option<&'static str> {
    KEY_NAME_TABLE
        .iter()
        .find(|(_, k)| *k == key)
        .map(|(name, _)| *name)
}

/// Maps a canonical config key name (case-insensitive, e.g., `"Pause"`, `"F9"`, `"L"`)
/// to its hardware `PhysicalKey`.
pub fn parse_key_name(name: &str) -> Option<PhysicalKey> {
    let lower = name.trim().to_ascii_lowercase();
    match lower.as_str() {
        "pausebreak" | "break" => return Some(scan::PAUSE),
        "scroll" => return Some(scan::SCROLL_LOCK),
        _ => {}
    }
    KEY_NAME_TABLE
        .iter()
        .find(|(canonical, _)| canonical.eq_ignore_ascii_case(&lower))
        .map(|(_, k)| *k)
}

/// Normalized key press/release event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: PhysicalKey,
    pub modifiers: Modifiers,
    pub injected: InjectedBy,
    pub active_layout: LayoutId,
    pub focus_id: FocusId,
    pub seq: u64,
}

/// Normalized input events delivered from `InputListener` to `ConversionEngine`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    KeyDown(KeyEvent),
    KeyUp(KeyEvent),
    MouseClick { focus_id: FocusId, seq: u64 },
    FocusChanged { focus: FocusContext, seq: u64 },
    QueueOverflow { seq: u64 },
}

/// A single buffered printable keystroke.
///
/// Privacy invariant (Constitution Principle V): `Debug` MUST redact key contents
/// so panic messages or debug formatting never leak typed text.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BufferedKeystroke {
    pub key: PhysicalKey,
    pub shift: bool,
}

impl fmt::Debug for BufferedKeystroke {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BufferedKeystroke(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_helpers_detect_left_and_right() {
        let mut m = Modifiers::default();
        assert!(!m.any_shift());
        assert!(!m.has_shortcut_modifier());
        assert!(m.held_modifier_keys().is_empty());

        m.right_ctrl = true;
        m.left_shift = true;
        assert!(m.any_ctrl());
        assert!(m.any_shift());
        assert!(m.has_shortcut_modifier());
        assert_eq!(m.held_modifier_keys(), vec![scan::LEFT_SHIFT, scan::RIGHT_CTRL]);
    }

    #[test]
    fn parse_key_name_resolves_standard_keys_case_insensitively() {
        assert_eq!(parse_key_name("Pause"), Some(scan::PAUSE));
        assert_eq!(parse_key_name("pause"), Some(scan::PAUSE));
        assert_eq!(parse_key_name("ScrollLock"), Some(scan::SCROLL_LOCK));
        assert_eq!(parse_key_name("F9"), Some(PhysicalKey::new(0x43, false)));
        assert_eq!(parse_key_name("L"), Some(PhysicalKey::new(0x26, false)));
        assert_eq!(parse_key_name("l"), Some(PhysicalKey::new(0x26, false)));
        assert_eq!(parse_key_name("NonExistentKey"), None);
    }

    #[test]
    fn supported_hotkey_names_round_trip_with_parse_and_key_name_for() {
        assert!(!SUPPORTED_HOTKEY_NAMES.is_empty());
        assert!(SUPPORTED_HOTKEY_NAMES.contains(&"Pause"));
        assert!(SUPPORTED_HOTKEY_NAMES.contains(&"F9"));
        assert!(SUPPORTED_HOTKEY_NAMES.contains(&"L"));
        for &name in SUPPORTED_HOTKEY_NAMES {
            let key = parse_key_name(name).unwrap_or_else(|| panic!("missing key for {name}"));
            assert_eq!(key_name_for(key), Some(name));
        }
    }

    #[test]
    fn classify_key_handles_printable_backspace_modifiers_and_resets() {
        let default_mods = Modifiers::default();
        // Letter 'h' (0x23) and Space (0x39)
        assert_eq!(classify_key(PhysicalKey::new(0x23, false), default_mods), KeyKind::Printable);
        assert_eq!(classify_key(scan::SPACE, default_mods), KeyKind::Printable);
        assert_eq!(classify_key(scan::BACKSPACE, default_mods), KeyKind::Backspace);
        assert_eq!(classify_key(scan::LEFT_SHIFT, default_mods), KeyKind::Modifier);
        assert_eq!(classify_key(scan::RIGHT_CTRL, default_mods), KeyKind::Modifier);

        // Reset boundaries including Enter, Tab, Escape, Insert, CapsLock, nav keys
        for k in [
            scan::ENTER,
            scan::NUMPAD_ENTER,
            scan::TAB,
            scan::ESCAPE,
            scan::INSERT,
            scan::DELETE,
            scan::CAPS_LOCK,
            scan::HOME,
            scan::END,
            scan::PAGE_UP,
            scan::PAGE_DOWN,
            scan::UP,
            scan::DOWN,
            scan::LEFT,
            scan::RIGHT,
        ] {
            assert_eq!(classify_key(k, default_mods), KeyKind::ResetBoundary, "expected ResetBoundary for {k:?}");
        }
    }

    #[test]
    fn classify_key_distinguishes_numpad_with_numlock_on_vs_off() {
        // Numpad 4 / Left (0x4B, extended=false)
        let numpad_4 = PhysicalKey::new(0x4B, false);
        let num_on = Modifiers { num_lock: true, ..Default::default() };
        let num_off = Modifiers { num_lock: false, ..Default::default() };
        assert_eq!(classify_key(numpad_4, num_on), KeyKind::Printable);
        assert_eq!(classify_key(numpad_4, num_off), KeyKind::ResetBoundary);
    }

    #[test]
    fn buffered_keystroke_debug_redacts_scan_code() {
        let stroke = BufferedKeystroke {
            key: PhysicalKey::new(0x23, false),
            shift: true,
        };
        let formatted = format!("{stroke:?}");
        assert!(!formatted.contains("35"), "Debug leaked decimal scan code: {formatted}");
        assert!(!formatted.contains("23"), "Debug leaked hex scan code: {formatted}");
        assert!(formatted.contains("redacted"), "Debug should indicate redaction: {formatted}");
    }
}
