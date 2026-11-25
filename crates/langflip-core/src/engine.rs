//! Core conversion engine orchestrating safe `Check Layout -> Check Freshness
//! -> Switch & Confirm -> Re-check Freshness -> Plan & Execute Injection
//! -> Retain Buffer on Full Success`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::buffer::{KeystrokeBuffer, NaiveBurstPolicy};
use crate::config::AppConfig;
use crate::hotkey::{HotkeyDecision, HotkeyMatcher};
use crate::key::{
    BufferedKeystroke, InjectedBy, InputEvent, KeyEvent, KeyKind, classify_key,
};
use crate::layout::LayoutId;
use crate::planner::plan_injection;
use crate::ports::{
    FocusId, FocusObserver, InjectError, InputInjector, LayoutController, LayoutSwitchOutcome,
};

pub const DEFAULT_SWITCH_CONFIRM_TIMEOUT_MS: u64 = 100;

/// Outcome of processing a single `InputEvent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineOutcome {
    Ignored,
    Buffered,
    BufferReset,
    LayoutToggled(LayoutId),
    Converted {
        erased: usize,
        replayed: usize,
        new_layout: LayoutId,
    },
    AbortedSwitchRejected,
    AbortedStale,
    AbortedThirdLayout,
    FailedInjection(InjectError),
    LayoutError(String),
}

/// Platform-independent LangFlip conversion engine.
pub struct ConversionEngine {
    config: AppConfig,
    matcher: HotkeyMatcher,
    policy: NaiveBurstPolicy,
    overflow_flag: Arc<AtomicBool>,
    latest_seq: Arc<AtomicU64>,
    confirm_timeout_ms: u64,
}

impl ConversionEngine {
    pub fn new(
        config: AppConfig,
        overflow_flag: Arc<AtomicBool>,
        latest_seq: Arc<AtomicU64>,
    ) -> Self {
        let matcher = HotkeyMatcher::new(config.hotkey);
        let policy = NaiveBurstPolicy::new(config.max_buffer_len);
        Self {
            config,
            matcher,
            policy,
            overflow_flag,
            latest_seq,
            confirm_timeout_ms: DEFAULT_SWITCH_CONFIRM_TIMEOUT_MS,
        }
    }

    pub fn buffer(&self) -> &KeystrokeBuffer {
        self.policy.buffer()
    }

    pub fn handle_event<I, L, F>(
        &mut self,
        event: InputEvent,
        injector: &mut I,
        layouts: &mut L,
        focus: &F,
    ) -> EngineOutcome
    where
        I: InputInjector,
        L: LayoutController,
        F: FocusObserver,
    {
        // Check bounded-queue overflow flag before handling any event.
        if self.overflow_flag.swap(false, Ordering::SeqCst) {
            self.policy.buffer_mut().clear();
        }

        match event {
            InputEvent::QueueOverflow { .. }
            | InputEvent::MouseClick { .. }
            | InputEvent::FocusChanged { .. } => {
                self.policy.buffer_mut().clear();
                EngineOutcome::BufferReset
            }
            InputEvent::KeyUp(ev) => {
                if ev.injected == InjectedBy::SelfMarker {
                    return EngineOutcome::Ignored;
                }
                self.matcher.on_key_up(ev.key);
                EngineOutcome::Ignored
            }
            InputEvent::KeyDown(ev) => self.handle_key_down(ev, injector, layouts, focus),
        }
    }

    fn handle_key_down<I, L, F>(
        &mut self,
        ev: KeyEvent,
        injector: &mut I,
        layouts: &mut L,
        focus: &F,
    ) -> EngineOutcome
    where
        I: InputInjector,
        L: LayoutController,
        F: FocusObserver,
    {
        match ev.injected {
            InjectedBy::SelfMarker => return EngineOutcome::Ignored,
            InjectedBy::External => {
                self.policy.buffer_mut().clear();
                return EngineOutcome::BufferReset;
            }
            InjectedBy::Physical => {}
        }

        match self.matcher.on_key_down(ev.key, ev.modifiers) {
            HotkeyDecision::SwallowRepeat => return EngineOutcome::Ignored,
            HotkeyDecision::Trigger => return self.trigger_conversion(&ev, injector, layouts, focus),
            HotkeyDecision::PassThrough => {}
        }

        // If focus or active layout changed externally since the last buffered stroke,
        // invalidate the prior burst before recording the new keystroke.
        let layout_changed = self
            .policy
            .buffer()
            .burst_layout()
            .is_some_and(|prev| prev != &ev.active_layout);
        let focus_changed = self
            .policy
            .buffer()
            .burst_focus()
            .is_some_and(|prev| prev != ev.focus_id);
        if layout_changed || focus_changed {
            self.policy.buffer_mut().clear();
        }

        match classify_key(ev.key, ev.modifiers) {
            KeyKind::Modifier | KeyKind::Other => EngineOutcome::Ignored,
            KeyKind::ResetBoundary => {
                self.policy.buffer_mut().clear();
                EngineOutcome::BufferReset
            }
            KeyKind::Backspace => {
                if ev.modifiers.has_shortcut_modifier() {
                    self.policy.buffer_mut().clear();
                    return EngineOutcome::BufferReset;
                }
                self.policy.buffer_mut().pop_backspace();
                EngineOutcome::Buffered
            }
            KeyKind::Printable => {
                if ev.modifiers.has_shortcut_modifier() {
                    self.policy.buffer_mut().clear();
                    return EngineOutcome::BufferReset;
                }
                let stroke = BufferedKeystroke {
                    key: ev.key,
                    shift: ev.modifiers.any_shift(),
                };
                self.policy
                    .buffer_mut()
                    .push(stroke, ev.active_layout, ev.focus_id);
                EngineOutcome::Buffered
            }
        }
    }

    fn is_fresh<F: FocusObserver>(&self, trigger_focus: FocusId, trigger_seq: u64, focus: &F) -> bool {
        if self.overflow_flag.load(Ordering::SeqCst) {
            return false;
        }
        if self.latest_seq.load(Ordering::SeqCst) != trigger_seq {
            return false;
        }
        focus.current_focus().focus_id == trigger_focus
    }

    fn trigger_conversion<I, L, F>(
        &mut self,
        ev: &KeyEvent,
        injector: &mut I,
        layouts: &mut L,
        focus: &F,
    ) -> EngineOutcome
    where
        I: InputInjector,
        L: LayoutController,
        F: FocusObserver,
    {
        let current_layout = match layouts.active_layout() {
            Ok(id) => id,
            Err(e) => {
                self.policy.buffer_mut().clear();
                return EngineOutcome::LayoutError(e.to_string());
            }
        };

        // Empty buffer -> optionally toggle layout only (FR-007).
        if self.policy.buffer().is_empty() {
            if !self.config.toggle_on_empty_buffer {
                return EngineOutcome::Ignored;
            }
            let target = self
                .config
                .layouts
                .target_for_empty_toggle(&current_layout)
                .clone();
            return match layouts.switch_and_confirm(&target, self.confirm_timeout_ms) {
                Ok(LayoutSwitchOutcome::Confirmed(new_id)) => EngineOutcome::LayoutToggled(new_id),
                Ok(LayoutSwitchOutcome::RejectedOrTimedOut) => EngineOutcome::AbortedSwitchRejected,
                Err(e) => EngineOutcome::LayoutError(e.to_string()),
            };
        }

        // 1. Verify active layout belongs to configured pair (3rd-layout non-destructive safety).
        let Some(target_layout) = self
            .config
            .layouts
            .opposite_if_member(&current_layout)
            .cloned()
        else {
            self.policy.buffer_mut().clear();
            return EngineOutcome::AbortedThirdLayout;
        };

        // 2. Check freshness before layout switch.
        if !self.is_fresh(ev.focus_id, ev.seq, focus) {
            self.policy.buffer_mut().clear();
            return EngineOutcome::AbortedStale;
        }

        // 3. Switch & confirm BEFORE any erasure.
        let confirmed_layout =
            match layouts.switch_and_confirm(&target_layout, self.confirm_timeout_ms) {
                Ok(LayoutSwitchOutcome::Confirmed(new_id)) => new_id,
                Ok(LayoutSwitchOutcome::RejectedOrTimedOut) => {
                    self.policy.buffer_mut().clear();
                    return EngineOutcome::AbortedSwitchRejected;
                }
                Err(e) => {
                    self.policy.buffer_mut().clear();
                    return EngineOutcome::LayoutError(e.to_string());
                }
            };

        // 4. Re-check freshness before erasing; if stale, best-effort restore prior layout.
        if !self.is_fresh(ev.focus_id, ev.seq, focus) {
            let _ = layouts.switch_and_confirm(&current_layout, self.confirm_timeout_ms);
            self.policy.buffer_mut().clear();
            return EngineOutcome::AbortedStale;
        }

        // 5. Plan & execute injection (mask key + L/R modifier release + erase + replay).
        let snapshot = self.policy.buffer().snapshot();
        let count = snapshot.len();
        let steps = plan_injection(ev.modifiers, count, &snapshot, self.config.replay_delay_ms);
        if let Err(err) = injector.execute_steps(&steps) {
            self.policy.buffer_mut().clear();
            return EngineOutcome::FailedInjection(err);
        }

        // 6. Retain buffer ONLY after full success (enables clean round-trip on second press).
        self.policy
            .buffer_mut()
            .set_burst_layout(confirmed_layout.clone());
        EngineOutcome::Converted {
            erased: count,
            replayed: count,
            new_layout: confirmed_layout,
        }
    }
}
