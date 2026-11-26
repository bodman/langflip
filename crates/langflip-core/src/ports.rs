//! Narrow OS adapter traits and extension seams for LangFlip.
//!
//! All platform-specific code lives behind these traits so `langflip-core`
//! remains 100% OS-independent and natively testable.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::mpsc::SyncSender;

use crate::hotkey::HotkeySpec;
use crate::key::{BufferedKeystroke, InputEvent, PhysicalKey};
use crate::layout::{InstalledLayout, LayoutId, LayoutPair};

/// Opaque identifier for a foreground focus target (e.g., `HWND` value on Windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FocusId(pub u64);

/// Classification of the currently focused UI control (Phase 2 UIA seam).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlKind {
    Unknown,
    SingleLineText,
    MultiLineText,
    Terminal,
}

/// Snapshot of the focused UI target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusContext {
    pub focus_id: FocusId,
    pub element_id: Option<String>,
    pub control_kind: ControlKind,
}

/// RAM-only text read from the focused editable control when the live buffer is empty.
///
/// Privacy invariant: `Debug` NEVER prints text contents.
#[derive(Clone, PartialEq, Eq)]
pub struct FallbackText {
    pub text: String,
    /// `true` when `text` came from an active selection (1 Backspace erases it);
    /// `false` when `text` is the current line before the caret (`text.chars().count()` Backspaces).
    pub replaces_selection: bool,
}

impl fmt::Debug for FallbackText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FallbackText")
            .field("char_len", &self.text.chars().count())
            .field("replaces_selection", &self.replaces_selection)
            .finish()
    }
}

/// Observes foreground focus target and control metadata.
pub trait FocusObserver {
    fn current_focus(&self) -> FocusContext;

    /// Returns `true` if the focused input control currently holds a non-empty
    /// highlighted text selection (e.g., browser Omnibox inline autocomplete).
    /// Must never read or log selected text. Defaults to `false`.
    fn has_active_selection(&self) -> bool {
        false
    }

    /// Reads the current line before the caret (or active single-line selection)
    /// from an editable, non-password, non-terminal control when the live keystroke
    /// buffer is empty. Returns `None` if unavailable, unsafe, or longer than `max_len`.
    fn read_line_before_caret(&self, _max_len: usize) -> Option<FallbackText> {
        None
    }

    /// Collapses a synthetic fallback selection if conversion is aborted before erasure.
    fn clear_fallback_selection(&self) {}
}

/// Low-level injection step planned by `langflip-core::planner::plan_injection`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectionStep {
    /// Harmless unassigned mask key to prevent Alt-menu / Ctrl+Shift OS-switch misfires.
    MaskKey,
    /// Release a specific physically held left/right modifier key.
    ReleaseModifier(PhysicalKey),
    /// Tap a physical key (optionally wrapped in Left Shift down/up).
    KeyTap { key: PhysicalKey, shift: bool },
    /// Pause between keystrokes (only emitted when `replay_delay_ms > 0`).
    DelayMs(u64),
}

/// Errors reported by `InputInjector`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectError {
    PartialInjection { expected: usize, sent: usize },
    OsError(String),
}

impl fmt::Display for InjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PartialInjection { expected, sent } => {
                write!(f, "partial key injection: sent {sent} of {expected} events")
            }
            Self::OsError(msg) => write!(f, "OS injection error: {msg}"),
        }
    }
}

impl std::error::Error for InjectError {}

/// Executes planned synthetic input steps tagged with LangFlip's self-injection marker.
pub trait InputInjector {
    fn execute_steps(&mut self, steps: &[InjectionStep]) -> Result<(), InjectError>;
}

/// Result of attempting to switch and confirm the foreground keyboard layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutSwitchOutcome {
    Confirmed(LayoutId),
    RejectedOrTimedOut,
}

/// Queries installed layouts, switches foreground layout, and confirms acceptance.
pub trait LayoutController {
    type Error: fmt::Display;

    fn active_layout(&self) -> Result<LayoutId, Self::Error>;

    fn switch_and_confirm(
        &mut self,
        target: &LayoutId,
        confirm_timeout_ms: u64,
    ) -> Result<LayoutSwitchOutcome, Self::Error>;

    fn list_installed_layouts(&self) -> Result<Vec<InstalledLayout>, Self::Error>;

    /// Reverse-maps Unicode text into physical keystrokes under `layout`.
    /// Returns `None` if any character cannot be represented cleanly without
    /// non-Shift modifiers or if the layout is unavailable.
    fn text_to_keystrokes(
        &self,
        _text: &str,
        _layout: &LayoutId,
    ) -> Option<Vec<BufferedKeystroke>> {
        None
    }
}

/// Wake-up handle allowing idle OS message pumps (e.g., `GetMessageW`) to exit immediately.
pub trait ListenerShutdown: Send + Sync {
    fn stop(&self);
}

/// Captures global input and focus events into a bounded `SyncSender`.
pub trait InputListener {
    type Error: fmt::Display;
    type Shutdown: ListenerShutdown;

    fn run<F>(
        self,
        hotkey: HotkeySpec,
        tx: SyncSender<InputEvent>,
        overflow_flag: Arc<AtomicBool>,
        latest_seq: Arc<AtomicU64>,
        on_ready: F,
    ) -> Result<(), Self::Error>
    where
        F: FnOnce(Self::Shutdown);
}

/// Current settings snapshot handed to the OS tray / Settings UI.
#[derive(Debug, Clone)]
pub struct SettingsSnapshot {
    pub installed_layouts: Vec<InstalledLayout>,
    pub layouts: LayoutPair,
    pub hotkey: HotkeySpec,
}

/// Cross-platform system tray & Settings UI seam.
///
/// Implemented natively per OS (`Shell_NotifyIconW` on Windows, `NSStatusItem` on
/// macOS, StatusNotifier/GTK on Linux) while all validation, persistence, and
/// live engine updates stay in `langflip-core`.
pub trait TrayHost {
    type Error: fmt::Display;

    fn run<S, E>(
        self,
        initial: SettingsSnapshot,
        on_save: S,
        on_exit: E,
    ) -> Result<(), Self::Error>
    where
        S: FnMut(LayoutPair, HotkeySpec) -> Result<(), String> + 'static,
        E: FnOnce() + 'static;
}
