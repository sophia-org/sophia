use super::{
    LiveProductionPresentLayoutState, LiveProductionPresentScheduler, SurfaceId,
    SurfaceTransactionKey, TransactionId,
};
use std::time::{Duration, Instant};

/// How many candidates one surface may hold parked before the oldest are
/// settled immediately.
///
/// Mesa's DRI3 loader keeps at most four back buffers for a drawable, so a
/// conforming client cannot have more than that outstanding and never reaches
/// this. It bounds a client that does not wait for its buffers, without
/// inventing a second way to unwind a candidate: the overflow takes the
/// ordinary rejection the tick would have given it, only sooner.
const FRAME_TICK_PARKED_PER_SURFACE: usize = 8;

impl LiveProductionPresentScheduler {
    /// Hold a candidate that cannot reach a screen until the head's next
    /// refresh, rather than settling it in the pass its Present arrived in.
    ///
    /// An onscreen client is paced by retirement: its Present completes from a
    /// real page flip, so it draws at the head's rate. A client whose window no
    /// head can carry was settled synchronously instead, which paced it by
    /// nothing at all -- it redrew as fast as it could render, and every one of
    /// those frames cost owner-loop present handling, a protocol completion and
    /// its evidence. Parking to the tick gives the invisible client the same
    /// cadence as the visible one.
    ///
    /// The deadline is shared. Candidates parked inside one interval settle
    /// together at that tick in queue order, which is how a burst behaves on a
    /// vblank, rather than each starting an interval of its own and stretching
    /// a flood across many ticks.
    ///
    /// Returns whether the front candidate was the one named and was parked.
    pub fn defer_to_frame_tick(
        &mut self,
        candidate: SurfaceTransactionKey,
        now: Instant,
        interval: Duration,
    ) -> bool {
        let deadline = match self.frame_tick {
            Some(deadline) if deadline > now => deadline,
            _ => now + interval.max(Duration::from_micros(1)),
        };
        let Some(queued) = self.queued.front() else {
            return false;
        };
        if queued.candidate.key() != candidate || !queued.runnable() {
            return false;
        }
        let mut queued = self
            .queued
            .pop_front()
            .expect("the front candidate was just inspected");
        queued.layout_state = LiveProductionPresentLayoutState::AwaitingFrameTick { deadline };
        // To the back, because `poll_gate` moves each newly eligible candidate
        // to the front: left in place, parked candidates would stack up in
        // reverse arrival order, and both the tick and the overflow bound
        // would take the newest first. This keeps the front of the queue the
        // runnable set, which is what `poll_gate` scans.
        self.queued.push_back(queued);
        self.frame_tick = Some(deadline);
        self.paced_skips = self.paced_skips.saturating_add(1);
        self.observe_frame_tick_depth();
        true
    }

    /// Yield the parked candidates whose tick has come, for the caller to
    /// settle the ordinary way.
    ///
    /// Removed rather than returned to the runnable queue. The verdict that
    /// parked them was already reached against a composed scene, and re-driving
    /// them would lower head frames again only to reject them again. What the
    /// wait bought is the pacing, not a second chance at visibility: a surface
    /// that becomes visible presents a *newer* buffer, which enters the queue
    /// runnable and is composed normally.
    pub fn release_frame_tick(&mut self, now: Instant) -> Vec<TransactionId> {
        let Some(deadline) = self.frame_tick else {
            return Vec::new();
        };
        if now < deadline {
            return Vec::new();
        }
        let mut released = Vec::new();
        let mut retained = std::collections::VecDeque::with_capacity(self.queued.len());
        for queued in self.queued.drain(..) {
            match queued.layout_state {
                LiveProductionPresentLayoutState::AwaitingFrameTick { deadline }
                    if now >= deadline =>
                {
                    released.push(queued.submission.transaction);
                }
                _ => retained.push_back(queued),
            }
        }
        self.queued = retained;
        self.frame_tick = self.earliest_frame_tick();
        if !released.is_empty() {
            self.observe_queue_depth();
        }
        released
    }

    /// Settle the oldest parked candidates of a surface that has accumulated
    /// more than one refresh interval's worth of them.
    ///
    /// A client that keeps presenting without waiting for its buffers would
    /// otherwise grow the queue for as long as it stays invisible. The X-side
    /// per-client pending bound already refuses such a client, but that bound
    /// is reached by stalling its next request; this keeps the scheduler's own
    /// queue proportional to what the pacing actually holds.
    pub fn bound_frame_tick_parking(&mut self, surface: SurfaceId) -> Vec<TransactionId> {
        let parked = self
            .queued
            .iter()
            .filter(|queued| queued.surface == surface && queued.awaiting_frame_tick())
            .count();
        let Some(excess) = parked.checked_sub(FRAME_TICK_PARKED_PER_SURFACE) else {
            return Vec::new();
        };
        if excess == 0 {
            return Vec::new();
        }
        let mut remaining = excess;
        let mut released = Vec::new();
        let mut retained = std::collections::VecDeque::with_capacity(self.queued.len());
        for queued in self.queued.drain(..) {
            if remaining != 0 && queued.surface == surface && queued.awaiting_frame_tick() {
                remaining -= 1;
                released.push(queued.submission.transaction);
            } else {
                retained.push_back(queued);
            }
        }
        self.queued = retained;
        self.frame_tick = self.earliest_frame_tick();
        self.frame_tick_overflows = self.frame_tick_overflows.saturating_add(released.len());
        self.observe_queue_depth();
        released
    }

    /// When the owner must next wake to settle parked candidates.
    pub fn frame_tick_deadline(&self) -> Option<Instant> {
        self.frame_tick
    }

    pub fn frame_tick_parked(&self) -> usize {
        self.queued
            .iter()
            .filter(|queued| queued.awaiting_frame_tick())
            .count()
    }

    pub const fn paced_skips(&self) -> usize {
        self.paced_skips
    }

    pub const fn max_frame_tick_parked(&self) -> usize {
        self.max_frame_tick_parked
    }

    pub const fn frame_tick_overflows(&self) -> usize {
        self.frame_tick_overflows
    }

    pub(super) fn earliest_frame_tick(&self) -> Option<Instant> {
        self.queued
            .iter()
            .filter_map(|queued| match queued.layout_state {
                LiveProductionPresentLayoutState::AwaitingFrameTick { deadline } => Some(deadline),
                _ => None,
            })
            .min()
    }

    fn observe_frame_tick_depth(&mut self) {
        self.max_frame_tick_parked = self.max_frame_tick_parked.max(self.frame_tick_parked());
        self.observe_queue_depth();
    }
}
