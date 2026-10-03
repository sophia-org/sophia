//! Scalar retirement observations; no image, buffer or lease is retained.
//! Routine records are formatted on the existing metrics cadence, with an
//! exact recent tail also available on failure. Explicit proofs stay immediate.
use super::*;

const CAPACITY: usize = 32;

#[cfg(test)]
#[path = "../../../tests/support/native_present_evidence.rs"]
mod tests;

#[derive(Clone, Copy)]
pub(super) enum Record {
    Mixed {
        transaction: TransactionId,
        surface: SurfaceId,
        source: Size,
        target: Rect,
        clip: Option<Rect>,
        stable: bool,
        nonzero_rgb_pixels: usize,
        ust: u64,
        msc: u64,
    },
    Software(sophia_backend_live::LiveProductionRetiredSoftwarePresent),
}

impl Record {
    fn lines(self) -> u64 {
        match self {
            Self::Mixed { .. } => 2,
            Self::Software(_) => 1,
        }
    }

    fn emit(self) {
        match self {
            Self::Mixed {
                transaction,
                surface,
                source,
                target,
                clip,
                stable,
                nonzero_rgb_pixels,
                ust,
                msc,
            } => {
                let clip = clip.map_or_else(
                    || "none".to_owned(),
                    |clip| format!("{}x{}_{}_{}", clip.width, clip.height, clip.x, clip.y),
                );
                crate::session_println!(
                    "sophia_live_session_present schema=2 status=retired transaction={} surface={} source={}x{} target={}x{}_{}_{} clip={} unit_scale={} ust={ust} msc={msc}",
                    transaction.raw(),
                    surface.index(),
                    source.width,
                    source.height,
                    target.width,
                    target.height,
                    target.x,
                    target.y,
                    clip,
                    source.width == target.width && source.height == target.height,
                );
                crate::session_println!(
                    "sophia_live_session_scanout schema=2 status={} kind=mixed transaction={} nonzero_rgb_pixels={nonzero_rgb_pixels}",
                    if stable { "stable" } else { "superseded" },
                    transaction.raw(),
                );
            }
            Self::Software(retired) => {
                crate::session_println!(
                    "sophia_live_session_present schema=4 status=retired transaction={} surface={} source={}x{} kind=software frame={} native_submission={} ust={} msc={}",
                    retired.candidate.transaction.raw(),
                    retired.candidate.surface.index(),
                    retired.source_size.width,
                    retired.source_size.height,
                    retired.frame.raw(),
                    retired.native_submission,
                    retired.ust_usec,
                    retired.msc,
                );
            }
        }
    }
}

pub(crate) struct NativePresentEvidence {
    aggregate: bool,
    recent: [Option<Record>; CAPACITY],
    next: usize,
    retired: u64,
    attempted: u64,
    emitted: u64,
    coalesced: u64,
}

impl NativePresentEvidence {
    pub(crate) fn new(aggregate: bool) -> Self {
        Self {
            aggregate,
            recent: [None; CAPACITY],
            next: 0,
            retired: 0,
            attempted: 0,
            emitted: 0,
            coalesced: 0,
        }
    }

    pub(super) fn record(&mut self, record: Record, exact: bool) {
        self.retired = self.retired.saturating_add(1);
        self.attempted = self.attempted.saturating_add(record.lines());
        if !self.aggregate || exact {
            self.emitted = self.emitted.saturating_add(record.lines());
            record.emit();
            return;
        }
        if let Some(old) = self.recent[self.next].replace(record) {
            self.coalesced = self.coalesced.saturating_add(old.lines());
        }
        self.next = (self.next + 1) % CAPACITY;
    }

    pub(crate) fn flush(&mut self) {
        for index in (self.next..CAPACITY).chain(0..self.next) {
            if let Some(record) = self.recent[index].take() {
                self.emitted = self.emitted.saturating_add(record.lines());
                record.emit();
            }
        }
        self.next = 0;
        if self.retired != 0 {
            // Counts describe producer records; the archive still reports
            // its own queue and storage drops. Samples carry kernel UST,
            // while archive timestamps identify this flush.
            crate::session_println!(
                "sophia_live_present_work schema=1 retired_count={} attempted_count={} emitted_count={} coalesced_count={}",
                self.retired,
                self.attempted,
                self.emitted,
                self.coalesced,
            );
        }
    }
}

impl Drop for NativePresentEvidence {
    fn drop(&mut self) {
        self.flush();
    }
}
