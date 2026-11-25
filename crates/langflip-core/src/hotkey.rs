//! Physical-key hotkey specification and single-source-of-truth matcher
//! with auto-repeat suppression.

use crate::key::{Modifiers, PhysicalKey, scan};

/// Configured trigger hotkey matched by physical key + modifier flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeySpec {
    pub key: PhysicalKey,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl Default for HotkeySpec {
    fn default() -> Self {
        Self {
            key: scan::PAUSE,
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
        }
    }
}

/// Decision returned by `HotkeyMatcher` for a key press/release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyDecision {
    /// Fresh trigger press (not auto-repeat) — swallow and start conversion.
    Trigger,
    /// Auto-repeat press while trigger is still held — swallow without re-triggering.
    SwallowRepeat,
    /// Not the trigger hotkey — pass through normally.
    PassThrough,
}

/// Stateful matcher used by both the OS hook and core tests (DRY single source of truth).
#[derive(Debug, Clone)]
pub struct HotkeyMatcher {
    spec: HotkeySpec,
    hotkey_held: bool,
}

impl HotkeyMatcher {
    pub fn new(spec: HotkeySpec) -> Self {
        Self {
            spec,
            hotkey_held: false,
        }
    }

    pub fn spec(&self) -> HotkeySpec {
        self.spec
    }

    pub fn on_key_down(&mut self, key: PhysicalKey, modifiers: Modifiers) -> HotkeyDecision {
        let matches = key == self.spec.key
            && modifiers.any_ctrl() == self.spec.ctrl
            && modifiers.any_alt() == self.spec.alt
            && modifiers.any_shift() == self.spec.shift
            && modifiers.any_meta() == self.spec.meta;

        if !matches {
            return HotkeyDecision::PassThrough;
        }

        if self.hotkey_held {
            HotkeyDecision::SwallowRepeat
        } else {
            self.hotkey_held = true;
            HotkeyDecision::Trigger
        }
    }

    pub fn on_key_up(&mut self, key: PhysicalKey) {
        if key == self.spec.key {
            self.hotkey_held = false;
        }
    }

    pub fn is_held(&self) -> bool {
        self.hotkey_held
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pause_hotkey_triggers_once_and_suppresses_auto_repeat() {
        let mut matcher = HotkeyMatcher::new(HotkeySpec::default());
        let mods = Modifiers::default();

        assert_eq!(matcher.on_key_down(scan::PAUSE, mods), HotkeyDecision::Trigger);
        assert!(matcher.is_held());
        // Auto-repeat while held must be swallowed without re-triggering.
        assert_eq!(matcher.on_key_down(scan::PAUSE, mods), HotkeyDecision::SwallowRepeat);
        // Releasing re-arms the matcher.
        matcher.on_key_up(scan::PAUSE);
        assert!(!matcher.is_held());
        assert_eq!(matcher.on_key_down(scan::PAUSE, mods), HotkeyDecision::Trigger);
    }

    #[test]
    fn pause_does_not_match_numlock_or_modified_pause() {
        let mut matcher = HotkeyMatcher::new(HotkeySpec::default());
        assert_eq!(
            matcher.on_key_down(scan::NUM_LOCK, Modifiers::default()),
            HotkeyDecision::PassThrough
        );
        let with_ctrl = Modifiers { left_ctrl: true, ..Default::default() };
        assert_eq!(
            matcher.on_key_down(scan::PAUSE, with_ctrl),
            HotkeyDecision::PassThrough
        );
    }

    #[test]
    fn combo_hotkey_matches_left_or_right_modifiers() {
        let key_l = PhysicalKey::new(0x26, false);
        let mut matcher = HotkeyMatcher::new(HotkeySpec {
            key: key_l,
            ctrl: true,
            shift: true,
            alt: false,
            meta: false,
        });

        let right_combo = Modifiers {
            right_ctrl: true,
            right_shift: true,
            ..Default::default()
        };
        assert_eq!(matcher.on_key_down(key_l, right_combo), HotkeyDecision::Trigger);
        matcher.on_key_up(key_l);

        // Missing shift -> pass through
        let only_ctrl = Modifiers {
            left_ctrl: true,
            ..Default::default()
        };
        assert_eq!(matcher.on_key_down(key_l, only_ctrl), HotkeyDecision::PassThrough);

        // Extra alt -> pass through
        let extra_alt = Modifiers {
            left_ctrl: true,
            left_shift: true,
            left_alt: true,
            ..Default::default()
        };
        assert_eq!(matcher.on_key_down(key_l, extra_alt), HotkeyDecision::PassThrough);
    }
}
