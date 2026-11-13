//! G3-5d d-1: the combined fixture on a running worker. The core's own
//! thread runs the real loop with no render device; its bound combined frames
//! render through the deterministic seam, which records what it read and
//! answers a descriptor the worker leases in its own table. Attempt, bind,
//! the slot pool, the repaint table and damage settlement, the lease ids, the
//! Release checks and the result route are the worker's. The seam replaces
//! only the GPU draw: it constructs no capture, and no ordinary frame is
//! rendered (ordinary work is still only queued on this fixture).
//!
//! Each output's native service is the generic runtime tick on its exporter
//! with the counting device, then the owner's renderer-content advance and,
//! at the submission boundary, the owner's shared head operations
//! (singleton_tick.rs): `accepted_native_frame` checks the captured identity
//! against this owner, head and target, and `take_accepted_content` promotes
//! the rendering frame to submitted and hands over its content, which the
//! head keeps as `submitted_content`. A failed submission takes
//! `take_failed_content`. It is not the owner's tick wrapper: no pacing,
//! cursor, preview-failure or trace bookkeeping, no deferred-content
//! settlement and no flip completion.
use super::super::accept_device::{CountingDevice, Refuse};
use super::*;
use sophia_renderer_live::{
    LiveCpuPixelStorage, LiveOwnedMixedCompositionLayer, LiveRendererImportHealth,
    LiveRendererImportPathStatus, LiveRendererRuntimeObservation,
    LiveRendererScanoutBufferDescriptor, LiveRendererScanoutBufferExportDetail,
    LiveRendererSelectionObservation, LiveSharedCpuBufferSource,
};
use std::sync::{Arc, Mutex};

/// One validated occurrence the running seam read, in final layer order:
/// its layer index, image handle, a checksum of its bytes, and the pixels'
/// strong count when read.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CombinedSeen {
    pub index: usize,
    pub handle: u64,
    pub checksum: u64,
    pub strong: usize,
}

/// One output's native service on the running fixture.
pub(crate) struct NativeService {
    output: OutputId,
    runtime: crate::LiveBackendRuntimeAssembly,
    device: CountingDevice,
}

/// The seam's checksum: the same fold as the G3-2 bind controls.
fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(17, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(u64::from(*byte))
    })
}

/// The deterministic render: reads every validated occurrence of a Mixed
/// frame, then answers an XRGB8888 descriptor of the target's size.
fn render(
    seen: &Mutex<Vec<CombinedSeen>>,
    frame: &crate::PendingRenderedFrame,
    target: crate::api::LiveGbmEglFrameTargetRecord,
    _repaint: Option<&sophia_renderer_live::NativeCompositionRepaintTable>,
) -> Result<LiveRendererScanoutBufferDescriptor, LiveRendererScanoutBufferExportDetail> {
    let crate::PendingRenderedFrame::Mixed(mixed, _) = frame else {
        return Err(LiveRendererScanoutBufferExportDetail::ComposeRefused);
    };
    let mut seen = seen
        .lock()
        .map_err(|_| LiveRendererScanoutBufferExportDetail::ComposeRefused)?;
    for (index, layer) in mixed.layers.iter().enumerate() {
        if let LiveOwnedMixedCompositionLayer::Cpu {
            buffer:
                LiveSharedCpuBufferSource {
                    handle,
                    bytes: bytes @ LiveCpuPixelStorage::Validated(pixels),
                    ..
                },
            ..
        } = layer
        {
            seen.push(CombinedSeen {
                index,
                handle: *handle,
                checksum: checksum(bytes),
                strong: Arc::strong_count(pixels),
            });
        }
    }
    let pitch = u32::try_from(target.size.width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or(LiveRendererScanoutBufferExportDetail::InvalidTarget)?;
    Ok(LiveRendererScanoutBufferDescriptor::new(
        target.size,
        pitch,
        sophia_renderer_live::LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        1,
    ))
}

impl CombinedNativeFixture {
    /// One enabled head per output, each with an exporter on one running
    /// worker core whose combined renders go through the deterministic seam,
    /// and each output with its own native service.
    pub fn running(outputs: &[sophia_engine::HeadlessOutput]) -> Self {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&seen);
        let core = crate::running_seam_core(
            9_500,
            Box::new(
                move |frame: &crate::PendingRenderedFrame,
                      target: crate::api::LiveGbmEglFrameTargetRecord,
                      repaint: Option<&sophia_renderer_live::NativeCompositionRepaintTable>| {
                    render(&record, frame, target, repaint)
                },
            ),
        );
        let mut fixture = Self::on_core(outputs, &core, None);
        fixture.seen = seen;
        fixture.native = outputs
            .iter()
            .map(|output| NativeService {
                output: output.id,
                runtime: crate::LiveBackendRuntimeAssembly::from_ready_headless_scanout(
                    sophia_engine::HeadlessCompositorBackendAssembly::new(*output),
                    *output,
                    LiveRendererRuntimeObservation {
                        health: LiveRendererImportHealth::CpuFallback,
                        xpixmap: LiveRendererImportPathStatus::Disabled,
                        dmabuf: LiveRendererImportPathStatus::Disabled,
                        selection: LiveRendererSelectionObservation::CpuFallback,
                    },
                ),
                device: CountingDevice::new(Refuse::Nothing),
            })
            .collect();
        fixture
    }

    /// `output`'s native service once: the generic runtime tick on its
    /// exporter with the counting device, the owner's renderer-content
    /// advance, then the owner's head operations for the submission status
    /// (module doc). Answers the tracked submission's status, if the tick
    /// reached one. A submission whose captured identity is not this head's
    /// is an error and leaves the head as it was; an export still pending
    /// after the worker finished is an error, because the owner's deferred
    /// settlement is not modelled here.
    pub fn service_native(
        &mut self,
        output: OutputId,
    ) -> Result<
        Option<crate::LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus>,
        Box<dyn std::error::Error>,
    > {
        let index = self
            .heads
            .iter()
            .position(|head| head.output.id == output)
            .ok_or("no fixture head serves the output")?;
        let service = self
            .native
            .iter_mut()
            .find(|service| service.output == output)
            .ok_or("a running fixture serves the output natively")?;
        let exporter = &mut self.exporters[index];
        let worker_was_in_flight = exporter.worker_in_flight();
        let report = service
            .runtime
            .run_tick_with_native_gbm_rendered_primary_plane_scanout_exporter_with(
                sophia_engine::CompositorBackendTickInput::default(),
                &service.device,
                exporter,
            )
            .map_err(|error| format!("native tick: {error:?}"))?;
        let worker_is_in_flight = exporter.worker_in_flight();
        let nonzero_rgb_pixels = exporter.composition_nonzero_rgb_pixels();
        let head = &mut self.heads[index];
        let renderer_started = advance_live_production_renderer_content(
            worker_was_in_flight,
            worker_is_in_flight,
            &mut head.pending_content,
            &mut head.rendering_content,
        )?;
        if renderer_started && head.output_frames.pending().is_some() {
            head.output_frames
                .mark_rendering()
                .map_err(|error| format!("render transition: {error}"))?;
        }
        let status = report
            .rendered_primary_plane_scanout_submit
            .map(|submit| submit.status);
        use crate::LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus as Status;
        match status {
            Some(Status::SubmittedWaitingForPageFlip) => {
                let captured = service
                    .runtime
                    .submitted_rendered_frame_correlation(output)
                    .and_then(|correlation| correlation.native);
                let native_frame =
                    head.accepted_native_frame(captured, self.native_owner, output)?;
                head.submitted_content =
                    head.take_accepted_content(native_frame, nonzero_rgb_pixels)?;
            }
            Some(Status::ScanoutExportPending) if worker_was_in_flight && !worker_is_in_flight => {
                return Err("deferred renderer content settlement is not modelled".into());
            }
            Some(
                Status::ScanoutExportPending | Status::AlreadyInFlight | Status::CleanupPending,
            )
            | None => {}
            Some(_) => {
                let _ = head.take_failed_content(worker_was_in_flight);
            }
        }
        Ok(status)
    }

    /// `output`'s device counts: framebuffers, commits, test commits.
    pub fn device_counts(&self, output: OutputId) -> Option<(usize, usize, usize)> {
        self.native
            .iter()
            .find(|service| service.output == output)
            .map(|service| service.device.counts())
    }

    /// The occurrences the running seam has read so far, in order.
    pub fn seen(&self) -> Vec<CombinedSeen> {
        self.seen
            .lock()
            .map(|seen| seen.clone())
            .unwrap_or_default()
    }

    /// `output`'s worker slot metrics: acquisitions, deferrals, stale
    /// releases and leased slots, as the worker publishes them.
    pub fn slot_metrics(&self, output: OutputId) -> Option<crate::LiveRendererFrameSlotMetrics> {
        let index = self
            .heads
            .iter()
            .position(|head| head.output.id == output)?;
        self.exporters[index]
            .worker_metrics()
            .map(|metrics| metrics.frame_slots)
    }

    /// Rings `notifier` on every render result, through each exporter.
    pub fn attach_owner_wake(&self, notifier: &sophia_wake::Notifier) {
        for exporter in &self.exporters {
            exporter.attach_owner_wake(notifier);
        }
    }
}

#[cfg(test)]
#[path = "combined_native_running_tests.rs"]
mod tests;
