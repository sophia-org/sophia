//! G3-5d d-2a, test-support only: the renderer gate (design REV3 section 1).
//! It holds the worker on one output's forwarded combined job after the
//! validator has validated and forwarded it and before the route serves it,
//! so a control can let the job's deadline pass or stop it there. The gate
//! changes nothing the route decides with: the job is parked unchanged, with
//! its token, pixels, plan, deadline and stop flag, and `serve` then reads
//! them with a fresh instant.
use super::super::super::combined::OutputRoute;
use super::*;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

/// What the worker tells the control, in this order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GateFact {
    /// The worker read the install command; the gate is armed.
    Armed,
    /// A pausing gate found its output's route empty and paused there once,
    /// before deciding not to serve the route this pass.
    Waiting,
    /// A forwarded job reached the gated output's route and is held: its
    /// validator job number, its admitted deadline and its stop flag.
    Entered {
        job: u64,
        deadline: Instant,
        stopped: bool,
    },
    /// The hold ended. The facts are read again just before `serve`, which
    /// runs with `now`.
    Released {
        job: u64,
        now: Instant,
        deadline: Instant,
        stopped: bool,
        by: GateRelease,
    },
}

/// How a hold ended. Only `Released` is a control's own action: disconnect
/// is the cleanup escape when the control's handle drops, and a timeout is a
/// missing release, never a pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GateRelease {
    Released,
    Disconnected,
    TimedOut,
}

/// The worker's half: armed for one output and one job.
pub(crate) struct CombinedGate {
    output: LiveRendererWorkerOutputKey,
    wait: Duration,
    facts: mpsc::Sender<GateFact>,
    release: mpsc::Receiver<()>,
    /// For a pausing gate, until its one pause: where it waits on an empty
    /// route.
    resume: Option<mpsc::Receiver<()>>,
}

/// The control's half. Dropping it releases a hold as `Disconnected`.
// Only this crate's tests construct the control half until the d-2b fixture.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct GateHandle {
    facts: mpsc::Receiver<GateFact>,
    release: Option<mpsc::SyncSender<()>>,
    resume: Option<mpsc::SyncSender<()>>,
}

/// A gate for `output` whose hold ends at the latest after `wait`.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn combined_gate(
    output: LiveRendererWorkerOutputKey,
    wait: Duration,
) -> (CombinedGate, GateHandle) {
    gate(output, wait, false)
}

/// `combined_gate` that also pauses once, bounded by `wait`, the first time
/// it finds its output's route empty, so a control can forward a job inside
/// the empty-intake decision.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn pausing_combined_gate(
    output: LiveRendererWorkerOutputKey,
    wait: Duration,
) -> (CombinedGate, GateHandle) {
    gate(output, wait, true)
}

#[cfg_attr(not(test), allow(dead_code))]
fn gate(
    output: LiveRendererWorkerOutputKey,
    wait: Duration,
    pausing: bool,
) -> (CombinedGate, GateHandle) {
    let (facts_in, facts) = mpsc::channel();
    let (release, release_out) = mpsc::sync_channel(1);
    let (resume, resume_out) = if pausing {
        let (resume, resume_out) = mpsc::sync_channel(1);
        (Some(resume), Some(resume_out))
    } else {
        (None, None)
    };
    (
        CombinedGate {
            output,
            wait,
            facts: facts_in,
            release: release_out,
            resume: resume_out,
        },
        GateHandle {
            facts,
            release: Some(release),
            resume,
        },
    )
}

impl CombinedGate {
    pub(super) fn armed(&self) {
        // A control that already dropped its handle reads nothing.
        let _ = self.facts.send(GateFact::Armed);
    }
}

#[cfg_attr(not(test), allow(dead_code))]
impl GateHandle {
    /// The worker's next fact, or None after `wait`.
    pub(crate) fn next(&self, wait: Duration) -> Option<GateFact> {
        self.facts.recv_timeout(wait).ok()
    }

    /// Drops the release side only, so a hold ends as `Disconnected` while
    /// the control still reads the facts: the cleanup path, observed.
    pub(crate) fn disconnect(&mut self) {
        self.release = None;
    }

    /// Ends a pausing gate's pause, once.
    pub(crate) fn resume(&mut self) {
        if let Some(resume) = self.resume.take() {
            let _ = resume.try_send(());
        }
    }

    /// Ends the hold as `Released`, once.
    pub(crate) fn release(&mut self) {
        if let Some(release) = self.release.take() {
            // The gate holds at most one job and reads one release.
            let _ = release.try_send(());
        }
    }
}

/// Called by `serve_combined` before the output's route serves; one intake
/// decision for the gated output. Not armed for `output`: serve with `now`.
/// Armed and the route holds a forwarded job: park it unchanged, report it,
/// wait for the release, disarm and serve with a fresh instant. Armed and
/// the route is empty: `None`, so the route is not served this pass; a job
/// forwarded after the probe stays in the inbox, its ring wakes the worker,
/// and the next pass meets the gate with it.
pub(in super::super) fn hold(
    slot: &mut Option<CombinedGate>,
    output: LiveRendererWorkerOutputKey,
    route: &mut OutputRoute,
    now: Instant,
) -> Option<Instant> {
    let Some(armed) = slot.as_mut().filter(|gate| gate.output == output) else {
        return Some(now);
    };
    if !route.await_job(Duration::ZERO) {
        if let Some(resume) = armed.resume.take() {
            let _ = armed.facts.send(GateFact::Waiting);
            let _ = resume.recv_timeout(armed.wait);
        }
        return None;
    }
    let Some(gate) = slot.take() else {
        return Some(now);
    };
    let Some((job, deadline, stopped)) = route.parked_facts() else {
        return Some(now);
    };
    let _ = gate.facts.send(GateFact::Entered {
        job,
        deadline,
        stopped,
    });
    let by = match gate.release.recv_timeout(gate.wait) {
        Ok(()) => GateRelease::Released,
        Err(RecvTimeoutError::Disconnected) => GateRelease::Disconnected,
        Err(RecvTimeoutError::Timeout) => GateRelease::TimedOut,
    };
    let (job, deadline, stopped) = route
        .parked_facts()
        .expect("the gate holds the parked job until serve");
    let fresh = Instant::now();
    let _ = gate.facts.send(GateFact::Released {
        job,
        now: fresh,
        deadline,
        stopped,
        by,
    });
    Some(fresh)
}
