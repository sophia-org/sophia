use std::collections::VecDeque;

/// One FIFO owns complete encoded records through the last successful byte.
/// Partial writes release neither the record slot nor its retained allocation.
#[derive(Default)]
pub(super) struct ShellOutbox {
    frames: VecDeque<OwnedFrame>,
    bytes: usize,
    controls: usize,
}

struct OwnedFrame {
    bytes: Box<[u8]>,
    written: usize,
    control: bool,
}

impl ShellOutbox {
    pub(super) fn len(&self) -> usize {
        self.bytes
    }

    pub(super) fn records(&self) -> usize {
        self.frames.len()
    }

    pub(super) fn bulk_bytes(&self) -> usize {
        self.frames
            .iter()
            .filter(|frame| !frame.control)
            .map(|frame| frame.bytes.len())
            .sum()
    }

    pub(super) fn controls(&self) -> usize {
        self.controls
    }

    pub(super) fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub(super) fn clear(&mut self) {
        self.frames.clear();
        self.bytes = 0;
        self.controls = 0;
    }

    pub(super) fn push(&mut self, bytes: Vec<u8>, control: bool) {
        if bytes.is_empty() {
            return;
        }
        let bytes = bytes.into_boxed_slice();
        let length = bytes.len();
        self.frames.push_back(OwnedFrame {
            bytes,
            written: 0,
            control,
        });
        self.bytes += length;
        self.controls += usize::from(control);
    }

    pub(super) fn front(&self) -> &[u8] {
        self.frames
            .front()
            .map_or(&[], |frame| &frame.bytes[frame.written..])
    }

    pub(super) fn written(&mut self, count: usize) {
        let frame = self
            .frames
            .front_mut()
            .expect("write requires an owned frame");
        assert!(count <= frame.bytes.len() - frame.written);
        frame.written += count;
        if frame.written == frame.bytes.len() {
            self.bytes -= frame.bytes.len();
            self.controls -= usize::from(frame.control);
            self.frames.pop_front();
        }
    }
}

#[cfg(test)]
#[path = "../../tests/support/shell_outbox.rs"]
mod tests;
