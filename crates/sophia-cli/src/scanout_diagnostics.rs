//! Forward approved backend evidence without collecting application output.
use std::fmt::{self, Write};

use sophia_session::diagnostics::{DIAGNOSTIC_RECORD_MAX_BYTES, capture_line, recording};
use tracing::{Event, Subscriber, field::Visit};
use tracing_subscriber::{Layer, Registry, filter::filter_fn, layer::Context};

const SCANOUT_TARGET: &str = "sophia_scanout_evidence";

pub(crate) fn layer() -> impl Layer<Registry> {
    ScanoutDiagnostics.with_filter(filter_fn(|metadata| {
        matches!(
            metadata.target(),
            SCANOUT_TARGET | "sophia_application_evidence"
        )
    }))
}

struct ScanoutDiagnostics;

impl<S: Subscriber> Layer<S> for ScanoutDiagnostics {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        if !recording() {
            return;
        }
        let mut message = Message::new();
        event.record(&mut message);
        let line = message.as_str();
        if (event.metadata().target() == "sophia_application_evidence"
            && matches!(
                line.split_whitespace().next(),
                Some(
                    "sophia_x_window_lifecycle"
                        | "sophia_x_present_submission"
                        | "sophia_x_present_delivery"
                )
            ))
            || (event.metadata().target() == SCANOUT_TARGET
                && matches!(
                    line.split_whitespace().next(),
                    Some(
                        "sophia_live_atomic_test"
                            | "sophia_live_layout_probe"
                            | "sophia_shell_native_binding"
                            | "sophia_shell_native_completion"
                    )
                ))
        {
            capture_line(line);
        }
    }
}

struct Message {
    bytes: [u8; DIAGNOSTIC_RECORD_MAX_BYTES + 1],
    len: usize,
}

impl Message {
    fn new() -> Self {
        Self {
            bytes: [b' '; DIAGNOSTIC_RECORD_MAX_BYTES + 1],
            len: 0,
        }
    }

    fn as_str(&self) -> &str {
        // Writes end at UTF-8 boundaries; the unused suffix contains ASCII spaces.
        std::str::from_utf8(&self.bytes[..self.len]).expect("message retains valid UTF-8")
    }

    fn refuse(&mut self) {
        // One excess byte lets Capture count the whole record as discarded.
        self.len = self.bytes.len();
    }
}

impl Write for Message {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let remaining = DIAGNOSTIC_RECORD_MAX_BYTES.saturating_sub(self.len);
        let mut end = value.len().min(remaining);
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        self.bytes[self.len..self.len + end].copy_from_slice(&value.as_bytes()[..end]);
        self.len += end;
        if end != value.len() {
            self.refuse();
            return Err(fmt::Error);
        }
        Ok(())
    }
}

impl Visit for Message {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        if field.name() == "message" && write!(self, "{value:?}").is_err() {
            self.refuse();
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" && self.write_str(value).is_err() {
            self.refuse();
        }
    }
}
