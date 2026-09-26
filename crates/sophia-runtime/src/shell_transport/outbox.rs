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
    /// None for a socket frame; otherwise the file event kind whose body
    /// `bytes` holds. The journal supplies that record's header.
    file_kind: Option<sophia_protocol::shell_files::ShellFileKind>,
    /// Bytes charged to the queue: the whole record the wire will carry.
    charged: usize,
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
            .map(|frame| frame.charged)
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
            file_kind: None,
            charged: length,
        });
        self.bytes += length;
        self.controls += usize::from(control);
    }

    /// Queues one file event body. It leaves only as a whole journal record.
    pub(super) fn push_file(
        &mut self,
        kind: sophia_protocol::shell_files::ShellFileKind,
        body: Vec<u8>,
        control: bool,
    ) {
        let charged = body.len() + sophia_protocol::shell_files::SHELL_FILE_HEADER_BYTES;
        self.frames.push_back(OwnedFrame {
            bytes: body.into_boxed_slice(),
            written: 0,
            control,
            file_kind: Some(kind),
            charged,
        });
        self.bytes += charged;
        self.controls += usize::from(control);
    }

    /// The front file event: kind, body and whether it holds a credit.
    pub(super) fn front_file(
        &self,
    ) -> Option<(sophia_protocol::shell_files::ShellFileKind, &[u8], bool)> {
        self.frames.front().and_then(|frame| {
            frame
                .file_kind
                .map(|kind| (kind, &frame.bytes[..], frame.control))
        })
    }

    /// Releases the front file event after the journal took custody of it.
    pub(super) fn pop_file(&mut self) {
        let frame = self.frames.pop_front().expect("file event queued");
        assert!(frame.file_kind.is_some());
        self.bytes -= frame.charged;
        self.controls -= usize::from(frame.control);
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
            self.bytes -= frame.charged;
            self.controls -= usize::from(frame.control);
            self.frames.pop_front();
        }
    }
}

#[cfg(test)]
#[path = "../../tests/support/shell_outbox.rs"]
mod tests;
