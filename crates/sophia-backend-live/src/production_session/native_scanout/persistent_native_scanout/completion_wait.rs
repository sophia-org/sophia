use super::*;
use std::os::fd::{AsFd, BorrowedFd};

/// Borrowed readiness only: the existing completion pump remains the sole reader.
/// Drop this view before servicing, retiring or replacing any native owner.
#[derive(Default)]
pub struct LiveNativeCompletionWait<'a> {
    pub descriptors: Vec<BorrowedFd<'a>>,
    /// Work without a readiness contract still receives bounded short service.
    pub short_service: bool,
    pub submissions: bool,
    pub deadline: Option<Instant>,
}

impl<'a> LiveNativeCompletionWait<'a> {
    pub(crate) fn observe_fence(
        &mut self,
        fence: BorrowedFd<'a>,
        observed: crate::LibdrmNativeCompletionFenceStatus,
    ) {
        // A signalled sync_file stays readable even if retirement refused its
        // synthesized callback. Service it on the bounded fallback instead of
        // subscribing a permanently ready fd. Custody and watchdog stay intact.
        if observed == crate::LibdrmNativeCompletionFenceStatus::Signaled {
            self.short_service = true;
        } else {
            self.descriptors.push(fence);
        }
    }

    pub(super) fn observe_group(
        &mut self,
        card: BorrowedFd<'a>,
        submissions: impl Iterator<Item = Instant>,
        pending_callbacks: bool,
    ) {
        let mut active = false;
        for started in submissions {
            active = true;
            let deadline = started + LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL;
            self.deadline = Some(self.deadline.map_or(deadline, |old| old.min(deadline)));
        }
        self.submissions |= active;
        self.short_service |= pending_callbacks;
        if active {
            // A mirror has several submitted heads but only one card reader.
            self.descriptors.push(card);
        }
    }
}

impl LiveProductionNativeScanout {
    pub(crate) fn completion_wait(&self) -> LiveNativeCompletionWait<'_> {
        let mut wait = LiveNativeCompletionWait::default();
        if !self.output_topology_allows_frame_service() {
            // Topology drains have their own service deadline and ownership.
            wait.short_service = true;
            return wait;
        }
        for (group, owner) in self.groups.iter().enumerate() {
            wait.observe_group(
                owner.session.card().as_fd(),
                self.heads
                    .iter()
                    .filter(|head| head.group == group)
                    .filter_map(|head| head.submitted_at),
                owner
                    .session
                    .page_flip_poller_diagnostics()
                    .pending_callbacks
                    != 0,
            );
        }
        for head in &self.heads {
            if let Some(fence) = head
                .scanout_custody
                .submitted()
                .and_then(crate::LiveRenderedPrimaryPlaneScanoutSubmission::completion_fence)
            {
                wait.observe_fence(fence, head.completion_fence_status);
            }
            wait.short_service |= head.pending_callback.is_some()
                || head.pending_content.is_some()
                || head.rendering_content.is_some()
                || head.prepared_scanout.is_some()
                || head.scanout_custody.cleanup_pending();
        }
        wait.short_service |= !self.preview_failures.is_empty()
            || !self.output_topology_cleanup.is_empty()
            || self
                .logical_outputs
                .iter()
                .any(|output| self.deferred_mirror_generations.pending(output.id))
            || self
                .exporters
                .iter()
                .any(|exporter| exporter.pending_frame() || exporter.worker_in_flight());
        wait
    }

    pub(crate) fn completion_fence_observation(
        &self,
        output: OutputId,
    ) -> Option<crate::LibdrmNativeCompletionFenceStatus> {
        self.primary_head_index(output)
            .map(|index| self.heads[index].completion_fence_status)
    }

    /// Successful reader drains and out-fence retirements, for wake attribution.
    /// This is not presentation permission or a count of displayed frames.
    pub fn completion_progress(&self) -> (u64, u64) {
        let reads = self.groups.iter().fold(0_u64, |sum, group| {
            let d = group.session.page_flip_poller_cumulative_diagnostics();
            sum.saturating_add(
                d.read_calls
                    .saturating_sub(d.would_block_reads)
                    .saturating_sub(d.read_failures) as u64,
            )
        });
        let progress = self.heads.iter().fold(reads, |sum, head| {
            sum.saturating_add(head.out_fence_retirements as u64)
        });
        (self.retirement_owner_identity(), progress)
    }
}
