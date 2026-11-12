//! The combined frame job's exporter hop: the facade is private here, so the
//! native scanout reaches it only through these forwarders.
use super::super::worker::{
    CombinedFacade, CombinedRelease, CombinedRenderPlan, CombinedRenderResult,
    CombinedSettleRefused,
};
use super::*;
use sophia_runtime::{CombinedInbox, CombinedReservation, IsbValidationWorker, ValidatedIsbPixels};
use std::io;
use std::sync::mpsc::TryRecvError;

impl<R: RenderDeviceDiscoveryBackend> NativeGbmRenderedScanoutBufferDiscoveryExporter<R> {
    /// `Unsupported` without a worker; `WouldBlock` while the facade cannot
    /// take another combined job.
    pub(crate) fn reserve_combined_frame(
        &mut self,
    ) -> io::Result<(CombinedFacade, CombinedInbox<CombinedRenderPlan>)> {
        self.worker
            .as_mut()
            .ok_or(io::ErrorKind::Unsupported)?
            .reserve_combined_frame()
            .ok_or_else(|| io::ErrorKind::WouldBlock.into())
    }

    pub(crate) fn issued_combined(&self, facade: &CombinedFacade) -> bool {
        self.worker
            .as_ref()
            .is_some_and(|worker| worker.issued(facade))
    }

    /// This exporter's facade if it issued `facade`. Every call below is
    /// `None` on any other exporter, including a replacement facade under the
    /// same output key.
    fn issuing(
        &mut self,
        facade: &CombinedFacade,
    ) -> Option<&mut super::super::worker::NativeGbmRendererWorker> {
        self.worker.as_mut().filter(|worker| worker.issued(facade))
    }

    pub(crate) fn cancel_combined_frame(&mut self, facade: &CombinedFacade) -> Option<bool> {
        Some(self.issuing(facade)?.cancel_combined_frame(facade))
    }

    /// `Some(Err(NotConnected))` once the worker's combined route has closed.
    pub(crate) fn take_combined_frame_result(
        &mut self,
        facade: &CombinedFacade,
    ) -> Option<io::Result<Option<CombinedRenderResult>>> {
        Some(match self.issuing(facade)?.take_combined_result() {
            Ok(result) => Ok(Some(result)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(io::ErrorKind::NotConnected.into()),
        })
    }

    /// Everything comes back as `RendererNotFound` on any other exporter.
    pub(crate) fn settle_combined_frame(
        &mut self,
        facade: &CombinedFacade,
        validator: &mut IsbValidationWorker<CombinedRenderPlan>,
        reservation: CombinedReservation,
        result: CombinedRenderResult,
    ) -> Result<ValidatedIsbPixels, CombinedSettleRefused> {
        match self.issuing(facade) {
            Some(worker) => worker.settle_combined_frame(facade, validator, reservation, result),
            None => Err(CombinedSettleRefused::RendererNotFound(
                reservation,
                Box::new(result),
            )),
        }
    }

    pub(crate) fn release_combined_held(
        &mut self,
        facade: &CombinedFacade,
        job: u64,
    ) -> CombinedRelease {
        self.issuing(facade)
            .map_or(CombinedRelease::NotFound, |worker| {
                worker.release_combined_held(job)
            })
    }

    pub(crate) fn flush_combined_release(
        &mut self,
        facade: &CombinedFacade,
        job: u64,
    ) -> CombinedRelease {
        self.issuing(facade)
            .map_or(CombinedRelease::NotFound, |worker| {
                worker.flush_combined_release_for(job)
            })
    }
}
