//! The combined frame job's exporter hop: the facade is private here, so the
//! native scanout reaches it only through these forwarders.
use super::super::worker::{
    CombinedRenderPlan, CombinedRenderResult, CombinedSettleRefused, settle_without_renderer,
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
    ) -> io::Result<CombinedInbox<CombinedRenderPlan>> {
        self.worker
            .as_mut()
            .ok_or(io::ErrorKind::Unsupported)?
            .reserve_combined_frame()
            .ok_or_else(|| io::ErrorKind::WouldBlock.into())
    }

    pub(crate) fn cancel_combined_frame(&mut self) {
        if let Some(worker) = self.worker.as_mut() {
            worker.cancel_combined_frame();
        }
    }

    /// `NotConnected` once the worker's combined route has closed.
    pub(crate) fn take_combined_frame_result(&self) -> io::Result<Option<CombinedRenderResult>> {
        match self
            .worker
            .as_ref()
            .ok_or(io::ErrorKind::Unsupported)?
            .take_combined_result()
        {
            Ok(result) => Ok(Some(result)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(io::ErrorKind::NotConnected.into()),
        }
    }

    pub(crate) fn settle_combined_frame(
        &mut self,
        validator: &mut IsbValidationWorker<CombinedRenderPlan>,
        reservation: CombinedReservation,
        result: CombinedRenderResult,
    ) -> Result<ValidatedIsbPixels, CombinedSettleRefused> {
        match self.worker.as_mut() {
            Some(worker) => worker.settle_combined_frame(validator, reservation, result),
            None => settle_without_renderer(validator, reservation, result),
        }
    }
}
