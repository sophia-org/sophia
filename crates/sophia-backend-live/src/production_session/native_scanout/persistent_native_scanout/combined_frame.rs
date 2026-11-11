//! Combined frame-job entry points for the Session owner (BRIDGE.md 4c-3).
//! One output, one head, one facade: a mirrored output is refused rather
//! than split across facades that each hold their own Release cell.
use super::*;
use crate::{CombinedRenderPlan, CombinedRenderResult, CombinedSettleRefused};
use sophia_runtime::{CombinedInbox, CombinedReservation, IsbValidationWorker, ValidatedIsbPixels};
use std::io;

impl LiveProductionNativeScanout {
    fn combined_head(&self, output: OutputId) -> io::Result<usize> {
        let mut heads = self
            .heads
            .iter()
            .enumerate()
            .filter(|(_, head)| head.enabled && head.output.id == output);
        let (index, _) = heads.next().ok_or(io::ErrorKind::NotFound)?;
        if heads.next().is_some() {
            return Err(io::ErrorKind::Unsupported.into());
        }
        Ok(index)
    }

    fn combined_exporter(
        &mut self,
        output: OutputId,
    ) -> io::Result<
        &mut NativeGbmRenderedScanoutBufferDiscoveryExporter<
            RealAtomicScanoutRenderDeviceDiscovery,
        >,
    > {
        let index = self.combined_head(output)?;
        self.exporters
            .get_mut(index)
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    /// The renderer half of one combined admission: the output's Release
    /// cell and an inbox for the validator to forward into.
    pub fn reserve_combined_frame(
        &mut self,
        output: OutputId,
    ) -> io::Result<CombinedInbox<CombinedRenderPlan>> {
        self.combined_exporter(output)?.reserve_combined_frame()
    }

    /// The validator refused the submission after the reservation.
    pub fn cancel_combined_frame(&mut self, output: OutputId) {
        if let Ok(exporter) = self.combined_exporter(output) {
            exporter.cancel_combined_frame();
        }
    }

    pub fn take_combined_frame_result(
        &mut self,
        output: OutputId,
    ) -> io::Result<Option<CombinedRenderResult>> {
        self.combined_exporter(output)?.take_combined_frame_result()
    }

    /// Retains the rendered lease before the validation credit clears. An
    /// output no longer served has no renderer to retain for; the credit still
    /// clears and the pixels come back.
    // shortcut: an output that became mirrored after its reservation also
    // settles without a renderer, so an exported lease stays with its worker
    // until deregistration; step 3's pending row must carry the head identity.
    pub fn settle_combined_frame(
        &mut self,
        output: OutputId,
        validator: &mut IsbValidationWorker<CombinedRenderPlan>,
        reservation: CombinedReservation,
        result: CombinedRenderResult,
    ) -> Result<ValidatedIsbPixels, CombinedSettleRefused> {
        match self.combined_exporter(output) {
            Ok(exporter) => exporter.settle_combined_frame(validator, reservation, result),
            Err(_) => crate::settle_without_renderer(validator, reservation, result),
        }
    }
}
