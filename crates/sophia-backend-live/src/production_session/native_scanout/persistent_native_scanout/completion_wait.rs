use super::*;
use std::os::fd::{AsFd, BorrowedFd};

/// Why a wait took the bounded short retry instead of an fd wait, one bit per
/// source, so the owner receipt can name the obligation behind each Service
/// deadline. Diagnostic only: `short_service` keeps the scheduling decision.
pub mod native_short_service {
    pub const FENCE_UNOBSERVED: u32 = 1 << 0;
    pub const PREPARATION_PENDING: u32 = 1 << 1;
    pub const PRESENTATION_QUEUED: u32 = 1 << 2;
    pub const SOFTWARE_FRAME_WAITING: u32 = 1 << 3;
    pub const OUTPUT_CLEANUP_PENDING: u32 = 1 << 4;
    pub const SUBMISSION_WITHOUT_DESCRIPTOR: u32 = 1 << 5;
    pub const FENCE_SIGNALED: u32 = 1 << 6;
    pub const POLLER_CALLBACKS: u32 = 1 << 7;
    pub const TOPOLOGY_DRAIN: u32 = 1 << 8;
    pub const HEAD_PENDING_CALLBACK: u32 = 1 << 9;
    pub const HEAD_PENDING_CONTENT: u32 = 1 << 10;
    pub const HEAD_RENDERING_CONTENT: u32 = 1 << 11;
    pub const HEAD_PREPARED_SCANOUT: u32 = 1 << 12;
    pub const HEAD_CLEANUP_PENDING: u32 = 1 << 13;
    pub const PREVIEW_FAILURES: u32 = 1 << 14;
    pub const TOPOLOGY_CLEANUP: u32 = 1 << 15;
    pub const DEFERRED_MIRROR: u32 = 1 << 16;
    pub const EXPORTER_PENDING_FRAME: u32 = 1 << 17;
    pub const EXPORTER_WORKER_IN_FLIGHT: u32 = 1 << 18;
    /// Receipt field suffixes, in bit order.
    pub const NAMES: [&str; 19] = [
        "fence_unobserved",
        "preparation_pending",
        "presentation_queued",
        "software_frame_waiting",
        "output_cleanup_pending",
        "submission_without_descriptor",
        "fence_signaled",
        "poller_callbacks",
        "topology_drain",
        "head_pending_callback",
        "head_pending_content",
        "head_rendering_content",
        "head_prepared_scanout",
        "head_cleanup_pending",
        "preview_failures",
        "topology_cleanup",
        "deferred_mirror",
        "exporter_pending_frame",
        "exporter_worker_in_flight",
    ];
}
use native_short_service as reason;

/// Borrowed readiness only: the existing completion pump remains the sole reader.
/// Drop this view before servicing, retiring or replacing any native owner.
#[derive(Default)]
pub struct LiveNativeCompletionWait<'a> {
    pub descriptors: Vec<BorrowedFd<'a>>,
    /// Work without a readiness contract still receives bounded short service.
    pub short_service: bool,
    /// Which `native_short_service` sources set `short_service`.
    pub short_service_reasons: u32,
    pub submissions: bool,
    pub deadline: Option<Instant>,
}

impl<'a> LiveNativeCompletionWait<'a> {
    /// Take the bounded retry when `on`, and record why.
    pub(crate) fn short_if(&mut self, on: bool, reason: u32) {
        if on {
            self.short_service = true;
            self.short_service_reasons |= reason;
        }
    }

    pub(crate) fn observe_fence(
        &mut self,
        fence: BorrowedFd<'a>,
        observed: crate::LibdrmNativeCompletionFenceStatus,
    ) {
        // A signalled sync_file stays readable even if retirement refused its
        // synthesized callback. Service it on the bounded fallback instead of
        // subscribing a permanently ready fd. Custody and watchdog stay intact.
        if observed == crate::LibdrmNativeCompletionFenceStatus::Signaled {
            self.short_if(true, reason::FENCE_SIGNALED);
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
        self.short_if(pending_callbacks, reason::POLLER_CALLBACKS);
        if active {
            // A mirror has several submitted heads but only one card reader.
            self.descriptors.push(card);
        }
    }
}

impl LiveProductionNativeScanout {
    /// Attach the owner wake to every exporter worker, once each, so a render
    /// result ends the owner's wait instead of its 1 ms fallback.
    pub fn attach_owner_wake(&self, notifier: &sophia_wake::Notifier) {
        for exporter in &self.exporters {
            exporter.attach_owner_wake(notifier);
        }
    }

    pub(crate) fn completion_wait(&self) -> LiveNativeCompletionWait<'_> {
        let mut wait = LiveNativeCompletionWait::default();
        if !self.output_topology_allows_frame_service() {
            // Topology drains have their own service deadline and ownership.
            wait.short_if(true, reason::TOPOLOGY_DRAIN);
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
            wait.short_if(
                head.pending_callback.is_some(),
                reason::HEAD_PENDING_CALLBACK,
            );
            wait.short_if(head.pending_content.is_some(), reason::HEAD_PENDING_CONTENT);
            wait.short_if(
                head.rendering_content.is_some(),
                reason::HEAD_RENDERING_CONTENT,
            );
            wait.short_if(
                head.prepared_scanout.is_some(),
                reason::HEAD_PREPARED_SCANOUT,
            );
            wait.short_if(
                head.scanout_custody.cleanup_pending(),
                reason::HEAD_CLEANUP_PENDING,
            );
        }
        wait.short_if(!self.preview_failures.is_empty(), reason::PREVIEW_FAILURES);
        wait.short_if(
            !self.output_topology_cleanup.is_empty(),
            reason::TOPOLOGY_CLEANUP,
        );
        wait.short_if(
            self.logical_outputs
                .iter()
                .any(|output| self.deferred_mirror_generations.pending(output.id)),
            reason::DEFERRED_MIRROR,
        );
        wait.short_if(
            self.exporters
                .iter()
                .any(|exporter| exporter.pending_frame()),
            reason::EXPORTER_PENDING_FRAME,
        );
        wait.short_if(
            self.exporters
                .iter()
                .any(|exporter| exporter.worker_in_flight()),
            reason::EXPORTER_WORKER_IN_FLIGHT,
        );
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

    /// Cumulative renderer-worker thread CPU over every card group, for the
    /// owner's diagnostic cost rows; not a scheduling input.
    #[cfg(feature = "diagnostic-owner-cost")]
    pub fn render_worker_cpu_ns(&self) -> u64 {
        // Without shared workers every head owns its core and the group slot
        // stays empty (cpu-decomp-01 read zero here); with sharing, heads
        // attach one core, so count groups to avoid double counting.
        if Self::shared_renderer_worker_enabled() {
            self.groups
                .iter()
                .filter_map(|group| group.renderer_core.as_ref())
                .fold(0_u64, |sum, core| sum.saturating_add(core.cpu_ns()))
        } else {
            self.exporters.iter().fold(0_u64, |sum, exporter| {
                sum.saturating_add(exporter.worker_cpu_ns())
            })
        }
    }
}
