//! Combined frame-job entry points for the Session owner (BRIDGE.md 4c-3,
//! AUTHORITY-STEP3.md 4c-4). Admission picks the one facade serving an
//! output; every later call reaches that issuing facade through its
//! `CombinedHead`, whatever the output became since.
use super::*;
use crate::{
    CombinedFacade, CombinedRelease, CombinedRenderPlan, CombinedRenderResult,
    CombinedSettleRefused, RenderDeviceDiscoveryBackend,
};
use sophia_runtime::{CombinedInbox, CombinedReservation, IsbValidationWorker, ValidatedIsbPixels};
use std::io;

#[path = "../../../../tests/support/combined_frame_routing.rs"]
mod routing_tests;
// G3-4: the two-phase accept, beside this file.
#[path = "combined_accept.rs"]
mod accept;
pub(crate) use accept::CombinedAcceptNative;
pub use accept::{CombinedAccept, CombinedAcceptPlan};
// G3-5a: one device-free native authority over the owner's parts.
#[path = "../../../../tests/support/combined_native_fixture.rs"]
mod native_fixture;
#[cfg(all(test, feature = "experimental-combined-frame-job"))]
pub(crate) use native_fixture::CombinedNativeFixture;
// The one deterministic device both device-free fixtures drive.
#[cfg(all(test, feature = "experimental-combined-frame-job"))]
#[path = "../../../../tests/support/combined_frame_accept_device.rs"]
mod accept_device;

type Exporter<R> = NativeGbmRenderedScanoutBufferDiscoveryExporter<R>;

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

/// The parts of the native scanout the combined job routes through. The
/// production entries below use this and nothing else, so device-free
/// exporters exercise the same lookup.
struct CombinedRoutes<'a, R: RenderDeviceDiscoveryBackend> {
    native_owner: crate::NativeFrameOwner,
    heads: &'a mut [LiveProductionNativeHead],
    exporters: &'a mut [Exporter<R>],
}

impl<R: RenderDeviceDiscoveryBackend> CombinedRoutes<'_, R> {
    /// One output, one head, one facade: a mirrored output is refused rather
    /// than split across facades that each hold their own Release cell.
    fn reserve(
        &mut self,
        output: OutputId,
    ) -> io::Result<(CombinedHead, CombinedInbox<CombinedRenderPlan>)> {
        let heads = &*self.heads;
        let mut serving = heads
            .iter()
            .enumerate()
            .filter(|(_, head)| head.enabled && head.output.id == output);
        let (index, head) = serving.next().ok_or(io::ErrorKind::NotFound)?;
        if serving.next().is_some() {
            return Err(io::ErrorKind::Unsupported.into());
        }
        let exporter = self
            .exporters
            .get_mut(index)
            .ok_or(io::ErrorKind::NotFound)?;
        let (facade, inbox) = exporter.reserve_combined_frame()?;
        Ok((
            CombinedHead {
                native_owner: self.native_owner,
                head: head.head,
                target_generation: head.target_generation,
                facade,
            },
            inbox,
        ))
    }

    /// The exporter whose facade issued `head`, or none. Heads and exporters
    /// keep their construction order, so the head identity is a check here,
    /// and the claim pointer is the exact key. `enabled` and the current
    /// output are never read: cleanup follows the issuer, not the topology.
    fn issuing(&mut self, head: &CombinedHead) -> Option<&mut Exporter<R>> {
        if head.native_owner != self.native_owner {
            return None;
        }
        self.exporters
            .iter_mut()
            .zip(self.heads.iter())
            .find(|(exporter, current)| {
                current.head == head.head && exporter.issued_combined(&head.facade)
            })
            .map(|(exporter, _)| exporter)
    }

    fn cancel(&mut self, head: &CombinedHead) -> io::Result<bool> {
        self.issuing(head)
            .and_then(|exporter| exporter.cancel_combined_frame(&head.facade))
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    fn take(&mut self, head: &CombinedHead) -> io::Result<Option<CombinedRenderResult>> {
        self.issuing(head)
            .and_then(|exporter| exporter.take_combined_frame_result(&head.facade))
            .unwrap_or_else(|| Err(io::ErrorKind::NotFound.into()))
    }

    fn settle(
        &mut self,
        head: &CombinedHead,
        validator: &mut IsbValidationWorker<CombinedRenderPlan>,
        reservation: CombinedReservation,
        result: CombinedRenderResult,
    ) -> Result<ValidatedIsbPixels, CombinedSettleRefused> {
        match self.issuing(head) {
            Some(exporter) => {
                exporter.settle_combined_frame(&head.facade, validator, reservation, result)
            }
            None => Err(CombinedSettleRefused::RendererNotFound(
                reservation,
                Box::new(result),
            )),
        }
    }

    fn release_held(&mut self, head: &CombinedHead, job: u64) -> CombinedRelease {
        self.issuing(head)
            .map_or(CombinedRelease::NotFound, |exporter| {
                exporter.release_combined_held(&head.facade, job)
            })
    }

    fn flush(&mut self, head: &CombinedHead, job: u64) -> CombinedRelease {
        self.issuing(head)
            .map_or(CombinedRelease::NotFound, |exporter| {
                exporter.flush_combined_release(&head.facade, job)
            })
    }
}

impl LiveProductionNativeScanout {
    fn combined_routes(&mut self) -> CombinedRoutes<'_, RealAtomicScanoutRenderDeviceDiscovery> {
        CombinedRoutes {
            native_owner: self.native_frame_owner,
            heads: &mut self.heads,
            exporters: &mut self.exporters,
        }
    }

    /// The native frame a combined admission is planned as, drawn from the
    /// counter ordinary batches use, so the two never share an identity.
    pub fn allocate_combined_frame(&mut self) -> LiveProductionNativeFrameId {
        native_facts::allocate_frame_id(&mut self.next_frame_id)
    }

    /// The renderer half of one combined admission: the output's Release
    /// cell, the handle that reaches it again, and an inbox for the validator.
    pub fn reserve_combined_frame(
        &mut self,
        output: OutputId,
    ) -> io::Result<(CombinedHead, CombinedInbox<CombinedRenderPlan>)> {
        self.combined_routes().reserve(output)
    }

    /// The validator refused the submission after this admission's
    /// reservation. `NotFound` when the issuing facade is gone; `false` when
    /// the reservation was already spent.
    pub fn cancel_combined_frame(&mut self, head: &CombinedHead) -> io::Result<bool> {
        self.combined_routes().cancel(head)
    }

    /// `NotFound` when the issuing facade is gone, `InvalidInput` when this
    /// admission is no longer the one its facade is waiting on.
    pub fn take_combined_frame_result(
        &mut self,
        head: &CombinedHead,
    ) -> io::Result<Option<CombinedRenderResult>> {
        self.combined_routes().take(head)
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
        self.combined_routes()
            .settle(head, validator, reservation, result)
    }

    /// The owner declined `job`'s held result: retain its lease for Release
    /// and try the flush once.
    pub fn release_combined_held(&mut self, head: &CombinedHead, job: u64) -> CombinedRelease {
        self.combined_routes().release_held(head, job)
    }

    /// The owner's per-turn maintenance for `job`'s Release debt, on idle
    /// and disabled heads alike; it never polls ordinary work.
    pub fn flush_combined_release(&mut self, head: &CombinedHead, job: u64) -> CombinedRelease {
        self.combined_routes().flush(head, job)
    }
}
