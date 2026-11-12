//! Combined frame-job entry points for the Session owner (BRIDGE.md 4c-3,
//! AUTHORITY-STEP3.md 4c-4). Admission picks the one facade serving an
//! output; every later call reaches that issuing facade through its
//! `CombinedHead`, whatever the output became since.
use super::*;
use crate::{
    CombinedFacade, CombinedRelease, CombinedRenderPlan, CombinedRenderResult,
    CombinedSettleRefused,
};
use sophia_runtime::{CombinedInbox, CombinedReservation, IsbValidationWorker, ValidatedIsbPixels};
use std::io;

/// The facade that issued one combined admission. Opaque to the session; it
/// locates the same renderer state after disable, mirror or remap, and never
/// a replacement facade attached under the same output key.
pub struct CombinedHead {
    native_owner: crate::NativeFrameOwner,
    head: sophia_engine::RenderHeadId,
    target_generation: u64,
    facade: CombinedFacade,
}

impl CombinedHead {
    /// The head's target generation at admission; acceptance compares it
    /// with the current one, cleanup never does.
    pub fn target_generation(&self) -> u64 {
        self.target_generation
    }
}

impl LiveProductionNativeScanout {
    /// One output, one head, one facade: a mirrored output is refused rather
    /// than split across facades that each hold their own Release cell.
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

    /// The exporter whose facade issued `head`, or none. Heads and exporters
    /// keep their construction order, so the head identity is a check here,
    /// and the claim pointer is the exact key.
    fn issuing_exporter(
        &mut self,
        head: &CombinedHead,
    ) -> Option<
        &mut NativeGbmRenderedScanoutBufferDiscoveryExporter<
            RealAtomicScanoutRenderDeviceDiscovery,
        >,
    > {
        if head.native_owner != self.native_frame_owner {
            return None;
        }
        self.exporters
            .iter_mut()
            .zip(&self.heads)
            .find(|(exporter, current)| {
                current.head == head.head && exporter.issued_combined(&head.facade)
            })
            .map(|(exporter, _)| exporter)
    }

    /// The renderer half of one combined admission: the output's Release
    /// cell, the handle that reaches it again, and an inbox for the validator.
    pub fn reserve_combined_frame(
        &mut self,
        output: OutputId,
    ) -> io::Result<(CombinedHead, CombinedInbox<CombinedRenderPlan>)> {
        let index = self.combined_head(output)?;
        let native_owner = self.native_frame_owner;
        let (head, target_generation) = {
            let head = &self.heads[index];
            (head.head, head.target_generation)
        };
        let exporter = self
            .exporters
            .get_mut(index)
            .ok_or(io::ErrorKind::NotFound)?;
        let (facade, inbox) = exporter.reserve_combined_frame()?;
        Ok((
            CombinedHead {
                native_owner,
                head,
                target_generation,
                facade,
            },
            inbox,
        ))
    }

    /// The validator refused the submission after this admission's
    /// reservation. `NotFound` when the issuing facade is gone; `false` when
    /// the reservation was already spent.
    pub fn cancel_combined_frame(&mut self, head: &CombinedHead) -> io::Result<bool> {
        self.issuing_exporter(head)
            .and_then(|exporter| exporter.cancel_combined_frame(&head.facade))
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    pub fn take_combined_frame_result(
        &mut self,
        head: &CombinedHead,
    ) -> io::Result<Option<CombinedRenderResult>> {
        self.issuing_exporter(head)
            .and_then(|exporter| exporter.take_combined_frame_result(&head.facade))
            .unwrap_or_else(|| Err(io::ErrorKind::NotFound.into()))
    }

    /// Holds an exported lease in the issuing facade before the validation
    /// credit clears. A missing facade is `RendererNotFound` with everything
    /// returned, never a settlement.
    pub fn settle_combined_frame(
        &mut self,
        head: &CombinedHead,
        validator: &mut IsbValidationWorker<CombinedRenderPlan>,
        reservation: CombinedReservation,
        result: CombinedRenderResult,
    ) -> Result<ValidatedIsbPixels, CombinedSettleRefused> {
        match self.issuing_exporter(head) {
            Some(exporter) => {
                exporter.settle_combined_frame(&head.facade, validator, reservation, result)
            }
            None => Err(CombinedSettleRefused::RendererNotFound(
                reservation,
                Box::new(result),
            )),
        }
    }

    /// The owner declined `job`'s held result: retain its lease for Release
    /// and try the flush once.
    pub fn release_combined_held(&mut self, head: &CombinedHead, job: u64) -> CombinedRelease {
        self.issuing_exporter(head)
            .map_or(CombinedRelease::NotFound, |exporter| {
                exporter.release_combined_held(&head.facade, job)
            })
    }

    /// The owner's per-turn maintenance for `job`'s Release debt, on idle
    /// and disabled heads alike; it never polls ordinary work.
    pub fn flush_combined_release(&mut self, head: &CombinedHead, job: u64) -> CombinedRelease {
        self.issuing_exporter(head)
            .map_or(CombinedRelease::NotFound, |exporter| {
                exporter.flush_combined_release(&head.facade, job)
            })
    }
}
