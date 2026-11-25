//! Pure injection-sequence planner (strictly TDD-tested on Linux).
//!
//! Converts `(held_modifiers, erase_count, strokes, replay_delay_ms)` into a
//! deterministic `Vec<InjectionStep>` executed verbatim by the thin OS injector.

use crate::key::{BufferedKeystroke, Modifiers, scan};
use crate::ports::InjectionStep;

/// Plans the exact low-level synthetic injection sequence:
/// 1. If any combo modifiers are physically held, emits `MaskKey` followed by
///    `ReleaseModifier(exact_left_or_right_key)` for each held modifier.
/// 2. Emits `erase_count` Backspace taps (with optional per-keystroke `DelayMs`).
/// 3. Emits each replayed keystroke tap (with optional per-keystroke `DelayMs`).
pub fn plan_injection(
    held_modifiers: Modifiers,
    erase_count: usize,
    strokes: &[BufferedKeystroke],
    replay_delay_ms: u64,
) -> Vec<InjectionStep> {
    let held = held_modifiers.held_modifier_keys();
    let total_taps = erase_count + strokes.len();
    let mut steps = Vec::with_capacity(held.len() + 1 + total_taps * 2);

    if !held.is_empty() {
        steps.push(InjectionStep::MaskKey);
        for key in held {
            steps.push(InjectionStep::ReleaseModifier(key));
        }
    }

    let mut emitted_taps = 0usize;
    for _ in 0..erase_count {
        if replay_delay_ms > 0 && emitted_taps > 0 {
            steps.push(InjectionStep::DelayMs(replay_delay_ms));
        }
        steps.push(InjectionStep::KeyTap {
            key: scan::BACKSPACE,
            shift: false,
        });
        emitted_taps += 1;
    }

    for stroke in strokes {
        if replay_delay_ms > 0 && emitted_taps > 0 {
            steps.push(InjectionStep::DelayMs(replay_delay_ms));
        }
        steps.push(InjectionStep::KeyTap {
            key: stroke.key,
            shift: stroke.shift,
        });
        emitted_taps += 1;
    }

    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::{PhysicalKey, scan};

    #[test]
    fn single_key_hotkey_omits_mask_and_modifier_releases() {
        let strokes = vec![
            BufferedKeystroke {
                key: PhysicalKey::new(0x23, false),
                shift: true,
            },
            BufferedKeystroke {
                key: PhysicalKey::new(0x12, false),
                shift: false,
            },
        ];

        let steps = plan_injection(Modifiers::default(), 2, &strokes, 0);
        assert_eq!(
            steps,
            vec![
                InjectionStep::KeyTap {
                    key: scan::BACKSPACE,
                    shift: false,
                },
                InjectionStep::KeyTap {
                    key: scan::BACKSPACE,
                    shift: false,
                },
                InjectionStep::KeyTap {
                    key: PhysicalKey::new(0x23, false),
                    shift: true,
                },
                InjectionStep::KeyTap {
                    key: PhysicalKey::new(0x12, false),
                    shift: false,
                },
            ]
        );
    }

    #[test]
    fn combo_hotkey_emits_mask_key_and_exact_left_right_modifier_releases() {
        let held = Modifiers {
            right_ctrl: true,
            left_shift: true,
            ..Default::default()
        };
        let strokes = vec![BufferedKeystroke {
            key: PhysicalKey::new(0x23, false),
            shift: false,
        }];

        let steps = plan_injection(held, 1, &strokes, 0);
        assert_eq!(
            steps,
            vec![
                InjectionStep::MaskKey,
                InjectionStep::ReleaseModifier(scan::LEFT_SHIFT),
                InjectionStep::ReleaseModifier(scan::RIGHT_CTRL),
                InjectionStep::KeyTap {
                    key: scan::BACKSPACE,
                    shift: false,
                },
                InjectionStep::KeyTap {
                    key: PhysicalKey::new(0x23, false),
                    shift: false,
                },
            ]
        );
    }

    #[test]
    fn non_zero_replay_delay_inserts_per_keystroke_delay_between_taps() {
        let strokes = vec![BufferedKeystroke {
            key: PhysicalKey::new(0x23, false),
            shift: false,
        }];
        let steps = plan_injection(Modifiers::default(), 1, &strokes, 2);
        assert_eq!(
            steps,
            vec![
                InjectionStep::KeyTap {
                    key: scan::BACKSPACE,
                    shift: false,
                },
                InjectionStep::DelayMs(2),
                InjectionStep::KeyTap {
                    key: PhysicalKey::new(0x23, false),
                    shift: false,
                },
            ]
        );
    }
}
