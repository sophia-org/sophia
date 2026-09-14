//! Bounded starts and elapsed accounting for an ordered input owner.
//!
//! The caller supplies monotonic time from one origin. Time spent queued,
//! deliberately delayed, frozen, or waiting for a service interval is outside
//! an operation. Once started, guard acquisition and application both count.
//! These are start limits, not preemption: an operation can overrun its
//! allowance, and finishing reports that fact separately.

use std::time::Duration;

/// The supported service allowance and its cleanup reservation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceLimits {
    /// Minimum interval between resets of a consumed allowance.
    pub interval: Duration,
    /// Maximum operation starts in one interval.
    pub starts: u32,
    /// Elapsed operation time after which no more work starts.
    pub charge: Duration,
    /// Starts kept for eligible cleanup before new work may use them.
    pub cleanup_starts: u32,
    /// Elapsed allowance kept for eligible cleanup at each start decision.
    pub cleanup_charge: Duration,
}

impl ServiceLimits {
    /// The M3 service contract, independent of queue capacity.
    pub const PLANNED: Self = Self {
        interval: Duration::from_millis(16),
        starts: 32,
        charge: Duration::from_millis(2),
        cleanup_starts: 4,
        cleanup_charge: Duration::from_micros(500),
    };

    fn verify(self) -> Result<(), InvalidServiceLimits> {
        if self.interval.is_zero()
            || self.starts == 0
            || self.charge.is_zero()
            || self.charge > self.interval
            || self.cleanup_starts > self.starts
            || self.cleanup_charge > self.charge
        {
            Err(InvalidServiceLimits)
        } else {
            Ok(())
        }
    }
}

/// An allowance was empty or its reservation exceeded its total.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidServiceLimits;

/// Whether this start serves already-owed cleanup or new work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceWork {
    /// New admitted operations and their ordered control operations.
    NewWork,
    /// Work that discharges an existing native or recipient obligation.
    Cleanup,
}

/// What the owner established about eligible cleanup for this decision.
///
/// This answer is supplied again for every start, so donation after an empty
/// scan cannot silently persist when cleanup subsequently becomes eligible.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CleanupReadiness {
    /// Keep the unconsumed cleanup allowance available.
    Eligible,
    /// The owner established that no cleanup can start on this turn.
    NoneEligible,
}

/// Why this operation cannot start. No refusal consumes a start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceStartRefusal {
    /// Supplied time moved backwards within this budget's clock domain.
    ClockRegressed,
    /// An operation lost its accounting guard. This budget never reopens.
    Interrupted,
    /// Every permitted start was used.
    StartsExhausted { retry_after: Duration },
    /// The total elapsed allowance was used or exceeded.
    TimeExhausted { retry_after: Duration },
    /// Only the starts reserved for eligible cleanup remain.
    CleanupStartsReserved { retry_after: Duration },
    /// Only elapsed allowance reserved for eligible cleanup remains.
    CleanupTimeReserved { retry_after: Duration },
}

/// Failure to account an operation; none grants permission for another start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceAccountingError {
    /// The finishing time predates a time already supplied by the caller.
    ClockRegressed,
    /// Recovery accounting was requested on a healthy budget.
    NotInterrupted,
    /// The interrupted operation was already accounted.
    NothingUnaccounted,
    /// Elapsed accounting could not be represented. The budget stays closed.
    ArithmeticOverflow,
}

/// Read-only accounting for the current service interval.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ServiceUsage {
    /// All starts, including an operation that did not finish.
    pub starts: u32,
    /// Cleanup starts, also including an unfinished cleanup operation.
    pub cleanup_starts: u32,
    /// Elapsed time of operations whose accounting finished.
    pub charged: Duration,
    /// The cleanup part of that elapsed time.
    pub cleanup_charged: Duration,
}

/// What one finished accounting operation establishes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceCharge {
    /// Time from its start through guard acquisition and its last effect.
    pub elapsed: Duration,
    /// Total interval charge after this operation.
    pub interval_charge: Duration,
    /// How far the total charge exceeds the interval's elapsed allowance.
    pub allowance_overrun: Duration,
    /// A new operation ran into time reserved for eligible cleanup.
    ///
    /// A start limit cannot preempt it. This measures the overrun instead of
    /// claiming that the elapsed cleanup reservation survived an arbitrary
    /// operation duration. It is zero for cleanup or a donated reservation.
    pub cleanup_reservation_overrun: Duration,
    /// How far the finishing time is past the start interval's boundary.
    ///
    /// The entire operation is accounted to its starting interval before any
    /// reset. Crossing an interval is not an erasure of an unfinished charge.
    pub interval_boundary_overrun: Duration,
}

#[derive(Clone, Copy, Debug)]
struct ActiveService {
    began: Duration,
    work: ServiceWork,
    new_work_ceiling: Option<Duration>,
}

/// One continuing budget owned by one execution runner.
///
/// There is no reset method. A new interval begins on the first scheduling
/// decision at least `interval` after the previous interval began; unused
/// intervals do not accumulate credit. A running operation keeps its original
/// interval through accounting. Dropping its guard latches failure permanently
/// so a later rollover cannot conceal unaccounted execution.
#[derive(Debug)]
pub struct ServiceBudget {
    limits: ServiceLimits,
    interval_began: Duration,
    last_now: Duration,
    usage: ServiceUsage,
    active: Option<ActiveService>,
    interrupted: bool,
}

impl ServiceBudget {
    /// Create a budget using caller-supplied monotonic time.
    pub fn new(now: Duration, limits: ServiceLimits) -> Result<Self, InvalidServiceLimits> {
        limits.verify()?;
        Ok(Self {
            limits,
            interval_began: now,
            last_now: now,
            usage: ServiceUsage::default(),
            active: None,
            interrupted: false,
        })
    }

    /// Create the fixed M3 budget.
    pub fn planned(now: Duration) -> Self {
        Self::new(now, ServiceLimits::PLANNED).expect("the planned allowance is valid")
    }

    /// The limits validated at construction.
    pub fn limits(&self) -> ServiceLimits {
        self.limits
    }

    /// A snapshot, never a mutable reference to the counters.
    pub fn usage(&self) -> ServiceUsage {
        self.usage
    }

    /// Whether execution lost the operation that would finish accounting.
    pub fn is_interrupted(&self) -> bool {
        self.interrupted
    }

    /// Start one operation without allocating, blocking, or reading a clock.
    ///
    /// Cleanup can consume the whole remaining allowance. New work preserves
    /// any unconsumed cleanup reservation unless the owner establishes that
    /// none is eligible for this decision. The owner retains its fair cursor;
    /// this budget does not choose which source or cleanup runs next.
    pub fn start(
        &mut self,
        now: Duration,
        work: ServiceWork,
        cleanup: CleanupReadiness,
    ) -> Result<ServiceRun<'_>, ServiceStartRefusal> {
        // Even a deliberately forgotten guard cannot overwrite its active
        // record with another operation or clear it through window rollover.
        if self.interrupted || self.active.is_some() {
            self.interrupted = true;
            return Err(ServiceStartRefusal::Interrupted);
        }
        if now < self.last_now {
            return Err(ServiceStartRefusal::ClockRegressed);
        }
        // A successful clock check changes no allowance by itself.
        self.last_now = now;
        let elapsed = now - self.interval_began;
        if elapsed >= self.limits.interval {
            self.interval_began = now;
            self.usage = ServiceUsage::default();
        }
        let retry_after = self.limits.interval - (now - self.interval_began);
        if self.usage.starts >= self.limits.starts {
            return Err(ServiceStartRefusal::StartsExhausted { retry_after });
        }
        if self.usage.charged >= self.limits.charge {
            return Err(ServiceStartRefusal::TimeExhausted { retry_after });
        }
        let reserved = work == ServiceWork::NewWork && cleanup == CleanupReadiness::Eligible;
        let new_work_ceiling = if reserved {
            let starts_owed = self
                .limits
                .cleanup_starts
                .saturating_sub(self.usage.cleanup_starts);
            if self.usage.starts >= self.limits.starts - starts_owed {
                return Err(ServiceStartRefusal::CleanupStartsReserved { retry_after });
            }
            let charge_owed = self
                .limits
                .cleanup_charge
                .saturating_sub(self.usage.cleanup_charged);
            let ceiling = self.limits.charge - charge_owed;
            if self.usage.charged >= ceiling {
                return Err(ServiceStartRefusal::CleanupTimeReserved { retry_after });
            }
            Some(ceiling)
        } else {
            None
        };
        // Each increment is below a verified u32 bound, so it cannot wrap.
        self.usage.starts += 1;
        if work == ServiceWork::Cleanup {
            self.usage.cleanup_starts += 1;
        }
        self.active = Some(ActiveService {
            began: now,
            work,
            new_work_ceiling,
        });
        Ok(ServiceRun {
            budget: self,
            finished: false,
        })
    }

    /// Account time retained by an interrupted operation without reopening it.
    ///
    /// The caller must first establish that the operation can no longer run.
    /// This is accounting only: it proves no native effect, transport outcome,
    /// or completion. The runner still requires its failure/teardown path.
    pub fn account_interrupted(
        &mut self,
        now: Duration,
    ) -> Result<ServiceCharge, ServiceAccountingError> {
        if !self.interrupted {
            return Err(ServiceAccountingError::NotInterrupted);
        }
        self.finish_active(now)
    }

    fn finish_active(&mut self, now: Duration) -> Result<ServiceCharge, ServiceAccountingError> {
        let active = self
            .active
            .ok_or(ServiceAccountingError::NothingUnaccounted)?;
        if now < self.last_now {
            return Err(ServiceAccountingError::ClockRegressed);
        }
        let elapsed = now - active.began;
        let total = self
            .usage
            .charged
            .checked_add(elapsed)
            .ok_or(ServiceAccountingError::ArithmeticOverflow)?;
        let cleanup_total = if active.work == ServiceWork::Cleanup {
            self.usage
                .cleanup_charged
                .checked_add(elapsed)
                .ok_or(ServiceAccountingError::ArithmeticOverflow)?
        } else {
            self.usage.cleanup_charged
        };
        let charge = ServiceCharge {
            elapsed,
            interval_charge: total,
            allowance_overrun: total.saturating_sub(self.limits.charge),
            cleanup_reservation_overrun: active
                .new_work_ceiling
                .map_or(Duration::ZERO, |ceiling| total.saturating_sub(ceiling)),
            interval_boundary_overrun: (now - self.interval_began)
                .saturating_sub(self.limits.interval),
        };
        self.usage.charged = total;
        self.usage.cleanup_charged = cleanup_total;
        self.last_now = now;
        self.active = None;
        Ok(charge)
    }
}

/// The exclusive right to finish the one operation this budget started.
///
/// Dropping it, including during unwind, closes the budget permanently. The
/// start and its time remain in the budget for explicit failure accounting.
#[must_use = "a started operation must be accounted or the runner stays interrupted"]
#[derive(Debug)]
pub struct ServiceRun<'a> {
    budget: &'a mut ServiceBudget,
    finished: bool,
}

impl ServiceRun<'_> {
    /// Finish accounting once, retaining any overrun as a measured fact.
    pub fn finish(mut self, now: Duration) -> Result<ServiceCharge, ServiceAccountingError> {
        let result = self.budget.finish_active(now);
        self.finished = result.is_ok();
        result
    }
}

impl Drop for ServiceRun<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.budget.interrupted = true;
        }
    }
}

/// Phase information for an independent executor watchdog.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionPhase {
    /// Dequeued, including any wait to acquire execution guards.
    BeforeGuards,
    /// Entering code that can apply an effect. Marked before that code runs.
    Applying,
    /// The operation established its commit; post-commit work may still stall.
    Committed,
}

/// An invalid phase advance or a time outside the supplied clock domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchdogError {
    /// A phase was skipped, repeated, or advanced after finishing.
    InvalidTransition,
    /// Supplied time predates dequeue or a previous observation.
    ClockRegressed,
}

/// A watchdog observation. None of these is an input completion or receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchdogObservation {
    /// No timeout has elapsed yet.
    Running {
        phase: ExecutionPhase,
        remaining: Duration,
    },
    /// The deadline elapsed, including at its exact boundary.
    Expired {
        phase: ExecutionPhase,
        overdue: Duration,
    },
    /// Execution returned; retain whether it had exceeded the allowance.
    Finished {
        phase: ExecutionPhase,
        elapsed: Duration,
        exceeded_deadline: bool,
    },
}

/// A deadline beginning at actual execution dequeue, before guard acquisition.
///
/// The caller must run supervision independently of the executor and its
/// locks. This helper creates no thread, reads no clock, and performs no
/// transport or lifecycle action. Construct it only when budget/delay/frozen
/// waiting has ended; never reset it when a phase changes.
#[derive(Debug)]
pub struct ExecutionWatchdog {
    dequeued: Duration,
    last_now: Duration,
    phase: ExecutionPhase,
    finished: Option<Duration>,
}

impl ExecutionWatchdog {
    /// M3 executor allowance, separate from recipient socket blockage.
    pub const DEADLINE: Duration = Duration::from_millis(250);

    /// Begin at execution dequeue using the same monotonic clock as checks.
    pub fn dequeued(now: Duration) -> Self {
        Self {
            dequeued: now,
            last_now: now,
            phase: ExecutionPhase::BeforeGuards,
            finished: None,
        }
    }

    /// Record entry into effectful work before performing it.
    pub fn applying(&mut self) -> Result<(), WatchdogError> {
        if self.finished.is_some() || self.phase != ExecutionPhase::BeforeGuards {
            return Err(WatchdogError::InvalidTransition);
        }
        self.phase = ExecutionPhase::Applying;
        Ok(())
    }

    /// Record an established commit without extending the deadline.
    pub fn committed(&mut self) -> Result<(), WatchdogError> {
        if self.finished.is_some() || self.phase != ExecutionPhase::Applying {
            return Err(WatchdogError::InvalidTransition);
        }
        self.phase = ExecutionPhase::Committed;
        Ok(())
    }

    /// Record return, including a pre-effect refusal, with its elapsed time.
    pub fn finish(&mut self, now: Duration) -> Result<WatchdogObservation, WatchdogError> {
        if self.finished.is_some() {
            return Err(WatchdogError::InvalidTransition);
        }
        self.check_clock(now)?;
        self.finished = Some(now - self.dequeued);
        self.observe(now)
    }

    /// Read timeout and phase without deriving an input outcome from either.
    pub fn observe(&mut self, now: Duration) -> Result<WatchdogObservation, WatchdogError> {
        self.check_clock(now)?;
        if let Some(elapsed) = self.finished {
            return Ok(WatchdogObservation::Finished {
                phase: self.phase,
                elapsed,
                exceeded_deadline: elapsed >= Self::DEADLINE,
            });
        }
        let elapsed = now - self.dequeued;
        if elapsed >= Self::DEADLINE {
            Ok(WatchdogObservation::Expired {
                phase: self.phase,
                overdue: elapsed - Self::DEADLINE,
            })
        } else {
            Ok(WatchdogObservation::Running {
                phase: self.phase,
                remaining: Self::DEADLINE - elapsed,
            })
        }
    }

    fn check_clock(&mut self, now: Duration) -> Result<(), WatchdogError> {
        if now < self.last_now {
            return Err(WatchdogError::ClockRegressed);
        }
        self.last_now = now;
        Ok(())
    }
}
