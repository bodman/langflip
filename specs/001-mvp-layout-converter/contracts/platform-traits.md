# Platform & Extension Contracts: LangFlip

**Feature**: `001-mvp-layout-converter` | **Date**: 2026-11-25 (Revised after Design Review)

All OS-specific functionality is isolated behind narrow traits defined in `langflip-core::ports`. The core engine depends ONLY on these traits, enabling:
1. 100% deterministic unit/integration tests on Linux using in-memory fakes/mocks.
2. Clean addition of Windows Phase 2+ features (UI Automation `FocusObserver`) and future macOS/Linux adapters without modifying `langflip-core`.

---

## 1. `InputListener` & `ListenerShutdown`
Captures global OS input events into a **bounded** channel (`SyncSender`, never unbounded) and exposes an explicit wake-up handle so idle message loops exit immediately on shutdown.

```rust
pub trait ListenerShutdown: Send + Sync {
    /// Signals the listener to stop and wakes any blocked OS message pump (e.g., WM_QUIT).
    fn stop(&self);
}

pub trait InputListener {
    type Error: std::fmt::Display;
    type Shutdown: ListenerShutdown;

    /// Start listening for global input events and enqueue them via non-blocking `try_send`.
    /// On channel full, sets `overflow_flag` to `true` without blocking.
    /// Suppresses trigger hotkey events (including auto-repeat while held) from reaching apps.
    /// Passes the created `Shutdown` handle to `on_ready` before entering the OS message loop.
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

## 2. `InputInjector`
Executes modifier hygiene, physical erasure, and keystroke replay on the active foreground target, reporting partial-injection failures explicitly.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectError {
    PartialInjection { expected: usize, sent: usize },
    OsError(String),
}

pub trait InputInjector {
    /// Release any physically held combo modifiers (e.g., Ctrl/Alt/Shift/Meta from a combo hotkey)
    /// via tagged KEYUP events before erasure/replay begins.
    fn release_modifiers(&mut self, held: Modifiers) -> Result<(), InjectError>;

    /// Emit `count` Backspace keystrokes, tagged as self-injected.
    /// `delay_ms` is applied per keystroke (0 = batched SendInput).
    fn send_backspaces(&mut self, count: usize, delay_ms: u64) -> Result<(), InjectError>;

    /// Replay buffered physical keystrokes (with Shift state) under the active layout,
    /// tagged as self-injected. `delay_ms` is applied per keystroke.
    fn replay_keystrokes(
        &mut self,
        strokes: &[BufferedKeystroke],
        delay_ms: u64,
    ) -> Result<(), InjectError>;
}
```

## 3. `LayoutController`
Queries, resolves, switches, and **confirms** keyboard layout changes on the foreground target.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutSwitchOutcome {
    Confirmed(LayoutId),
    RejectedOrTimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutResolutionWarning {
    pub configured: LayoutId,
    pub message: String,
}

pub trait LayoutController {
    type Error: std::fmt::Display;

    /// Returns the currently active keyboard layout for the foreground input target.
    fn active_layout(&self) -> Result<LayoutId, Self::Error>;

    /// Requests switching the foreground input target to `target` and waits/polls up to
    /// `confirm_timeout_ms` to confirm the target accepted the switch.
    fn switch_and_confirm(
        &mut self,
        target: &LayoutId,
        confirm_timeout_ms: u64,
    ) -> Result<LayoutSwitchOutcome, Self::Error>;

    /// Lists keyboard layouts currently installed on the OS.
    /// Used in MVP for startup validation/ambiguity warnings and in Phase 2 for auto-discovery.
    fn list_installed_layouts(&self) -> Result<Vec<LayoutId>, Self::Error>;

    /// Resolves a configured LayoutPair against installed OS layouts, returning canonical IDs
    /// plus any non-fatal ambiguity or missing-layout warnings.
    fn validate_pair(
        &self,
        pair: &LayoutPair,
    ) -> Result<(LayoutPair, Vec<LayoutResolutionWarning>), Self::Error>;
}
```

## 4. `FocusObserver` *(Extension Seam for Phase 2)*
Reports the active foreground target identity (`FocusId`) and input-area metadata. Used in MVP for focus-change reset and trigger-snapshot freshness checks; upgraded to Windows UI Automation in Phase 2.

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

## 5. `BufferPolicy` *(Extension Seam for Phase 2)*
Encapsulates how keystrokes and boundary events mutate the buffer:
- **MVP (`NaiveBurstPolicy`)**: Linear append/backspace buffer; resets on `Enter`, `Tab`, `Escape`, navigation keys, `CapsLock`, shortcuts, mouse click, focus change, external layout change, and queue overflow.
- **Phase 2 (`CursorAwarePolicy`)**: Tracks cursor index across arrows/`Home`/`End`/`Delete` and preserves `Enter` when `FocusContext.control_kind == ControlKind::MultiLineText`.
