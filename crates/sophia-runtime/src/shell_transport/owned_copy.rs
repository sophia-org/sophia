//! Experimental ring proposals share the persistent image owner and worker.
//!
//! Offers and completions propose work; this owner accepts transitions against
//! the current export and registry. Journal publication is a separate next
//! action, retained under backpressure. No completion authorizes a resource.
use super::{ShellComponentTransport, ShellTransportError};
use crate::owned_ring_protocol::{Identity, Offer};
use crate::{
    ContentEpochRegistry, IsbImageWithdrawal, IsbValidationCompletion, IsbValidationWorker,
    OwnedCopyImageJobs, OwnedCopyImageSettlement, OwnedRing,
};
use sophia_protocol::ContentReason;
use std::time::Instant;

mod admission;
#[cfg(test)]
#[path = "../../tests/support/shell_owned_copy_owner.rs"]
mod tests;
#[cfg(all(test, target_os = "linux"))]
#[path = "../../tests/support/owned_ring_transport.rs"]
mod wire_tests;

struct ActiveRing {
    identity: Identity,
    ring: OwnedRing,
}

/// A successful reservation owns one table slot until its release is journaled
/// or its old journal is gone. Preserve full ring identity across reconnects.
enum Work {
    Working {
        identity: Identity,
        job: u64,
        withdrawn: bool,
        handed_off: bool,
    },
    Releasing {
        identity: Identity,
        settlement: OwnedCopyImageSettlement,
        handed_off: bool,
    },
}

#[derive(Clone, Copy)]
enum Decision {
    AwaitingAdmission,
    Refusing(ContentReason),
}

#[derive(Clone, Copy)]
struct Waiting {
    offer: Offer,
    deadline: Instant,
    decision: Decision,
}

#[derive(Default)]
pub(super) struct OwnedCopyOwner {
    active: Option<ActiveRing>,
    jobs: OwnedCopyImageJobs,
    work: [Option<Work>; 4],
    waiting: Option<Waiting>,
    setup_deadline: Option<Instant>,
}

impl OwnedCopyOwner {
    pub fn pending(&self) -> usize {
        self.work.iter().flatten().count()
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.jobs
            .deadline()
            .into_iter()
            .chain(self.setup_deadline)
            .chain(self.waiting.and_then(|waiting| {
                matches!(waiting.decision, Decision::AwaitingAdmission).then_some(waiting.deadline)
            }))
            .min()
    }

    pub fn receive(
        &mut self,
        completion: IsbValidationCompletion,
    ) -> Result<(), IsbValidationCompletion> {
        self.jobs.receive(completion)
    }

    pub fn service(
        &mut self,
        transport: &mut ShellComponentTransport,
        epochs: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker,
        now: u64,
        instant: Instant,
        other_pending: usize,
    ) -> Result<(), ShellTransportError> {
        let current = transport
            .files_mut()
            .and_then(|files| files.export().owned_current_identity());
        if self.active.as_ref().map(|active| active.identity) != current {
            // In-flight read tickets retain the descriptor and source charge.
            self.active = None;
        }
        self.setup_deadline = None;
        if let Some(files) = transport.files_mut() {
            let before = files.export().owned_publication_sequence();
            let setup = files.export_mut().service_owned_ring_setup(epochs, instant);
            if before != files.export().owned_publication_sequence() {
                files.image_publication_pending();
            }
            match setup {
                Ok(Some((identity, ring))) => {
                    self.active = Some(ActiveRing { identity, ring });
                }
                Ok(None) | Err(sophia_9p::Errno::EAGAIN) => {}
                Err(error) => return Err(export_error("setup", error)),
            }
            if self.active.is_none() {
                self.setup_deadline = files.export().owned_deadline();
            }
        }
        let current = transport
            .files_mut()
            .and_then(|files| files.export().owned_current_identity());
        for entry in self.work.iter_mut().flatten() {
            if let Work::Working {
                identity,
                job,
                withdrawn,
                ..
            } = entry
                && Some(*identity) != current
                && !*withdrawn
            {
                self.jobs
                    .withdraw(epochs, worker, *job, IsbImageWithdrawal::ChannelLost, now)?;
                *withdrawn = true;
            }
        }
        self.jobs.observe_store_stops(epochs, worker, now)?;
        self.jobs.expire(epochs, worker, instant, now)?;
        self.reconcile(epochs, now);
        self.publish_releases(transport, current)?;
        self.propose_offer(transport, epochs, worker, now, instant, other_pending)
    }

    fn reconcile(&mut self, epochs: &mut ContentEpochRegistry, now: u64) {
        while let Some(settlement) = self.jobs.reconcile(epochs, now) {
            let entry = self
                .work
                .iter_mut()
                .find(|entry| {
                    matches!(entry, Some(Work::Working { job, .. })
                        if *job == settlement.admission.worker_job)
                })
                .expect("owned job retains its originating ring");
            let Some(Work::Working {
                identity,
                handed_off,
                ..
            }) = entry.take()
            else {
                unreachable!("matched working job");
            };
            *entry = Some(Work::Releasing {
                identity,
                settlement,
                handed_off,
            });
        }
    }

    fn publish_releases(
        &mut self,
        transport: &mut ShellComponentTransport,
        current: Option<Identity>,
    ) -> Result<(), ShellTransportError> {
        crate::shell_work_cost::result(crate::shell_work_cost::Stage::SourceRelease, 0, || {
            for entry in &mut self.work {
                let Some(Work::Releasing {
                    identity,
                    settlement,
                    handed_off,
                }) = entry
                else {
                    continue;
                };
                if Some(*identity) != current {
                    // Reconciliation consumed the exact retired resource result.
                    // A replacement journal never inherits its release event.
                    *entry = None;
                    continue;
                }
                let Some(active) = self.active.as_mut().filter(|a| a.identity == *identity) else {
                    continue;
                };
                let slot = settlement.admission.metadata.request().slot;
                if *handed_off && active.ring.pending_release(slot.slot) != Some(slot) {
                    continue;
                }
                let files = transport
                    .files_mut()
                    .ok_or(ShellTransportError::NotConnected)?;
                let before = files.export().owned_publication_sequence();
                let publication = files.export_mut().owned_release(settlement);
                if before != files.export().owned_publication_sequence() {
                    files.image_publication_pending();
                }
                match publication {
                    Ok(()) => {
                        if *handed_off {
                            assert!(
                                active.ring.confirm_release(slot),
                                "exact finished source read"
                            );
                        }
                        *entry = None;
                    }
                    // Terminal ResourceStatus must be journaled before slot release.
                    Err(sophia_9p::Errno::EAGAIN) => {}
                    Err(error) => return Err(export_error("slot release", error)),
                }
            }
            Ok(())
        })
    }
}

fn export_error(operation: &str, error: sophia_9p::Errno) -> ShellTransportError {
    ShellTransportError::Io(format!("owned ring {operation}: {error:?}"))
}
