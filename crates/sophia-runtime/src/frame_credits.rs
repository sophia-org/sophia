//! W3 pacing-credit lifecycle reused by the native FrameCommit experiment.
//!
//! Imported from the reviewed ledger checkpoint `33ff35990`; not authority or image admission.
//!
//! This experimental ledger has no wire or Session caller yet. The owner must
//! validate custody and reserve responses before committing a spend, and publish
//! terminal outcomes before committing replenishment. Callbacks make failures
//! leave credits unchanged. The existing candidate store still owns resource
//! leases, supersession, W2 cancellation, and response accounting.

use std::collections::VecDeque;

use sophia_protocol::{ContentAllocationId, ContentGrant, ContentOutputId, ContentReason};

use crate::ContentCandidateError;

/// Exact authority tuple for one stream; it never follows a replacement output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameStreamKey {
    pub grant: ContentGrant,
    pub output: ContentOutputId,
    pub allocation: ContentAllocationId,
    pub facts_generation: u64,
    pub stream_id: u64,
}

impl FrameStreamKey {
    fn valid(self) -> bool {
        self.grant.connection_epoch != 0
            && self.grant.content_grant_epoch != 0
            && self.output.id != 0
            && self.output.generation != 0
            && self.allocation.id != 0
            && self.allocation.generation != 0
            && self.facts_generation != 0
            && self.stream_id != 0
    }
}

/// Closing rejects admissions while prior candidates and the close notice drain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameStreamPhase {
    Closed,
    Open,
    Closing,
}

/// Exactly one close notice, backed by the reservation made at stream open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameStreamClose {
    pub key: FrameStreamKey,
    pub reason: ContentReason,
}

/// At most two pacing credits for the qualified two-slot bar experiment.
///
/// Times are server monotonic milliseconds. This is not a rate limiter: fast
/// terminal outcomes can recycle credits immediately. A connection owner retains
/// this ledger across stream reopen so old stream/candidate ids cannot replay.
#[derive(Debug)]
pub struct FrameCredits {
    grant: ContentGrant,
    key: Option<FrameStreamKey>,
    phase: FrameStreamPhase,
    available: VecDeque<u64>,
    outstanding: Vec<u64>,
    pending_close: Option<FrameStreamClose>,
    last_stream_id: u64,
    last_candidate: u64,
    last_now: u64,
    ttl_ms: u32,
    poisoned: bool,
}

impl FrameCredits {
    /// Create an empty per-grant ledger with the existing permit TTL.
    pub fn new(grant: ContentGrant, ttl_ms: u32) -> Result<Self, ContentCandidateError> {
        if grant.connection_epoch == 0 || grant.content_grant_epoch == 0 || ttl_ms == 0 {
            return Err(ContentCandidateError::Malformed);
        }
        Ok(Self {
            grant,
            key: None,
            phase: FrameStreamPhase::Closed,
            available: VecDeque::with_capacity(2),
            outstanding: Vec::with_capacity(2),
            pending_close: None,
            last_stream_id: 0,
            last_candidate: 0,
            last_now: 0,
            ttl_ms,
            poisoned: false,
        })
    }

    /// Reserve the open reply and one lifetime close notice before opening.
    ///
    /// The callback must also refuse outstanding demand/permit/assembly state and
    /// uncommitted old settlements. It runs only after local validation succeeds.
    pub fn open(
        &mut self,
        key: FrameStreamKey,
        credits: u8,
        now: u64,
        reserve: impl FnOnce() -> Result<(), ContentCandidateError>,
    ) -> Result<(), ContentCandidateError> {
        self.expire(now)?;
        if !key.valid()
            || key.grant != self.grant
            || key.stream_id <= self.last_stream_id
            || self.phase != FrameStreamPhase::Closed
        {
            return Err(ContentCandidateError::Stale);
        }
        if !(1..=2).contains(&credits) {
            return Err(ContentCandidateError::Malformed);
        }
        let deadline = self.deadline(now)?;
        reserve()?;
        self.key = Some(key);
        self.last_stream_id = key.stream_id;
        self.available
            .extend(std::iter::repeat_n(deadline, usize::from(credits)));
        self.phase = FrameStreamPhase::Open;
        Ok(())
    }

    /// Spend the oldest credit only after the candidate's full admission commits.
    ///
    /// The callback validates admission and reserves every response obligation,
    /// including any supersession outcome. It must leave pending candidates
    /// untouched on error. Only after success may the owner cancel a predecessor
    /// and report its terminal outcome. Budget refusal spends identity, not credit.
    pub fn admit(
        &mut self,
        key: FrameStreamKey,
        candidate: u64,
        now: u64,
        reserve: impl FnOnce() -> Result<(), ContentCandidateError>,
    ) -> Result<(), ContentCandidateError> {
        self.expire(now)?;
        self.check_key(key)?;
        if self.phase != FrameStreamPhase::Open || candidate <= self.last_candidate {
            return Err(ContentCandidateError::Stale);
        }
        self.last_candidate = candidate;
        if self.available.is_empty() {
            return Err(ContentCandidateError::Budget);
        }
        reserve()?;
        self.available.pop_front();
        self.outstanding.push(candidate);
        Ok(())
    }

    /// Commit a terminal outcome, then return exactly one credit if still open.
    ///
    /// Prepared must never call this method. The callback publishes the existing
    /// terminal outcome and its optional replenishment deadline in one commit;
    /// errors leave the candidate outstanding. The deadline is anchored here,
    /// never at client receipt or ACK. Closing outcomes replenish nothing.
    pub fn terminal(
        &mut self,
        key: FrameStreamKey,
        candidate: u64,
        now: u64,
        commit: impl FnOnce(Option<u64>) -> Result<(), ContentCandidateError>,
    ) -> Result<(), ContentCandidateError> {
        // A clock fault closes admission, but cannot strand prior settlements.
        let clock = self.expire(now);
        if self.phase != FrameStreamPhase::Closing {
            clock?;
        }
        self.check_key(key)?;
        let index = self
            .outstanding
            .iter()
            .position(|id| *id == candidate)
            .ok_or(ContentCandidateError::Stale)?;
        let deadline = if self.phase == FrameStreamPhase::Open {
            Some(self.deadline(now)?)
        } else {
            None
        };
        commit(deadline)?;
        self.outstanding.remove(index);
        if let Some(deadline) = deadline {
            self.available.push_back(deadline);
        }
        self.finish_close();
        Ok(())
    }

    /// Close on explicit cancellation, revocation or tuple invalidation.
    /// Already admitted candidates still owe their ordinary terminal outcomes.
    pub fn close(
        &mut self,
        key: FrameStreamKey,
        reason: ContentReason,
    ) -> Result<(), ContentCandidateError> {
        self.check_key(key)?;
        if self.phase != FrameStreamPhase::Open {
            return Err(ContentCandidateError::Stale);
        }
        self.start_close(reason);
        Ok(())
    }

    /// Observe available-credit expiry from the owner's existing expiry sweep.
    /// Clock regression permanently prevents this grant from opening again.
    pub fn expire(&mut self, now: u64) -> Result<(), ContentCandidateError> {
        if self.poisoned || now < self.last_now {
            self.poisoned = true;
            self.start_close(ContentReason::Malformed);
            return Err(ContentCandidateError::ClockRegression);
        }
        self.last_now = now;
        if self.phase == FrameStreamPhase::Open
            && self
                .available
                .front()
                .is_some_and(|deadline| now >= *deadline)
        {
            self.start_close(ContentReason::Timeout);
        }
        Ok(())
    }

    /// Publish the single reserved close notice; failed commits may be retried.
    pub fn publish_close(
        &mut self,
        commit: impl FnOnce(FrameStreamClose) -> Result<(), ContentCandidateError>,
    ) -> Result<(), ContentCandidateError> {
        let notice = self.pending_close.ok_or(ContentCandidateError::Stale)?;
        commit(notice)?;
        self.pending_close = None;
        self.finish_close();
        Ok(())
    }

    pub fn phase(&self) -> FrameStreamPhase {
        self.phase
    }

    pub fn available(&self) -> usize {
        self.available.len()
    }

    pub fn outstanding(&self) -> usize {
        self.outstanding.len()
    }

    pub fn pending_close(&self) -> Option<FrameStreamClose> {
        self.pending_close
    }

    fn deadline(&self, now: u64) -> Result<u64, ContentCandidateError> {
        now.checked_add(u64::from(self.ttl_ms))
            .ok_or(ContentCandidateError::Malformed)
    }

    fn check_key(&self, key: FrameStreamKey) -> Result<(), ContentCandidateError> {
        if self.key != Some(key) {
            return Err(ContentCandidateError::Stale);
        }
        Ok(())
    }

    fn start_close(&mut self, reason: ContentReason) {
        if self.phase == FrameStreamPhase::Open {
            self.available.clear();
            self.phase = FrameStreamPhase::Closing;
            self.pending_close = self.key.map(|key| FrameStreamClose { key, reason });
        }
    }

    fn finish_close(&mut self) {
        if self.phase == FrameStreamPhase::Closing
            && self.pending_close.is_none()
            && self.outstanding.is_empty()
        {
            self.phase = FrameStreamPhase::Closed;
        }
    }
}
