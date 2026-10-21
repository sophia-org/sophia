//! Per-component ISB (Immutable Shared Buffers) work survives channel closure
//! and connection replacement.
use super::{ShellComponentTransport, ShellTransportError};
use crate::{
    ContentEpochRegistry, IsbImageJobs, IsbImageSettlement, IsbImageWithdrawal, IsbValidationWorker,
};
use sophia_protocol::{ContentGrant, ContentReason};
use std::os::fd::BorrowedFd;
use std::time::Instant;

/// Own this beside the component's registry/transport, never inside an export.
/// Call service on every owner turn, even after disconnect, with the same
/// monotonic millisecond clock used by that component's resource store.
/// Shutdown must drain pending work before dropping this owner or the registry.
pub struct ShellIsbImageOwner {
    jobs: IsbImageJobs,
    #[cfg(feature = "experimental-draw")]
    draw: super::draw::DrawJobs,
    owned: super::owned_copy::OwnedCopyOwner,
    worker: IsbValidationWorker,
    releases: [Option<IsbImageSettlement>; 4],
    waiting: Option<(ContentGrant, u64, Instant, Option<u16>)>,
}

impl ShellIsbImageOwner {
    pub fn new(max_resource_bytes: usize, validation_bytes: usize) -> std::io::Result<Self> {
        Ok(Self {
            jobs: IsbImageJobs::default(),
            #[cfg(feature = "experimental-draw")]
            draw: super::draw::DrawJobs::default(),
            owned: super::owned_copy::OwnedCopyOwner::default(),
            worker: IsbValidationWorker::with_validation_budget(
                4,
                max_resource_bytes,
                validation_bytes,
            )?,
            releases: std::array::from_fn(|_| None),
            waiting: None,
        })
    }

    pub fn readiness(&self) -> BorrowedFd<'_> {
        self.worker.readiness()
    }

    pub fn deadline(&self) -> Option<Instant> {
        let deadlines = self.jobs.deadline().into_iter();
        #[cfg(feature = "experimental-draw")]
        let deadlines = deadlines.chain(self.draw.deadline());
        let deadlines = deadlines.chain(self.owned.deadline());
        deadlines
            .chain(
                self.waiting
                    .filter(|(_, _, _, refusal)| refusal.is_none())
                    .map(|(_, _, at, _)| at),
            )
            .min()
    }

    pub fn pending(&self) -> usize {
        let image = self.jobs.pending() + self.releases.iter().flatten().count();
        #[cfg(feature = "experimental-draw")]
        let image = image + self.draw.pending();
        image + self.owned.pending()
    }

    #[cfg(feature = "experimental-draw")]
    fn service_draw(
        &mut self,
        transport: &mut ShellComponentTransport,
        epochs: &mut ContentEpochRegistry,
        now: u64,
        instant: Instant,
    ) -> Result<(), ShellTransportError> {
        let other_pending = self.pending() - self.draw.pending();
        self.draw.service(
            transport,
            epochs,
            &mut self.worker,
            now,
            instant,
            other_pending,
        )
    }
    fn service_owned(
        &mut self,
        transport: &mut ShellComponentTransport,
        epochs: &mut ContentEpochRegistry,
        now: u64,
        instant: Instant,
    ) -> Result<(), ShellTransportError> {
        let other_pending = self.pending() - self.owned.pending();
        self.owned.service(
            transport,
            epochs,
            &mut self.worker,
            now,
            instant,
            other_pending,
        )
    }

    fn reconcile(&mut self, epochs: &mut ContentEpochRegistry, now: u64) {
        // Each admission occupies a slot through release publication, even
        // after the worker and resource store have completed their handoffs.
        for slot in &mut self.releases {
            if slot.is_none() {
                *slot = self.jobs.reconcile(epochs, now);
            }
        }
    }

    pub fn service(
        &mut self,
        transport: &mut ShellComponentTransport,
        epochs: &mut ContentEpochRegistry,
        now: u64,
        instant: Instant,
    ) -> Result<(), ShellTransportError> {
        #[cfg(feature = "experimental-draw")]
        self.service_draw(transport, epochs, now, instant)?;
        self.service_owned(transport, epochs, now, instant)?;
        let grant = transport.content_grant();
        let ready = transport.isb_image_channel_ready();
        let mut lost = [None; 4];
        for (slot, admission) in lost.iter_mut().zip(self.jobs.admissions()) {
            if !ready || grant != Some(admission.grant) {
                *slot = Some(admission.grant);
            }
        }
        for grant in lost.into_iter().flatten() {
            self.jobs
                .withdraw_epoch(epochs, &mut self.worker, grant, now)?;
        }
        self.jobs
            .observe_store_stops(epochs, &mut self.worker, now)?;
        while let Some(completion) = self
            .worker
            .take_completed()
            .map_err(|e| ShellTransportError::Io(e.to_string()))?
        {
            #[cfg(feature = "experimental-draw")]
            let completion = match self.draw.receive(completion) {
                Ok(()) => continue,
                Err(completion) => completion,
            };
            let completion = match self.owned.receive(completion) {
                Ok(()) => continue,
                Err(completion) => completion,
            };
            // This owner exclusively reserves, submits and receives this worker.
            assert!(
                self.jobs.receive(completion).is_ok(),
                "paired image worker completion"
            );
        }
        #[cfg(feature = "experimental-draw")]
        self.service_draw(transport, epochs, now, instant)?;
        self.service_owned(transport, epochs, now, instant)?;
        self.reconcile(epochs, now);
        self.jobs.expire(epochs, &mut self.worker, instant, now)?;
        self.reconcile(epochs, now);
        for slot in &mut self.releases {
            let Some(settlement) = slot.as_ref() else {
                continue;
            };
            if grant != Some(settlement.admission.grant) {
                // Its journal died; resource reconciliation already retained
                // or released backing under the exact retired epoch.
                *slot = None;
                continue;
            }
            match transport.release_isb_image_source(settlement) {
                Ok(_) => *slot = None,
                // The resource drain must journal the terminal status first.
                // Keep the completed settlement for the next owner turn.
                Err(ShellTransportError::ContentQueueSaturated) => {}
                Err(error) => return Err(error),
            }
        }
        if ready {
            transport.enable_isb_image_admission()?;
            while let Some((job, source)) = transport.take_isb_image_source() {
                if let Err((_, source)) = self.jobs.submit(&mut self.worker, job, source) {
                    drop(source);
                    self.jobs.withdraw(
                        epochs,
                        &mut self.worker,
                        job,
                        IsbImageWithdrawal::Cancelled,
                        now,
                    )?;
                }
            }
        }
        let Some(metadata) = transport.pending_isb_image_offer() else {
            self.waiting = None;
            return Ok(());
        };
        let grant = grant.ok_or(ShellTransportError::NotConnected)?;
        if self
            .waiting
            .is_none_or(|(old, id, _, _)| old != grant || id != metadata.offer_id)
        {
            let timeout = transport
                .wire
                .as_ref()
                .and_then(|wire| wire.export().image_validation_timeout())
                .ok_or(ShellTransportError::MissingCapability)?;
            self.waiting = Some((
                grant,
                metadata.offer_id,
                instant
                    .checked_add(timeout)
                    .ok_or(ShellTransportError::WrongContentGrant)?,
                None,
            ));
        }
        let reason = if let Some(reason) = self.waiting.and_then(|(_, _, _, reason)| reason) {
            Some(reason)
        } else if !ready {
            Some(13)
        } else if instant >= self.waiting.expect("pending offer deadline").2 {
            Some(ContentReason::Budget as u16)
        } else if self.pending() >= 4 {
            None
        } else {
            match transport.admit_pending_isb_image(
                epochs,
                &mut self.jobs,
                &mut self.worker,
                now,
                instant,
            ) {
                Ok(_) => {
                    self.waiting = None;
                    return Ok(());
                }
                Err(ShellTransportError::ContentQueueSaturated) => None,
                Err(ShellTransportError::WrongContentGrant) => Some(ContentReason::Stale as u16),
                Err(ShellTransportError::WrongContentRecord) => {
                    Some(ContentReason::Malformed as u16)
                }
                Err(error) => return Err(error),
            }
        };
        if let Some(reason) = reason {
            let files = transport
                .files_mut()
                .ok_or(ShellTransportError::NotConnected)?;
            match files.export_mut().refuse_image_offer_reason(reason) {
                Ok(()) => {
                    self.waiting = None;
                    files.image_publication_pending();
                }
                Err(sophia_9p::Errno::EAGAIN) => {
                    // The deadline has already selected refusal. Only peer
                    // ACK progress can free journal room; do not busy-wake on
                    // the same expired deadline while that refusal is held.
                    self.waiting.as_mut().expect("pending refusal").3 = Some(reason);
                }
                Err(error) => {
                    return Err(ShellTransportError::Io(format!("image refusal: {error:?}")));
                }
            }
        }
        Ok(())
    }
}
