use super::{
    LiveProductionPresentLayoutState, LiveProductionPresentScheduler, SurfaceId,
    SurfaceTransactionKey, TransactionId,
};
use std::time::{Duration, Instant};

/// Background pacing may occupy at most one quarter of the default global
/// presentation registry. Pressure releases oldest background debt; it must
/// not consume the capacity needed by visible and first-visibility work.
const FRAME_TICK_PARKED_CAPACITY: usize = 64;

impl LiveProductionPresentScheduler {
    /// Hold a candidate that cannot reach a screen until the caller's pacing
    /// interval, rather than settling it in the pass its Present arrived in.
    ///
    /// An onscreen client is paced by retirement: its Present completes from a
    /// real page flip, so it draws at the head's rate. A client whose window no
    /// head can carry was settled synchronously instead, which paced it by
    /// nothing at all -- it redrew as fast as it could render, and every one of
    /// those frames cost owner-loop present handling, a protocol completion and
    /// its evidence. Parking bounds this background work independently of refresh.
    ///
    /// The deadline is shared. Candidates parked inside one interval settle
    /// together at that tick in queue order, which is how a burst behaves on a
    /// vblank, rather than each starting an interval of its own and stretching
    /// a flood across many ticks.
    ///
    /// Every queued candidate already owns a registration in the bounded
    /// presentation resource registry (256 by default). The X frontend also
    /// backpressures each client at 64 pending Presents in a live Session.
    /// A separate global pressure bound reserves room for visible work. A
    /// many-buffer client below that bound cannot bypass the pacing merely
    /// by crossing an arbitrary per-surface count.
    ///
    /// Returns whether the front candidate was the one named and was parked.
    pub fn defer_to_frame_tick(
        &mut self,
        candidate: SurfaceTransactionKey,
        now: Instant,
        interval: Duration,
        visible_at_park: bool,
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
        queued.layout_state = LiveProductionPresentLayoutState::AwaitingFrameTick {
            deadline,
            observed_hidden: !visible_at_park,
        };
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
        self.release_frame_tick_after_visibility(now, None)
    }

    /// Visibility removes background pacing immediately. These candidates
    /// already had a Skip verdict: settle it once and free their buffers so
    /// the client can produce a current visible frame. First-admission debt
    /// uses the separate first-visibility queue and is never skipped here.
    pub fn release_frame_tick_or_visible(
        &mut self,
        now: Instant,
        visible: &[SurfaceId],
    ) -> Vec<TransactionId> {
        self.release_frame_tick_after_visibility(now, Some(visible))
    }

    fn release_frame_tick_after_visibility(
        &mut self,
        now: Instant,
        visible: Option<&[SurfaceId]>,
    ) -> Vec<TransactionId> {
        if self.frame_tick.is_none_or(|deadline| now < deadline) && visible.is_none() {
            return Vec::new();
        }
        let mut released = Vec::new();
        let mut retained = std::collections::VecDeque::with_capacity(self.queued.len());
        for mut queued in self.queued.drain(..) {
            if let LiveProductionPresentLayoutState::AwaitingFrameTick {
                deadline,
                observed_hidden,
            } = &mut queued.layout_state
            {
                let visible_now = visible.map(|surfaces| surfaces.contains(&queued.surface));
                // Parking is decided by actual capture, while this predicate
                // observes scene visibility. A disagreement must not turn
                // every owner pass into an immediate completion. Only an
                // observed hidden -> visible edge releases before the tick.
                if now >= *deadline || (*observed_hidden && visible_now == Some(true)) {
                    released.push(queued.submission.transaction);
                    continue;
                }
                *observed_hidden |= visible_now == Some(false);
            }
            retained.push_back(queued);
        }
        self.queued = retained;
        self.frame_tick = self.earliest_frame_tick();
        if !released.is_empty() {
            self.observe_queue_depth();
        }
        released
    }

    pub fn awaiting_frame_tick(
        &self,
    ) -> impl Iterator<Item = (SurfaceId, sophia_protocol::Rect)> + '_ {
        self.queued
            .iter()
            .filter(|queued| queued.awaiting_frame_tick())
            .map(|queued| (queued.surface, queued.candidate.target_geometry))
    }

    /// Release only excess global background debt. This is an explicit
    /// overload exception to pacing, not a claim to rate-limit an unlimited
    /// producer. Keep every remaining candidate's original deadline.
    pub fn bound_frame_tick_parking(&mut self) -> Vec<TransactionId> {
        let excess = self
            .frame_tick_parked()
            .saturating_sub(FRAME_TICK_PARKED_CAPACITY);
        self.release_frame_tick_pressure(excess)
    }

    pub fn release_frame_tick_pressure(&mut self, mut excess: usize) -> Vec<TransactionId> {
        let mut released = Vec::new();
        self.queued.retain(|queued| {
            if excess != 0 && queued.awaiting_frame_tick() {
                excess -= 1;
                released.push(queued.submission.transaction);
                false
            } else {
                true
            }
        });
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
                LiveProductionPresentLayoutState::AwaitingFrameTick { deadline, .. } => {
                    Some(deadline)
                }
                _ => None,
            })
            .min()
    }

    fn observe_frame_tick_depth(&mut self) {
        self.max_frame_tick_parked = self.max_frame_tick_parked.max(self.frame_tick_parked());
        self.observe_queue_depth();
    }
}
