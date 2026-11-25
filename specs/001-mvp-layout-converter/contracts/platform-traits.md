# Platform & Extension Contracts: LangFlip

**Feature**: `001-mvp-layout-converter` | **Date**: 2026-11-25 (Revision 3 after Multi-Agent Design Review)

All OS-specific functionality is isolated behind narrow traits defined in `langflip-core::ports`. All pure logic (`classify_key`, `HotkeyMatcher`, `resolve_layouts`, `plan_injection`, `ConversionEngine`) lives in `langflip-core` so OS adapters stay thin translators.

---

## 1. Focus Types (`langflip-core::ports`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FocusId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlKind {
    Unknown,
    SingleLineText,
    MultiLineText,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusContext {
    pub focus_id: FocusId,
    pub element_id: Option<String>,
    pub control_kind: ControlKind,
}

pub trait FocusObserver {
    fn current_focus(&self) -> FocusContext;
}
```

## 2. `InputListener` & `ListenerShutdown`
Captures global keyboard, mouse-click, and foreground focus-change events into a **bounded** `SyncSender` (`sync_channel(256)`). Uses `HotkeyMatcher` from `langflip-core` inside the hook only to decide whether to swallow the hotkey press/repeat, and exposes `ListenerShutdown::stop()` to wake idle OS message pumps (`PostThreadMessageW(WM_QUIT)` on Windows).

```rust
pub trait ListenerShutdown: Send + Sync {
    /// Signals the listener to stop and wakes any blocked OS message pump immediately.
    fn stop(&self);
}

pub trait InputListener {
    type Error: std::fmt::Display;
    type Shutdown: ListenerShutdown;

    /// Start listening for global input & focus events, enqueuing via non-blocking `try_send`.
    /// - Advances `latest_seq` ONLY on mutating non-injected events (never on KeyUp of hotkey/modifiers).
    /// - On channel full, sets `overflow_flag = true` and advances `latest_seq` without blocking.
    /// - Passes `Shutdown` handle to `on_ready` before entering the OS message loop.
    fn run<F>(
        self,
        hotkey: HotkeySpec,
        tx: std::sync::mpsc::SyncSender<InputEvent>,
        overflow_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
        latest_seq: std::sync::Arc<std::sync::atomic::AtomicU64>,
        on_ready: F,
    ) -> Result<(), Self::Error>
    where
        F: FnOnce(Self::Shutdown);
}
```

## 3. `InputInjector`
Executes the pure `Vec<InjectionStep>` planned by `langflip-core::planner::plan_injection(...)` (mask key, exact left/right modifier release, Backspaces, Shift-wrapped scan-code taps, optional per-keystroke delay), tagging every synthetic event with `LANGFLIP_INJECTED_MARKER`.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectError {
    PartialInjection { expected: usize, sent: usize },
    OsError(String),
}

pub trait InputInjector {
    /// Execute a planned sequence of low-level injection steps, all tagged as self-injected.
    /// Returns PartialInjection if the OS reports fewer inserted events than requested.
    fn execute_steps(&mut self, steps: &[InjectionStep]) -> Result<(), InjectError>;
}
```

## 4. `LayoutController`
Queries installed layouts (feeding `langflip-core::layout::resolve_layouts`), switches the foreground target's layout, and confirms acceptance before erasure proceeds.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutSwitchOutcome {
    Confirmed(LayoutId),
    RejectedOrTimedOut,
}

pub trait LayoutController {
    type Error: std::fmt::Display;

    /// Returns the currently active keyboard layout for the foreground input target.
    fn active_layout(&self) -> Result<LayoutId, Self::Error>;

    /// Requests switching the foreground input target to `target` and polls up to
    /// `confirm_timeout_ms` to confirm the target accepted the switch.
    fn switch_and_confirm(
        &mut self,
        target: &LayoutId,
        confirm_timeout_ms: u64,
    ) -> Result<LayoutSwitchOutcome, Self::Error>;

    /// Enumerates keyboard layouts currently installed on the OS (with KLID/ID + language tag).
    /// Passed into `langflip-core::layout::resolve_layouts` for pure, testable resolution.
    fn list_installed_layouts(&self) -> Result<Vec<InstalledLayout>, Self::Error>;
}
```

## 5. `BufferPolicy` *(Extension Seam for Phase 2)*
- **MVP (`NaiveBurstPolicy`)**: Linear append/backspace buffer; resets on `FR-005` boundaries; tail-bounded on buffer overflow.
- **Phase 2 (`CursorAwarePolicy`)**: Tracks cursor index across arrows/`Home`/`End`/`Delete` and preserves `Enter` when `FocusContext.control_kind == ControlKind::MultiLineText`.
