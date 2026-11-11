//! Test-only support for the combined credit seam: a probe plan, a validator
//! checkpoint and fixture helpers. No rendered lease or retirement proof.
#![cfg(test)]

use super::*;

// Only the test instantiation is concrete; transport code cannot inspect P.
pub(super) struct ProbePlan {
    id: u64,
    descriptor: OwnedFd,
    lifetime: Arc<()>,
}

type Worker = IsbValidationWorker<ProbePlan>;

const CHECKPOINT_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

pub(super) struct Checkpoint {
    entered: mpsc::SyncSender<()>,
    resume: mpsc::Receiver<()>,
}

/// Holds the real validator before validation until the fixture releases it.
pub(super) fn park<P>(route: Option<&mut Route<P>>) {
    if let Some(checkpoint) = route.and_then(|route| route.checkpoint.take()) {
        checkpoint
            .entered
            .send(())
            .expect("checkpoint owner present");
        checkpoint
            .resume
            .recv_timeout(CHECKPOINT_WAIT)
            .expect("fixture checkpoint deliberately disconnected or timed out");
    }
}

/// An inbox whose wake has no owner; forwarding tests that need the ring hold
/// their own `Wake`.
fn inbox<P>(sender: mpsc::SyncSender<ForwardedValidation<P>>) -> CombinedInbox<P> {
    let wake = sophia_wake::Wake::new().expect("fixture wake");
    CombinedInbox::new(sender, ring(&wake))
}

fn ring(wake: &sophia_wake::Wake) -> sophia_wake::WakeSlot {
    let slot = sophia_wake::WakeSlot::default();
    slot.set(wake.notifier());
    // Attaching rings once by design; tests count only forwarding rings.
    wake.clear().expect("consume the attach ring");
    slot
}

impl<P: Send + 'static> IsbValidationWorker<P> {
    fn submit_combined_checkpoint(
        &mut self,
        reservation: &CombinedReservation,
        source: OwnedFd,
        plan: P,
        sender: mpsc::SyncSender<ForwardedValidation<P>>,
        checkpoint: Option<Checkpoint>,
    ) -> Result<mpsc::Receiver<P>, (IsbSubmitFailure, OwnedFd, P)> {
        let (mut job, returned_plan) = self
            .combined_job(reservation, Source::Isb(source), plan, inbox(sender))
            .map_err(|(failure, source, plan)| (failure, source.into_isb(), plan))?;
        job.combined
            .as_mut()
            .expect("combined job carries its route")
            .checkpoint = checkpoint;
        self.send_combined(job)
            .map(|()| returned_plan)
            .map_err(|(failure, source, plan)| (failure, source.into_isb(), plan))
    }
}

#[path = "combined_frame_credit/tests.rs"]
mod tests;

#[path = "combined_frame_credit/payload_tests.rs"]
mod payload_tests;

#[path = "combined_frame_credit/wake_tests.rs"]
mod wake_tests;
