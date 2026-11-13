//! The combined frame job's exporter hop: the facade is private here, so the
//! native scanout reaches it only through these forwarders.
use super::super::worker::{
    CombinedAcceptRefusal, CombinedFacade, CombinedRelease, CombinedRenderPlan,
    CombinedRenderResult, CombinedSettleRefused,
};
use super::*;
#[cfg(any(test, feature = "test-support"))]
use sophia_runtime::CombinedToken;
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

    /// A capture of this exporter's next frame is requested or reserved.
    pub(crate) fn combined_capture_pending(&self) -> bool {
        self.capture_requested.is_some() || self.capture_reservation.is_some()
    }

    /// The modifiers an ordinary submission on this exporter asks for.
    pub(crate) fn combined_preferred_modifiers(&self) -> Vec<u64> {
        self.preferred_modifiers.clone()
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

    /// `Some(Err(NotConnected))` once the worker's combined route has closed;
    /// `Some(Err(InvalidInput))` for an admission that is no longer pending.
    pub(crate) fn take_combined_frame_result(
        &mut self,
        facade: &CombinedFacade,
    ) -> Option<io::Result<Option<CombinedRenderResult>>> {
        Some(match self.issuing(facade)?.take_combined_frame(facade) {
            Some(Ok(result)) => Ok(Some(result)),
            Some(Err(TryRecvError::Empty)) => Ok(None),
            Some(Err(TryRecvError::Disconnected)) => Err(io::ErrorKind::NotConnected.into()),
            None => Err(io::ErrorKind::InvalidInput.into()),
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

    /// The facade's half of settlement alone, for the combined fixture's
    /// declared credit fault; everything comes back on any other exporter.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn settle_combined_cell(
        &mut self,
        facade: &CombinedFacade,
        reservation: CombinedReservation,
        result: CombinedRenderResult,
    ) -> Result<(CombinedReservation, CombinedToken, ValidatedIsbPixels), CombinedSettleRefused>
    {
        match self.issuing(facade) {
            Some(worker) => worker.settle_combined_cell(facade, reservation, result),
            None => Err(CombinedSettleRefused::RendererNotFound(
                reservation,
                Box::new(result),
            )),
        }
    }

    /// Control only: a capture of `expected` requested on this exporter, for
    /// the owner-facts decline (G3-5b). No snapshot is reserved.
    #[cfg(all(test, feature = "experimental-combined-frame-job"))]
    pub(crate) fn request_combined_capture_for_control(
        &mut self,
        expected: super::super::LiveNativeFrameIdentity,
    ) {
        self.capture_requested = Some(expected);
    }

    /// Control only: a capture snapshot reserved on this exporter with no
    /// request outstanding, for the owner-facts decline (G3-5b R4).
    #[cfg(all(test, feature = "experimental-combined-frame-job"))]
    pub(crate) fn reserve_combined_capture_for_control(&mut self) {
        let reservation = sophia_renderer_live::LiveSnapshotReservation::for_image(16, 16).unwrap();
        self.capture_reservation = Some(std::sync::Arc::new(std::sync::Mutex::new(Some(
            reservation,
        ))));
    }

    /// The exporter's part of the accept preflight: the issuing facade's
    /// checks, then no ordinary frame queued on this exporter. Reads only.
    pub(crate) fn combined_accept_refusal(
        &self,
        facade: &CombinedFacade,
        job: u64,
    ) -> Option<CombinedAcceptRefusal> {
        let Some(worker) = self.worker.as_ref().filter(|worker| worker.issued(facade)) else {
            return Some(CombinedAcceptRefusal::NotIssuing);
        };
        worker.combined_accept_refusal(facade, job).or_else(|| {
            self.pending_frame
                .is_some()
                .then_some(CombinedAcceptRefusal::OrdinaryPending)
        })
    }

    /// Accept step 4 on the issuing facade, after `combined_accept_refusal`
    /// passed in this owner turn.
    pub(crate) fn accept_combined_held(
        &mut self,
        facade: &CombinedFacade,
        job: u64,
        correlation: crate::LiveRendererFrameCorrelation,
        output_format: Option<sophia_renderer_live::LiveCompositionFormatRequest>,
    ) {
        self.issuing(facade)
            .expect("the accept preflight found the issuing facade")
            .accept_combined_held(facade, job, correlation, output_format);
    }

    pub(crate) fn release_combined_held(
        &mut self,
        facade: &CombinedFacade,
        job: u64,
    ) -> CombinedRelease {
        self.issuing(facade)
            .map_or(CombinedRelease::NotFound, |worker| {
                worker.release_combined_held(facade, job)
            })
    }

    pub(crate) fn flush_combined_release(
        &mut self,
        facade: &CombinedFacade,
        job: u64,
    ) -> CombinedRelease {
        self.issuing(facade)
            .map_or(CombinedRelease::NotFound, |worker| {
                worker.flush_combined_release_for(facade, job)
            })
    }
}
