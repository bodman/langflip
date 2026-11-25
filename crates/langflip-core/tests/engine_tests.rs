//! End-to-end mocked integration tests for `ConversionEngine` (`T022` Group A & `T023` Group B).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use langflip_core::config::AppConfig;
use langflip_core::engine::{ConversionEngine, EngineOutcome};
use langflip_core::key::{InjectedBy, InputEvent, KeyEvent, Modifiers, PhysicalKey, scan};
use langflip_core::layout::{InstalledLayout, LayoutId, LayoutPair};
use langflip_core::ports::{
    ControlKind, FocusContext, FocusId, FocusObserver, InjectError, InjectionStep, InputInjector,
    LayoutController, LayoutSwitchOutcome,
};

#[derive(Default)]
struct MockInjector {
    executed: Vec<Vec<InjectionStep>>,
    fail_with: Option<InjectError>,
}

impl InputInjector for MockInjector {
    fn execute_steps(&mut self, steps: &[InjectionStep]) -> Result<(), InjectError> {
        if let Some(err) = self.fail_with.clone() {
            return Err(err);
        }
        self.executed.push(steps.to_vec());
        Ok(())
    }
}

struct MockLayouts {
    active: LayoutId,
    accept_switch: bool,
    switch_requests: Vec<LayoutId>,
    on_switch: Option<Box<dyn FnMut()>>,
}

impl MockLayouts {
    fn new(active: LayoutId) -> Self {
        Self {
            active,
            accept_switch: true,
            switch_requests: Vec::new(),
            on_switch: None,
        }
    }
}

impl LayoutController for MockLayouts {
    type Error = String;

    fn active_layout(&self) -> Result<LayoutId, Self::Error> {
        Ok(self.active.clone())
    }

    fn switch_and_confirm(
        &mut self,
        target: &LayoutId,
        _confirm_timeout_ms: u64,
    ) -> Result<LayoutSwitchOutcome, Self::Error> {
        self.switch_requests.push(target.clone());
        if let Some(cb) = self.on_switch.as_mut() {
            cb();
        }
        if self.accept_switch {
            self.active = target.clone();
            Ok(LayoutSwitchOutcome::Confirmed(target.clone()))
        } else {
            Ok(LayoutSwitchOutcome::RejectedOrTimedOut)
        }
    }

    fn list_installed_layouts(&self) -> Result<Vec<InstalledLayout>, Self::Error> {
        Ok(vec![])
    }
}

struct MockFocus {
    current: Arc<AtomicU64>,
}

impl MockFocus {
    fn new(id: u64) -> Self {
        Self {
            current: Arc::new(AtomicU64::new(id)),
        }
    }
}

impl FocusObserver for MockFocus {
    fn current_focus(&self) -> FocusContext {
        FocusContext {
            focus_id: FocusId(self.current.load(Ordering::SeqCst)),
            element_id: None,
            control_kind: ControlKind::Unknown,
        }
    }
}

fn en() -> LayoutId {
    LayoutId::new("00004009")
}

fn he() -> LayoutId {
    LayoutId::new("0002040D")
}

fn third() -> LayoutId {
    LayoutId::new("00000419")
}

fn make_config() -> AppConfig {
    AppConfig {
        layouts: LayoutPair {
            primary: en(),
            secondary: he(),
        },
        replay_delay_ms: 0,
        ..AppConfig::default()
    }
}

fn key_down(
    key: PhysicalKey,
    modifiers: Modifiers,
    layout: LayoutId,
    focus: u64,
    seq: u64,
) -> InputEvent {
    InputEvent::KeyDown(KeyEvent {
        key,
        modifiers,
        injected: InjectedBy::Physical,
        active_layout: layout,
        focus_id: FocusId(focus),
        seq,
    })
}

fn key_up(key: PhysicalKey, layout: LayoutId, focus: u64, seq: u64) -> InputEvent {
    InputEvent::KeyUp(KeyEvent {
        key,
        modifiers: Modifiers::default(),
        injected: InjectedBy::Physical,
        active_layout: layout,
        focus_id: FocusId(focus),
        seq,
    })
}

// ============================================================================
// T022 — Group A: Core Flows & Every FR-005 Reset Boundary
// ============================================================================

#[test]
fn converts_burst_switch_before_erase_and_round_trips_on_second_press() {
    let overflow = Arc::new(AtomicBool::new(false));
    let latest_seq = Arc::new(AtomicU64::new(0));
    let mut engine = ConversionEngine::new(make_config(), overflow, latest_seq.clone());
    let mut injector = MockInjector::default();
    let mut layouts = MockLayouts::new(he());
    let focus = MockFocus::new(1);

    // Type 'H' (shifted 0x23) + 'e' (0x12) + typo 'x' (0x2D) + Backspace
    let shifted = Modifiers { left_shift: true, ..Default::default() };
    latest_seq.store(1, Ordering::SeqCst);
    assert_eq!(
        engine.handle_event(key_down(PhysicalKey::new(0x23, false), shifted, he(), 1, 1), &mut injector, &mut layouts, &focus),
        EngineOutcome::Buffered
    );
    latest_seq.store(2, Ordering::SeqCst);
    engine.handle_event(key_down(PhysicalKey::new(0x12, false), Modifiers::default(), he(), 1, 2), &mut injector, &mut layouts, &focus);
    latest_seq.store(3, Ordering::SeqCst);
    engine.handle_event(key_down(PhysicalKey::new(0x2D, false), Modifiers::default(), he(), 1, 3), &mut injector, &mut layouts, &focus);
    latest_seq.store(4, Ordering::SeqCst);
    engine.handle_event(key_down(scan::BACKSPACE, Modifiers::default(), he(), 1, 4), &mut injector, &mut layouts, &focus);

    // Press Pause (does not advance latest_seq)
    let out = engine.handle_event(key_down(scan::PAUSE, Modifiers::default(), he(), 1, 4), &mut injector, &mut layouts, &focus);
    assert_eq!(
        out,
        EngineOutcome::Converted {
            erased: 2,
            replayed: 2,
            new_layout: en(),
        }
    );
    assert_eq!(layouts.switch_requests, vec![en()]);
    assert_eq!(injector.executed.len(), 1);
    assert_eq!(
        injector.executed[0],
        vec![
            InjectionStep::KeyTap { key: scan::BACKSPACE, shift: false },
            InjectionStep::KeyTap { key: scan::BACKSPACE, shift: false },
            InjectionStep::KeyTap { key: PhysicalKey::new(0x23, false), shift: true },
            InjectionStep::KeyTap { key: PhysicalKey::new(0x12, false), shift: false },
        ]
    );

    // Auto-repeat while Pause is still held -> ignored (no second conversion)
    let rep = engine.handle_event(key_down(scan::PAUSE, Modifiers::default(), en(), 1, 4), &mut injector, &mut layouts, &focus);
    assert_eq!(rep, EngineOutcome::Ignored);
    assert_eq!(injector.executed.len(), 1);

    // Release Pause (does NOT advance latest_seq) -> press Pause again -> clean round-trip back to HE
    engine.handle_event(key_up(scan::PAUSE, en(), 1, 4), &mut injector, &mut layouts, &focus);
    let out2 = engine.handle_event(key_down(scan::PAUSE, Modifiers::default(), en(), 1, 4), &mut injector, &mut layouts, &focus);
    assert_eq!(
        out2,
        EngineOutcome::Converted {
            erased: 2,
            replayed: 2,
            new_layout: he(),
        }
    );
    assert_eq!(layouts.switch_requests, vec![en(), he()]);
}

#[test]
fn empty_buffer_toggles_layout_only_when_enabled() {
    let overflow = Arc::new(AtomicBool::new(false));
    let latest_seq = Arc::new(AtomicU64::new(0));
    let mut engine = ConversionEngine::new(make_config(), overflow, latest_seq);
    let mut injector = MockInjector::default();
    let mut layouts = MockLayouts::new(en());
    let focus = MockFocus::new(1);

    let out = engine.handle_event(key_down(scan::PAUSE, Modifiers::default(), en(), 1, 0), &mut injector, &mut layouts, &focus);
    assert_eq!(out, EngineOutcome::LayoutToggled(he()));
    assert!(injector.executed.is_empty());

    // Disabled -> Ignored
    let mut cfg = make_config();
    cfg.toggle_on_empty_buffer = false;
    let mut engine2 = ConversionEngine::new(cfg, Arc::new(AtomicBool::new(false)), Arc::new(AtomicU64::new(0)));
    let out2 = engine2.handle_event(key_down(scan::PAUSE, Modifiers::default(), he(), 1, 0), &mut injector, &mut layouts, &focus);
    assert_eq!(out2, EngineOutcome::Ignored);
}

#[test]
fn every_fr005_reset_boundary_clears_active_burst() {
    let reset_keys = [
        scan::ENTER,
        scan::NUMPAD_ENTER,
        scan::TAB,
        scan::ESCAPE,
        scan::INSERT,
        scan::DELETE,
        scan::CAPS_LOCK,
        scan::LEFT,
        scan::RIGHT,
        scan::UP,
        scan::DOWN,
        scan::HOME,
        scan::END,
        scan::PAGE_UP,
        scan::PAGE_DOWN,
        PhysicalKey::new(0x4B, false), // numpad 4/Left with NumLock off
    ];

    for boundary in reset_keys {
        let overflow = Arc::new(AtomicBool::new(false));
        let latest_seq = Arc::new(AtomicU64::new(1));
        let mut engine = ConversionEngine::new(make_config(), overflow, latest_seq.clone());
        let mut injector = MockInjector::default();
        let mut layouts = MockLayouts::new(en());
        let focus = MockFocus::new(1);

        engine.handle_event(key_down(PhysicalKey::new(0x23, false), Modifiers::default(), en(), 1, 1), &mut injector, &mut layouts, &focus);
        assert_eq!(engine.buffer().len(), 1);

        latest_seq.store(2, Ordering::SeqCst);
        let res = engine.handle_event(key_down(boundary, Modifiers::default(), en(), 1, 2), &mut injector, &mut layouts, &focus);
        assert_eq!(res, EngineOutcome::BufferReset, "boundary {boundary:?} did not reset buffer");
        assert!(engine.buffer().is_empty());
    }

    // Shortcuts (Ctrl+C, AltGr), external synthetic input, mouse click, focus change, external layout change
    let overflow = Arc::new(AtomicBool::new(false));
    let latest_seq = Arc::new(AtomicU64::new(1));
    let mut engine = ConversionEngine::new(make_config(), overflow, latest_seq.clone());
    let mut injector = MockInjector::default();
    let mut layouts = MockLayouts::new(en());
    let focus = MockFocus::new(1);

    // Ctrl+C resets
    engine.handle_event(key_down(PhysicalKey::new(0x23, false), Modifiers::default(), en(), 1, 1), &mut injector, &mut layouts, &focus);
    let ctrl = Modifiers { left_ctrl: true, ..Default::default() };
    assert_eq!(engine.handle_event(key_down(PhysicalKey::new(0x2E, false), ctrl, en(), 1, 2), &mut injector, &mut layouts, &focus), EngineOutcome::BufferReset);
    assert!(engine.buffer().is_empty());

    // Externally injected input resets
    engine.handle_event(key_down(PhysicalKey::new(0x23, false), Modifiers::default(), en(), 1, 3), &mut injector, &mut layouts, &focus);
    let ext = InputEvent::KeyDown(KeyEvent {
        key: PhysicalKey::new(0x12, false),
        modifiers: Modifiers::default(),
        injected: InjectedBy::External,
        active_layout: en(),
        focus_id: FocusId(1),
        seq: 4,
    });
    assert_eq!(engine.handle_event(ext, &mut injector, &mut layouts, &focus), EngineOutcome::BufferReset);

    // MouseClick resets
    engine.handle_event(key_down(PhysicalKey::new(0x23, false), Modifiers::default(), en(), 1, 5), &mut injector, &mut layouts, &focus);
    assert_eq!(engine.handle_event(InputEvent::MouseClick { focus_id: FocusId(1), seq: 6 }, &mut injector, &mut layouts, &focus), EngineOutcome::BufferReset);

    // FocusChanged resets
    engine.handle_event(key_down(PhysicalKey::new(0x23, false), Modifiers::default(), en(), 1, 7), &mut injector, &mut layouts, &focus);
    assert_eq!(
        engine.handle_event(
            InputEvent::FocusChanged {
                focus: FocusContext { focus_id: FocusId(2), element_id: None, control_kind: ControlKind::Unknown },
                seq: 8,
            },
            &mut injector,
            &mut layouts,
            &focus,
        ),
        EngineOutcome::BufferReset
    );

    // External layout change between keystrokes resets prior burst and starts fresh in new layout
    engine.handle_event(key_down(PhysicalKey::new(0x23, false), Modifiers::default(), en(), 1, 9), &mut injector, &mut layouts, &focus);
    engine.handle_event(key_down(PhysicalKey::new(0x12, false), Modifiers::default(), he(), 1, 10), &mut injector, &mut layouts, &focus);
    assert_eq!(engine.buffer().len(), 1);
    assert_eq!(engine.buffer().burst_layout(), Some(&he()));
}

// ============================================================================
// T023 — Group B: Safety, Races, Overflow, 3rd Layout & Self-Injection
// ============================================================================

#[test]
fn rejected_layout_switch_aborts_before_any_erasure_and_clears_buffer() {
    let overflow = Arc::new(AtomicBool::new(false));
    let latest_seq = Arc::new(AtomicU64::new(1));
    let mut engine = ConversionEngine::new(make_config(), overflow, latest_seq);
    let mut injector = MockInjector::default();
    let mut layouts = MockLayouts::new(he());
    layouts.accept_switch = false;
    let focus = MockFocus::new(1);

    engine.handle_event(key_down(PhysicalKey::new(0x23, false), Modifiers::default(), he(), 1, 1), &mut injector, &mut layouts, &focus);
    let out = engine.handle_event(key_down(scan::PAUSE, Modifiers::default(), he(), 1, 1), &mut injector, &mut layouts, &focus);

    assert_eq!(out, EngineOutcome::AbortedSwitchRejected);
    assert!(injector.executed.is_empty(), "must NEVER send Backspaces when switch fails");
    assert!(engine.buffer().is_empty());
}

#[test]
fn partial_injection_clears_buffer_and_does_not_retain_for_round_trip() {
    let overflow = Arc::new(AtomicBool::new(false));
    let latest_seq = Arc::new(AtomicU64::new(1));
    let mut engine = ConversionEngine::new(make_config(), overflow, latest_seq);
    let mut injector = MockInjector {
        executed: vec![],
        fail_with: Some(InjectError::PartialInjection { expected: 4, sent: 2 }),
    };
    let mut layouts = MockLayouts::new(he());
    let focus = MockFocus::new(1);

    engine.handle_event(key_down(PhysicalKey::new(0x23, false), Modifiers::default(), he(), 1, 1), &mut injector, &mut layouts, &focus);
    let out = engine.handle_event(key_down(scan::PAUSE, Modifiers::default(), he(), 1, 1), &mut injector, &mut layouts, &focus);

    assert_eq!(
        out,
        EngineOutcome::FailedInjection(InjectError::PartialInjection { expected: 4, sent: 2 })
    );
    assert!(engine.buffer().is_empty());
}

#[test]
fn rapid_typing_or_focus_change_during_switch_cancels_and_restores_layout() {
    let overflow = Arc::new(AtomicBool::new(false));
    let latest_seq = Arc::new(AtomicU64::new(1));
    let seq_for_cb = latest_seq.clone();

    let mut engine = ConversionEngine::new(make_config(), overflow, latest_seq.clone());
    let mut injector = MockInjector::default();
    let mut layouts = MockLayouts::new(he());
    // Simulate user typing a new character while layout switch is in flight
    layouts.on_switch = Some(Box::new(move || {
        seq_for_cb.store(2, Ordering::SeqCst);
    }));
    let focus = MockFocus::new(1);

    engine.handle_event(key_down(PhysicalKey::new(0x23, false), Modifiers::default(), he(), 1, 1), &mut injector, &mut layouts, &focus);
    let out = engine.handle_event(key_down(scan::PAUSE, Modifiers::default(), he(), 1, 1), &mut injector, &mut layouts, &focus);

    assert_eq!(out, EngineOutcome::AbortedStale);
    assert!(injector.executed.is_empty(), "must not erase when stale");
    assert!(engine.buffer().is_empty());
    // First switch requested EN, then best-effort restore requested HE
    assert_eq!(layouts.switch_requests, vec![en(), he()]);
}

#[test]
fn queue_overflow_invalidates_burst_and_self_injected_is_ignored() {
    let overflow = Arc::new(AtomicBool::new(false));
    let latest_seq = Arc::new(AtomicU64::new(1));
    let mut engine = ConversionEngine::new(make_config(), overflow.clone(), latest_seq);
    let mut injector = MockInjector::default();
    let mut layouts = MockLayouts::new(en());
    let focus = MockFocus::new(1);

    engine.handle_event(key_down(PhysicalKey::new(0x23, false), Modifiers::default(), en(), 1, 1), &mut injector, &mut layouts, &focus);

    // Self-injected event must be ignored without mutating buffer
    let self_ev = InputEvent::KeyDown(KeyEvent {
        key: PhysicalKey::new(0x12, false),
        modifiers: Modifiers::default(),
        injected: InjectedBy::SelfMarker,
        active_layout: en(),
        focus_id: FocusId(1),
        seq: 1,
    });
    assert_eq!(engine.handle_event(self_ev, &mut injector, &mut layouts, &focus), EngineOutcome::Ignored);
    assert_eq!(engine.buffer().len(), 1);

    // QueueOverflow invalidates burst
    assert_eq!(engine.handle_event(InputEvent::QueueOverflow { seq: 2 }, &mut injector, &mut layouts, &focus), EngineOutcome::BufferReset);
    assert!(engine.buffer().is_empty());
}

#[test]
fn third_unrelated_layout_never_destructively_erases_non_empty_burst() {
    let overflow = Arc::new(AtomicBool::new(false));
    let latest_seq = Arc::new(AtomicU64::new(1));
    let mut engine = ConversionEngine::new(make_config(), overflow, latest_seq);
    let mut injector = MockInjector::default();
    let mut layouts = MockLayouts::new(third());
    let focus = MockFocus::new(1);

    engine.handle_event(key_down(PhysicalKey::new(0x23, false), Modifiers::default(), third(), 1, 1), &mut injector, &mut layouts, &focus);
    let out = engine.handle_event(key_down(scan::PAUSE, Modifiers::default(), third(), 1, 1), &mut injector, &mut layouts, &focus);

    assert_eq!(out, EngineOutcome::AbortedThirdLayout);
    assert!(injector.executed.is_empty());
    assert!(engine.buffer().is_empty());
}

#[test]
fn buffer_tail_overflow_converts_only_retained_tail() {
    let mut cfg = make_config();
    cfg.max_buffer_len = 2;
    let overflow = Arc::new(AtomicBool::new(false));
    let latest_seq = Arc::new(AtomicU64::new(3));
    let mut engine = ConversionEngine::new(cfg, overflow, latest_seq);
    let mut injector = MockInjector::default();
    let mut layouts = MockLayouts::new(he());
    let focus = MockFocus::new(1);

    for (i, code) in [0x10u16, 0x11, 0x12].into_iter().enumerate() {
        engine.handle_event(key_down(PhysicalKey::new(code, false), Modifiers::default(), he(), 1, (i + 1) as u64), &mut injector, &mut layouts, &focus);
    }

    let out = engine.handle_event(key_down(scan::PAUSE, Modifiers::default(), he(), 1, 3), &mut injector, &mut layouts, &focus);
    assert_eq!(
        out,
        EngineOutcome::Converted {
            erased: 2,
            replayed: 2,
            new_layout: en(),
        }
    );
}
