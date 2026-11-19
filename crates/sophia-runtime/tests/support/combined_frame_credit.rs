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

// G3-5d d-2a: the hold moved to test-support (combined_frame_hold.rs).
use super::hold::{CHECKPOINT_WAIT, Checkpoint};

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
            .combined_job(reservation, Source::Isb(source), None, plan, inbox(sender))
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

#[path = "combined_frame_credit/hold_tests.rs"]
mod hold_tests;
