/// Total time one topology transaction may spend preparing its candidate and
/// rollback renderer resources, measured on the monotonic clock from
/// `begin_output_topology_preparation`.
///
/// Each prepared head is one GPU composition, normally ready within a frame.
/// Five seconds allows progress beyond the renderer worker's one-second
/// hard-stall report and ends indefinite slot deferral independently of the
/// client's lifetime or deadline.
/// This deadline does not replace worker stall handling: it refuses a
/// preparation that stops making progress (an export that keeps deferring,
/// for example for want of a frame slot), and hands it to the existing abort
/// drain. An export already running on a worker is never dropped; the drain
/// waits for it, and a worker that never returns is the abandonment bound's
/// business (`LIVE_RENDERER_WORKER_STALL_ABANDON`).
const LIVE_PRODUCTION_TOPOLOGY_PREPARATION_LIMIT: std::time::Duration =
    std::time::Duration::from_secs(5);

/// Minimum spacing between export retries while preparation or its abort
/// drain waits on renderer work. Retries are skipped, never slept: the owner
/// loop decides how to idle until `next_service`.
const LIVE_PRODUCTION_TOPOLOGY_PREPARATION_SERVICE_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LiveProductionNativeTopologyPreparationTurn {
    /// Poll the renderer now.
    Service,
    /// The previous poll was less than one interval ago; `next_service`
    /// says when the next one is due.
    Wait,
    /// Candidate and rollback preparation together exceeded the limit.
    Expired { elapsed: std::time::Duration },
}

/// Deadline and retry pacing for one preparation. Owned by the preparation
/// state and dropped with it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct LiveProductionNativeTopologyPreparationBudget {
    started: std::time::Instant,
    deadline: std::time::Instant,
    next_service: std::time::Instant,
}

impl LiveProductionNativeTopologyPreparationBudget {
    pub(super) fn new(started: std::time::Instant) -> Self {
        Self::with_limit(started, LIVE_PRODUCTION_TOPOLOGY_PREPARATION_LIMIT)
    }

    pub(super) fn with_limit(started: std::time::Instant, limit: std::time::Duration) -> Self {
        Self {
            started,
            // An unrepresentable deadline is no deadline at all; refuse it
            // rather than letting preparation wait forever.
            deadline: started.checked_add(limit).unwrap_or(started),
            next_service: started,
        }
    }

    /// Decides one owner-loop turn for `phase`. Only the two preparing phases
    /// carry the deadline. The abort drain is paced but never expires, because
    /// it must not drop an export a worker still owns. Every other phase is
    /// serviced immediately, as before.
    pub(super) fn turn(
        &mut self,
        phase: LiveProductionNativeTopologyPreparationPhase,
        now: std::time::Instant,
    ) -> LiveProductionNativeTopologyPreparationTurn {
        use LiveProductionNativeTopologyPreparationPhase as Phase;
        let deadline_applies =
            matches!(phase, Phase::PreparingCandidate | Phase::PreparingRollback);
        let paced = deadline_applies || phase == Phase::Aborting;
        if deadline_applies && now >= self.deadline {
            return LiveProductionNativeTopologyPreparationTurn::Expired {
                elapsed: now.saturating_duration_since(self.started),
            };
        }
        if !paced {
            return LiveProductionNativeTopologyPreparationTurn::Service;
        }
        if now < self.next_service {
            return LiveProductionNativeTopologyPreparationTurn::Wait;
        }
        self.next_service = now
            .checked_add(LIVE_PRODUCTION_TOPOLOGY_PREPARATION_SERVICE_INTERVAL)
            .unwrap_or(now);
        LiveProductionNativeTopologyPreparationTurn::Service
    }

    /// The earliest instant the owner loop needs to call the service again for
    /// `phase`, or `None` when it is not paced. A caller may idle until then.
    pub(super) fn next_service(
        &self,
        phase: LiveProductionNativeTopologyPreparationPhase,
    ) -> Option<std::time::Instant> {
        use LiveProductionNativeTopologyPreparationPhase as Phase;
        match phase {
            Phase::PreparingCandidate | Phase::PreparingRollback => {
                Some(self.next_service.min(self.deadline))
            }
            Phase::Aborting => Some(self.next_service),
            _ => None,
        }
    }
}
