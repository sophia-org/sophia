//! The sealed validation worker beside the lock transport. It outlives each
//! epoch's custody: custody decides and journals, this owner reserves
//! worker credit for admitted offers, hands matched Sources to the worker
//! and reports completions back. Each turn drains delivered completions
//! before custody expires anything. Jobs of an ended epoch are released if
//! unsent or cancelled if submitted; their completions are taken and
//! dropped, so no slot is lost and nothing journals after the epoch.
//!
//! If the worker itself ends, its owed completions can never arrive. The
//! live epoch fails closed (custody is revoked, so the connection ends) and
//! every later offer is refused with reason 9 instead of being admitted.
use std::os::fd::BorrowedFd;
use std::time::Instant;

use sophia_9p::Errno;
use sophia_protocol::lock_files::LockImagePacket;

use sophia_protocol::lock_files::LOCK_RESOURCE_REASON_IMAGE_CHANNEL_UNAVAILABLE;

use super::{LockFileCustody, LockImageSource, LockImageWorkerAction, reason};
use crate::image_channel::ImageDatagram;
use crate::{IsbSubmitFailure, IsbValidationFailure, IsbValidationWorker, SharedPixels};

/// A worker job and whether its epoch has ended.
struct Job {
    id: u64,
    ended: bool,
}

pub(super) struct ImageWorker {
    worker: IsbValidationWorker,
    jobs: Vec<Job>,
    /// A completion custody could not journal yet; retried before others.
    parked: Option<(u64, Result<SharedPixels, IsbValidationFailure>)>,
    /// The worker ended; its jobs will never complete.
    dead: bool,
}

impl ImageWorker {
    pub(super) fn new(
        max_jobs: usize,
        max_bytes: usize,
        validation_bytes: usize,
    ) -> std::io::Result<Self> {
        Ok(Self {
            worker: IsbValidationWorker::with_validation_budget(
                max_jobs,
                max_bytes,
                validation_bytes,
            )?,
            jobs: Vec::new(),
            parked: None,
            dead: false,
        })
    }

    /// The worker's completion readiness while it can still complete
    /// anything; a dead worker is never polled.
    pub(super) fn readiness(&self) -> Option<BorrowedFd<'_>> {
        (!self.dead && self.pending() != 0).then(|| self.worker.readiness())
    }

    /// The worker ended. The live epoch, if any, fails closed: its owed
    /// completions are lost, so custody is revoked and the connection ends.
    pub(super) fn fail(&mut self, custody: Option<&mut LockFileCustody>) {
        self.dead = true;
        if let Some(custody) = custody {
            custody.revoke();
        }
    }

    /// Jobs this owner tracks: reservations, jobs whose completion is
    /// owed, and a parked completion. Each holds a worker credit except the
    /// parked one, whose credit came back when it was taken.
    pub(super) fn pending(&self) -> usize {
        debug_assert_eq!(
            self.jobs.len(),
            self.worker.pending() + usize::from(self.parked.is_some())
        );
        self.jobs.len()
    }

    fn forget(&mut self, id: u64) -> Option<Job> {
        let index = self.jobs.iter().position(|job| job.id == id)?;
        Some(self.jobs.remove(index))
    }

    /// Reports one completion to custody. `false` parks it for room.
    ///
    /// The offer's two held credits leave room for its settlement, so
    /// `EAGAIN` is not expected; a parked completion still keeps its job.
    /// A revoked custody (`ESTALE` from its liveness check) ends the epoch,
    /// and its result is dropped. Any other refusal means custody no
    /// longer owes this live job a settlement it can journal: custody is
    /// revoked, failing closed, so the connection ends and no offer waits
    /// on a completion that was lost.
    fn complete(
        &mut self,
        custody: &mut LockFileCustody,
        id: u64,
        result: Result<SharedPixels, IsbValidationFailure>,
    ) -> bool {
        match custody.image_completed(id, result.clone()) {
            Ok(_) => {}
            Err(Errno::EAGAIN) => {
                self.parked = Some((id, result));
                return false;
            }
            Err(_) if custody.is_revoked() => {}
            Err(_) => custody.revoke(),
        }
        self.forget(id);
        true
    }

    /// One owner turn against the live epoch: completions, worker actions,
    /// expiry, then admissions. Returns whether anything may have been
    /// journalled.
    pub(super) fn serve(&mut self, custody: &mut LockFileCustody, now: Instant) -> bool {
        let before = custody.position().next_sequence;
        if let Some((id, result)) = self.parked.take()
            && !self.complete(custody, id, result)
        {
            return false;
        }
        if self.dead {
            self.actions(custody);
            let _ = custody.expire_images(now);
            self.actions(custody);
            self.refuse_all(custody, now);
            return custody.position().next_sequence != before;
        }
        loop {
            let completion = match self.worker.take_completed() {
                Ok(Some(completion)) => completion,
                Ok(None) => break,
                Err(_) => {
                    self.fail(Some(custody));
                    return custody.position().next_sequence != before;
                }
            };
            let id = completion.job;
            let ended = self
                .jobs
                .iter()
                .find(|job| job.id == id)
                .is_none_or(|job| job.ended);
            if ended {
                self.forget(id);
                continue;
            }
            let result = completion.result.map(|pixels| pixels.into_shared());
            if !self.complete(custody, id, result) {
                break;
            }
        }
        self.actions(custody);
        let _ = custody.expire_images(now);
        self.actions(custody);
        self.admit(custody, now);
        custody.position().next_sequence != before
    }

    fn actions(&mut self, custody: &mut LockFileCustody) {
        while let Some(action) = custody.take_image_worker_action() {
            match action {
                LockImageWorkerAction::Release(id) => {
                    self.worker.release_reserved(id);
                    self.forget(id);
                }
                LockImageWorkerAction::Cancel(id) => {
                    self.worker.cancel(id);
                }
            }
        }
    }

    /// With no worker, every offer custody would admit is refused instead.
    fn refuse_all(&mut self, custody: &mut LockFileCustody, now: Instant) {
        while let Ok(Some(admission)) = custody.image_admission(now) {
            if custody
                .refuse_image(
                    admission.offer_id,
                    LOCK_RESOURCE_REASON_IMAGE_CHANNEL_UNAVAILABLE,
                )
                .is_err()
            {
                return;
            }
        }
    }

    /// Reserves worker credit for each pending offer custody passes, oldest
    /// first. Without credit the offer waits within its deadline; a size
    /// the worker cannot take is refused as budget.
    fn admit(&mut self, custody: &mut LockFileCustody, now: Instant) {
        while let Ok(Some(admission)) = custody.image_admission(now) {
            match self.worker.reserve(admission.bytes, admission.deadline) {
                Ok(id) => match custody.admit_image(admission.offer_id, id, admission.deadline) {
                    Ok(_) => self.jobs.push(Job { id, ended: false }),
                    Err(_) => {
                        self.worker.release_reserved(id);
                        return;
                    }
                },
                Err(IsbSubmitFailure::Capacity) => return,
                Err(IsbSubmitFailure::Invalid | IsbSubmitFailure::Closed) => {
                    if custody
                        .refuse_image(admission.offer_id, reason::BUDGET)
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }
    }

    /// One received datagram on a ready channel. `Err` faults the channel:
    /// a malformed packet or a Source custody does not owe. A Source the
    /// worker refuses stays admitted with its reservation and settles at
    /// its deadline.
    pub(super) fn source(
        &mut self,
        custody: &mut LockFileCustody,
        mut datagram: ImageDatagram,
    ) -> Result<(), Errno> {
        let packet = LockImagePacket::decode(datagram.bytes()).map_err(|_| Errno::EPROTO)?;
        match custody.image_source(&packet, datagram.descriptor_count())? {
            LockImageSource::Stale => Ok(()),
            LockImageSource::Submit { job } => {
                let source = datagram.take_descriptor(0).ok_or(Errno::EPROTO)?;
                match self.worker.submit_reserved(job, source) {
                    Ok(()) => custody.image_source_submitted(job)?,
                    Err((IsbSubmitFailure::Closed, _)) => self.fail(Some(custody)),
                    // Kept admitted with its reservation until its deadline.
                    Err(_) => {}
                }
                Ok(())
            }
        }
    }

    /// The epoch ended: unsent reservations go back now, submitted jobs are
    /// cancelled and their completions dropped when they arrive.
    pub(super) fn end_epoch(&mut self) {
        // A parked completion's credit is already back; only its job and
        // result remain.
        if let Some((id, _)) = self.parked.take() {
            self.forget(id);
        }
        let worker = &mut self.worker;
        self.jobs.retain_mut(|job| {
            if worker.release_reserved(job.id) {
                return false;
            }
            worker.cancel(job.id);
            job.ended = true;
            true
        });
    }

    /// A turn with no live epoch: take and drop ended jobs' completions.
    pub(super) fn drain_ended(&mut self) {
        while !self.dead {
            match self.worker.take_completed() {
                Ok(Some(completion)) => {
                    self.forget(completion.job);
                }
                Ok(None) => break,
                Err(_) => self.fail(None),
            }
        }
    }
}

#[path = "../../tests/support/lock_image_worker.rs"]
mod tests;
