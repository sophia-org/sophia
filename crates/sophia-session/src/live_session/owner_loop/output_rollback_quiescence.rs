/// How long a rollback may wait for candidate presentation ownership to settle
/// before its blocking restoration commit. Measured from the first rollback
/// turn, not from preparation: peer loss can arrive after the candidate's
/// first frames were submitted, and those flips must retire first.
const OUTPUT_TOPOLOGY_ROLLBACK_QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(2);

/// Spacing between quiescence turns. A turn that is not due is skipped, not
/// slept; the owner loop idles until `next_wake`.
const OUTPUT_TOPOLOGY_ROLLBACK_QUIESCENCE_INTERVAL: Duration = Duration::from_millis(1);

/// One rollback's wait for ordinary presentation ownership to settle before
/// the reverse apply. Once ready it stays ready: later rollback turns (one per
/// card) go straight to the effect, and frame service remains gated meanwhile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OutputTopologyRollbackQuiescence {
    started: Instant,
    deadline: Instant,
    next_service: Instant,
    ready: bool,
}

#[derive(Debug, Eq, PartialEq)]
enum OutputTopologyRollbackStep<T> {
    /// Ownership has not settled; the reverse apply was not attempted.
    Pending,
    /// Ownership settled (now or earlier) and the reverse apply ran.
    Effect(T),
}

impl OutputTopologyRollbackQuiescence {
    fn new(now: Instant) -> Self {
        Self::until(now, now + OUTPUT_TOPOLOGY_ROLLBACK_QUIESCENCE_TIMEOUT)
    }

    /// A wait bounded by an existing deadline, for session completion, which
    /// already owns one for the whole topology abort.
    fn until(now: Instant, deadline: Instant) -> Self {
        Self {
            started: now,
            deadline,
            next_service: now,
            ready: false,
        }
    }

    /// Runs `apply` only after `quiesce` has reported settled ownership before
    /// the deadline. Past the deadline neither closure runs, so a late ready
    /// report cannot admit the reverse apply; readiness latched earlier still
    /// carries later cards. A quiescence error or an expired deadline returns
    /// an error, leaving every tracker as it was. Both closures receive the
    /// same owners in turn, so neither has to hold them while the other runs.
    fn turn<C: ?Sized, T>(
        &mut self,
        now: Instant,
        owners: &mut C,
        quiesce: impl FnOnce(&mut C) -> Result<bool, Box<dyn std::error::Error>>,
        apply: impl FnOnce(&mut C) -> Result<T, Box<dyn std::error::Error>>,
    ) -> Result<OutputTopologyRollbackStep<T>, Box<dyn std::error::Error>> {
        if !self.ready {
            if now >= self.deadline {
                return Err(format!(
                    "output topology rollback quiescence timed out after {} ms: candidate presentation ownership remained in flight",
                    now.saturating_duration_since(self.started).as_millis(),
                )
                .into());
            }
            if now < self.next_service {
                return Ok(OutputTopologyRollbackStep::Pending);
            }
            if !quiesce(owners)? {
                self.next_service = now
                    .checked_add(OUTPUT_TOPOLOGY_ROLLBACK_QUIESCENCE_INTERVAL)
                    .unwrap_or(now)
                    .min(self.deadline);
                return Ok(OutputTopologyRollbackStep::Pending);
            }
            self.ready = true;
        }
        apply(owners).map(OutputTopologyRollbackStep::Effect)
    }

    /// When the next quiescence turn is due, while still waiting.
    fn next_wake(&self) -> Option<Instant> {
        (!self.ready).then_some(self.next_service)
    }
}
