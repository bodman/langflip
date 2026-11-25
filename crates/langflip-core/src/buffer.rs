//! Bounded, RAM-only, privacy-redacted keystroke buffer behind a pluggable
//! `BufferPolicy` extension seam (NaiveBurstPolicy in MVP; CursorAwarePolicy in Phase 2).

use std::collections::VecDeque;
use std::fmt;

use crate::key::BufferedKeystroke;
use crate::layout::LayoutId;
use crate::ports::{FocusContext, FocusId};

/// Policy governing how keystrokes and boundary events mutate the buffer.
pub trait BufferPolicy {
    fn on_printable(&mut self, stroke: BufferedKeystroke);
    fn on_backspace(&mut self);
    fn on_reset(&mut self, focus: Option<&FocusContext>);
    fn snapshot(&self) -> Vec<BufferedKeystroke>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Bounded RAM-only linear burst buffer.
///
/// Privacy invariant: `Debug` NEVER prints keystroke contents.
pub struct KeystrokeBuffer {
    entries: VecDeque<BufferedKeystroke>,
    max_len: usize,
    burst_layout: Option<LayoutId>,
    burst_focus: Option<FocusId>,
}

impl fmt::Debug for KeystrokeBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeystrokeBuffer")
            .field("len", &self.entries.len())
            .field("max_len", &self.max_len)
            .field("burst_layout", &self.burst_layout)
            .field("burst_focus", &self.burst_focus)
            .finish()
    }
}

impl KeystrokeBuffer {
    pub fn new(max_len: usize) -> Self {
        assert!(max_len >= 1, "max_len must be >= 1");
        Self {
            entries: VecDeque::with_capacity(max_len.min(128)),
            max_len,
            burst_layout: None,
            burst_focus: None,
        }
    }

    pub fn push(&mut self, stroke: BufferedKeystroke, layout: LayoutId, focus: FocusId) {
        if self.entries.len() == self.max_len {
            self.entries.pop_front();
        }
        self.entries.push_back(stroke);
        self.burst_layout = Some(layout);
        self.burst_focus = Some(focus);
    }

    pub fn pop_backspace(&mut self) {
        self.entries.pop_back();
        if self.entries.is_empty() {
            self.burst_layout = None;
            self.burst_focus = None;
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.burst_layout = None;
        self.burst_focus = None;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn burst_layout(&self) -> Option<&LayoutId> {
        self.burst_layout.as_ref()
    }

    pub fn burst_focus(&self) -> Option<FocusId> {
        self.burst_focus
    }

    pub fn set_burst_layout(&mut self, layout: LayoutId) {
        self.burst_layout = Some(layout);
    }

    pub fn snapshot(&self) -> Vec<BufferedKeystroke> {
        self.entries.iter().copied().collect()
    }
}

/// MVP buffer policy: simple linear burst; resets on all FR-005 boundaries.
#[derive(Debug)]
pub struct NaiveBurstPolicy {
    buffer: KeystrokeBuffer,
}

impl NaiveBurstPolicy {
    pub fn new(max_len: usize) -> Self {
        Self {
            buffer: KeystrokeBuffer::new(max_len),
        }
    }

    pub fn buffer(&self) -> &KeystrokeBuffer {
        &self.buffer
    }

    pub fn buffer_mut(&mut self) -> &mut KeystrokeBuffer {
        &mut self.buffer
    }
}

impl BufferPolicy for NaiveBurstPolicy {
    fn on_printable(&mut self, stroke: BufferedKeystroke) {
        let layout = self
            .buffer
            .burst_layout()
            .cloned()
            .unwrap_or_else(|| LayoutId::new(""));
        let focus = self.buffer.burst_focus().unwrap_or(FocusId(0));
        self.buffer.push(stroke, layout, focus);
    }

    fn on_backspace(&mut self) {
        self.buffer.pop_backspace();
    }

    fn on_reset(&mut self, _focus: Option<&FocusContext>) {
        self.buffer.clear();
    }

    fn snapshot(&self) -> Vec<BufferedKeystroke> {
        self.buffer.snapshot()
    }

    fn len(&self) -> usize {
        self.buffer.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::PhysicalKey;

    fn stroke(code: u16) -> BufferedKeystroke {
        BufferedKeystroke {
            key: PhysicalKey::new(code, false),
            shift: false,
        }
    }

    #[test]
    fn push_backspace_and_clear_maintain_exact_sequence() {
        let mut buf = KeystrokeBuffer::new(16);
        let en = LayoutId::new("00004009");
        let focus = FocusId(1);

        // Backspace on empty is a safe no-op
        buf.pop_backspace();
        assert!(buf.is_empty());

        buf.push(stroke(0x23), en.clone(), focus);
        buf.push(stroke(0x12), en.clone(), focus);
        buf.push(stroke(0x26), en.clone(), focus);
        assert_eq!(buf.len(), 3);

        buf.pop_backspace();
        assert_eq!(buf.snapshot(), vec![stroke(0x23), stroke(0x12)]);

        buf.clear();
        assert!(buf.is_empty());
        assert_eq!(buf.burst_layout(), None);
        assert_eq!(buf.burst_focus(), None);
    }

    #[test]
    fn buffer_overflow_drops_oldest_and_keeps_tail() {
        let mut buf = KeystrokeBuffer::new(3);
        let en = LayoutId::new("00004009");
        let focus = FocusId(1);

        for code in [0x10, 0x11, 0x12, 0x13, 0x14] {
            buf.push(stroke(code), en.clone(), focus);
        }
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.snapshot(), vec![stroke(0x12), stroke(0x13), stroke(0x14)]);
    }

    #[test]
    fn keystroke_buffer_debug_never_leaks_key_contents() {
        let mut buf = KeystrokeBuffer::new(8);
        buf.push(stroke(0x23), LayoutId::new("00004009"), FocusId(7));
        let dbg = format!("{buf:?}");
        assert!(dbg.contains("len: 1"), "expected len in Debug: {dbg}");
        assert!(!dbg.contains("0x23") && !dbg.contains("35"), "Debug leaked scan code: {dbg}");
    }
}
