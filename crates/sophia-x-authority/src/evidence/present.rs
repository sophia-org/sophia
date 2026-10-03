//! Keep high-rate success evidence as bounded scalar records until the
//! session's existing metrics cadence. Failure and window-lifecycle evidence
//! is still immediate. The recent tail is also flushed at session teardown.
use super::*;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

const CAPACITY: usize = 128;
static AGGREGATE: AtomicBool = AtomicBool::new(false);
static WINDOW: Mutex<Window> = Mutex::new(Window::new());

#[cfg(test)]
#[path = "../../tests/support/present_evidence_window.rs"]
mod tests;

#[derive(Clone, Copy)]
enum Record {
    Accepted {
        client: XServerFrontendClientId,
        transaction: sophia_protocol::TransactionId,
        window: XResourceId,
        pixmap: XResourceId,
        serial: u32,
        pending: usize,
    },
    Delivery {
        client: XServerFrontendClientId,
        transaction: Option<sophia_protocol::TransactionId>,
        sequence: u16,
        event_id: XResourceId,
        window: XResourceId,
        serial: u32,
        kind: &'static str,
        pixmap: Option<XResourceId>,
        status: &'static str,
    },
}

impl Record {
    fn emit(self, observed_usec: u64) {
        match self {
            Self::Accepted {
                client,
                transaction,
                window,
                pixmap,
                serial,
                pending,
            } => {
                tracing::debug!(target: "sophia_application_evidence",
                    "sophia_x_present_submission schema=1 client={} transaction={} window_token={} pixmap_token={} serial={} pending_count={} status=accepted observed_monotonic_usec={observed_usec}",
                    client.raw(), transaction.raw(), token(window), token(pixmap), serial, pending,
                );
            }
            Self::Delivery {
                client,
                transaction,
                sequence,
                event_id,
                window,
                serial,
                kind,
                pixmap,
                status,
            } => {
                tracing::debug!(target: "sophia_application_evidence",
                    "sophia_x_present_delivery schema=1 client={} transaction={} sequence={} window_token={} subscription_token={} pixmap_token={} serial={} kind={} status={} observed_monotonic_usec={observed_usec}",
                    client.raw(), transaction.map_or(0, |id| id.raw()), sequence,
                    token(window), token(event_id), pixmap.map_or(0, token), serial, kind, status,
                );
            }
        }
    }
}

struct Window {
    recent: [Option<(Record, u64)>; CAPACITY],
    next: usize,
    attempted: u64,
    emitted: u64,
    coalesced: u64,
    accepted: u64,
    ready: u64,
    queued: u64,
    write_started: u64,
    written: u64,
}

impl Window {
    const fn new() -> Self {
        Self {
            recent: [None; CAPACITY],
            next: 0,
            attempted: 0,
            emitted: 0,
            coalesced: 0,
            accepted: 0,
            ready: 0,
            queued: 0,
            write_started: 0,
            written: 0,
        }
    }

    fn push(&mut self, record: Record, time: u64) {
        self.attempted = self.attempted.saturating_add(1);
        let counter = match record {
            Record::Accepted { .. } => &mut self.accepted,
            Record::Delivery {
                status: "ready", ..
            } => &mut self.ready,
            Record::Delivery {
                status: "queued", ..
            } => &mut self.queued,
            Record::Delivery {
                status: "write_started",
                ..
            } => &mut self.write_started,
            Record::Delivery {
                status: "written", ..
            } => &mut self.written,
            _ => unreachable!("only success records enter the diagnostic window"),
        };
        *counter = counter.saturating_add(1);
        if self.recent[self.next].is_some() {
            self.coalesced = self.coalesced.saturating_add(1);
        }
        self.recent[self.next] = Some((record, time));
        self.next = (self.next + 1) % CAPACITY;
    }
}

fn observed_usec() -> u64 {
    let clock = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    (clock.tv_sec as u64)
        .saturating_mul(1_000_000)
        .saturating_add(clock.tv_nsec as u64 / 1_000)
}

fn observe(record: Record) {
    let time = observed_usec();
    if AGGREGATE.load(Ordering::Relaxed) {
        let mut window = WINDOW.lock().unwrap_or_else(|error| error.into_inner());
        // Scope teardown disables aggregation before its final drain. An
        // observer delayed on this mutex must not append after that drain.
        if AGGREGATE.load(Ordering::Relaxed) {
            window.push(record, time);
            return;
        }
    }
    record.emit(time);
}

/// Owns the session's daily aggregation mode. Standalone authority users keep
/// full evidence by default. Explicit diagnostic sessions can do so too.
pub struct PresentEvidenceScope;

pub fn aggregate_present_evidence(enabled: bool) -> PresentEvidenceScope {
    AGGREGATE.store(enabled, Ordering::Relaxed);
    PresentEvidenceScope
}

impl Drop for PresentEvidenceScope {
    fn drop(&mut self) {
        AGGREGATE.store(false, Ordering::Relaxed);
        flush_present_evidence();
    }
}

/// Called on the existing metrics cadence and before fatal cleanup. Counters
/// are cumulative. `emitted` means handed to tracing, not persisted to disk.
/// Archive timestamps are flush time; samples retain their observation time.
pub fn flush_present_evidence() {
    let (records, next, counts) = {
        let mut window = WINDOW.lock().unwrap_or_else(|error| error.into_inner());
        let records = std::mem::replace(&mut window.recent, [None; CAPACITY]);
        let next = window.next;
        window.next = 0;
        window.emitted = window
            .emitted
            .saturating_add(records.iter().flatten().count() as u64);
        let counts = [
            window.attempted,
            window.emitted,
            window.coalesced,
            window.accepted,
            window.ready,
            window.queued,
            window.write_started,
            window.written,
        ];
        (records, next, counts)
    };
    for index in (next..CAPACITY).chain(0..next) {
        if let Some((record, time)) = records[index] {
            record.emit(time);
        }
    }
    let [
        attempted,
        emitted,
        coalesced,
        accepted,
        ready,
        queued,
        write_started,
        written,
    ] = counts;
    if attempted != 0 {
        tracing::debug!(target: "sophia_application_evidence",
            "sophia_x_present_work schema=1 attempted_count={attempted} emitted_count={emitted} coalesced_count={coalesced} accepted_count={accepted} ready_count={ready} queued_count={queued} write_started_count={write_started} written_count={written}");
    }
}

pub(crate) fn accepted(
    client: XServerFrontendClientId,
    transaction: sophia_protocol::TransactionId,
    window: XResourceId,
    pixmap: XResourceId,
    serial: u32,
    pending: usize,
) {
    observe(Record::Accepted {
        client,
        transaction,
        window,
        pixmap,
        serial,
        pending,
    });
}

pub(crate) fn delivery(
    client: XServerFrontendClientId,
    transaction: Option<sophia_protocol::TransactionId>,
    status: &'static str,
    event: XClientEvent,
) {
    let (sequence, event_id, window, serial, kind, pixmap) = match event {
        XClientEvent::PresentCompleteNotify {
            sequence,
            event_id,
            window,
            serial,
            kind,
            ..
        } => (
            sequence,
            event_id,
            window,
            serial,
            if kind == 0 { "complete" } else { "msc" },
            None,
        ),
        XClientEvent::PresentIdleNotify {
            sequence,
            event_id,
            window,
            serial,
            pixmap,
            ..
        } => (sequence, event_id, window, serial, "idle", Some(pixmap)),
        _ => return,
    };
    let record = Record::Delivery {
        client,
        transaction,
        sequence,
        event_id,
        window,
        serial,
        kind,
        pixmap,
        status,
    };
    if matches!(status, "ready" | "queued" | "write_started" | "written") {
        observe(record);
    } else {
        // Preserve all failure evidence and the recent successful correlation.
        flush_present_evidence();
        record.emit(observed_usec());
    }
}
