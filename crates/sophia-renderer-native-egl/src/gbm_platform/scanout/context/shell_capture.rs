//! Experimental owned source -> exact copy-fence -> owned snapshot adapter.
//!
//! No admission or authority is granted here. The caller must bind this work to
//! its shell job, grant and connection epoch, and recheck before publication.
//! Uncertain work is retained for process lifetime; EGL teardown is not proof.
use super::*;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

#[path = "shell_capture/escrow.rs"]
mod escrow;
use escrow::{Capacity, Escrow};

#[derive(Default)]
pub(super) struct Owner(Cell<bool>);

impl Owner {
    pub(super) fn retire(&self) {
        self.0.set(true);
    }

    fn matches(owner: &Rc<Self>, current: &Rc<Self>) -> bool {
        !owner.0.get() && Rc::ptr_eq(owner, current)
    }
}

/// First shell slice: opaque, single-plane XRGB, with no transfer/readback path.
/// Ownership crosses into the renderer, not a borrowed FD whose caller can drop.
#[derive(Debug)]
pub struct NativeShellCaptureSource {
    pub fd: OwnedFd,
    pub width: u32,
    pub height: u32,
    pub modifier: u64,
    pub offset: u32,
    pub stride: u32,
}

impl NativeShellCaptureSource {
    fn frame(&self) -> NativeMultiPlaneDmaBufFrame<'_> {
        NativeMultiPlaneDmaBufFrame {
            width: self.width,
            height: self.height,
            format: 0x3432_5258,
            modifier: self.modifier,
            plane_count: 1,
            planes: [
                Some(NativeDmaBufPlane {
                    fd: self.fd.as_fd(),
                    offset: self.offset,
                    stride: self.stride,
                }),
                None,
                None,
                None,
            ],
        }
    }
}

/// Capacity must already be reserved from this budget before source submission.
/// This request is metadata/custody only, not a completion or admission proof.
#[derive(Debug)]
pub struct NativeShellCaptureRequest {
    pub source: NativeShellCaptureSource,
    pub image_id: NativeRendererImageId,
    pub budget: Arc<NativeRendererSnapshotBudget>,
    pub reservation: NativeRendererSnapshotReservation,
    pub epoch: Arc<NativeRendererSnapshotEpoch>,
}

impl NativeShellCaptureRequest {
    fn preflight(&self) -> bool {
        self.image_id.is_valid()
            && self.epoch.is_valid()
            && self.reservation.belongs_to(&self.budget)
            && self.source.frame().is_valid()
            && (self.source.modifier != 0
                || self
                    .source
                    .width
                    .checked_mul(4)
                    .is_some_and(|row| self.source.stride >= row))
            && i32::try_from(self.source.width).is_ok()
            && i32::try_from(self.source.height).is_ok()
            && u64::from(self.source.width)
                .checked_mul(u64::from(self.source.height))
                .and_then(|n| n.checked_mul(4))
                .is_some_and(|n| n > 0 && n <= self.reservation.byte_bound())
    }
}

struct Submitted {
    request: NativeShellCaptureRequest,
    capture: Option<NativePooledCapture>,
}

/// Renderer-thread-bound, non-cloneable and tied to one context identity.
/// Dropping before completion quarantines, rather than releases, its custody.
#[must_use]
pub struct NativePendingShellCapture {
    owner: Rc<Owner>,
    custody: Escrow<Submitted>,
}

#[must_use]
pub enum NativeShellCaptureStart {
    Pending(NativePendingShellCapture),
    /// No source-using operation was attempted. Returns the original custody.
    Refused {
        request: NativeShellCaptureRequest,
        detail: NativeGbmScanoutBufferExportDetail,
    },
    /// Source/reservation retained, no source-release credit, admission stopped.
    Uncertain(NativeGbmScanoutBufferExportDetail),
}

#[must_use]
pub enum NativeShellCapturePoll {
    Pending(NativePendingShellCapture),
    Completed(NativeShellCaptureCompletion),
    Uncertain(NativeGbmScanoutBufferExportDetail),
}

/// Constructor and fields are private. Only an exact-copy fence observation
/// can return this receipt, even if snapshot export/retention subsequently fails.
#[must_use]
pub struct NativeShellCaptureCompletion {
    source: NativeShellCaptureSource,
    result: Result<Arc<NativeRendererImageSnapshot>, NativeShellCaptureOutputFailure>,
}

#[derive(Debug)]
pub struct NativeShellCaptureOutputFailure {
    pub detail: NativeGbmScanoutBufferExportDetail,
    pub reservation: NativeRendererSnapshotReservation,
}

impl NativeShellCaptureCompletion {
    /// Source reads have ended; publishing the result still requires the shell
    /// owner's grant/epoch/cancellation checks. Failure returns unused capacity.
    pub fn into_parts(
        self,
    ) -> (
        NativeShellCaptureSource,
        Result<Arc<NativeRendererImageSnapshot>, NativeShellCaptureOutputFailure>,
    ) {
        (self.source, self.result)
    }
}

impl<T: AsFd> NativeGbmRenderedScanoutContext<T> {
    /// Standalone capture on the existing owner and capture pool. No second
    /// renderer/device/thread, frame assembly, promotion or CPU fallback.
    pub fn begin_shell_capture(
        &mut self,
        request: NativeShellCaptureRequest,
    ) -> NativeShellCaptureStart {
        use NativeGbmScanoutBufferExportDetail as E;
        if !request.preflight() || !self.snapshot_reuse_enabled || self.shell_capture_owner.0.get()
        {
            return NativeShellCaptureStart::Refused {
                request,
                detail: E::InvalidTarget,
            };
        }
        let Some(slot) = Capacity::process().reserve() else {
            return NativeShellCaptureStart::Refused {
                request,
                detail: E::RendererImageStoreFull,
            };
        };
        let other_bytes = self
            .renderer_images
            .values()
            .filter(|image| image.pooled.is_none())
            .fold(self.image_bridge_bytes(), |sum, image| {
                sum.saturating_add(image.bytes)
            });
        let other_count = self
            .renderer_images
            .values()
            .filter(|image| image.pooled.is_none())
            .count();
        let mut custody = Escrow::new(
            Submitted {
                request,
                capture: None,
            },
            slot,
        );
        let request = &custody.payload().request;
        // Even an error here is conservatively uncertain. No imported source
        // is returned based only on an error classification or EGL destruction.
        let result = self.capture_reuse.capture_bounded(
            &self.egl,
            self.display,
            &self.gbm_device,
            request.source.frame(),
            DEFAULT_NATIVE_RENDERER_IMAGE_BYTE_BUDGET.saturating_sub(other_bytes),
            DEFAULT_NATIVE_RENDERER_IMAGE_CAPACITY.saturating_sub(other_count),
            request.reservation.byte_bound(),
        );
        match result {
            Ok(Some(capture)) => {
                custody.payload_mut().capture = Some(capture);
                NativeShellCaptureStart::Pending(NativePendingShellCapture {
                    owner: self.shell_capture_owner.clone(),
                    custody,
                })
            }
            Ok(None) => NativeShellCaptureStart::Uncertain(E::InvalidTarget),
            Err(detail) => NativeShellCaptureStart::Uncertain(detail),
        }
    }

    /// Exactly one zero-timeout fence poll. The owner must arrange another turn
    /// while pending; timeout, cancellation and epoch change are not completion.
    pub fn poll_shell_capture(
        &mut self,
        mut pending: NativePendingShellCapture,
    ) -> NativeShellCapturePoll {
        use NativeGbmScanoutBufferExportDetail as E;
        if !Owner::matches(&pending.owner, &self.shell_capture_owner) {
            return NativeShellCapturePoll::Uncertain(E::InvalidTarget);
        }
        let captured = pending
            .custody
            .payload_mut()
            .capture
            .as_mut()
            .expect("submitted capture");
        let proof = match captured.observe_shell_copy_completion() {
            Ok(None) => return NativeShellCapturePoll::Pending(pending),
            Ok(Some(proof)) => proof,
            Err(_) => return NativeShellCapturePoll::Uncertain(E::InvalidTarget),
        };
        if !captured
            .copy_completion
            .is_some_and(|sync| proof.is_for(&captured.allocation, &captured.generation, &sync))
        {
            return NativeShellCapturePoll::Uncertain(E::InvalidTarget);
        }
        // No custody can escape above this point without the native proof.
        let Submitted { request, capture } = pending.custody.finish();
        let mut captured = capture.expect("completed capture");
        let snapshot = export_copy(&request, &captured);
        if let Some((sync, use_guard)) = captured.take_copy_completion() {
            let execution = &captured.allocation.execution;
            if unsafe { execution.egl.destroy_sync(execution.display, sync) }.is_err() {
                captured.generation.abandon_reuse();
            }
            drop(use_guard);
        }
        let result = match snapshot {
            Ok(snapshot) => request
                .budget
                .retain_reserved(snapshot, request.epoch, request.reservation)
                .map_err(|failure| NativeShellCaptureOutputFailure {
                    detail: failure.detail.into(),
                    reservation: failure.reservation,
                }),
            Err(detail) => Err(NativeShellCaptureOutputFailure {
                detail,
                reservation: request.reservation,
            }),
        };
        NativeShellCapturePoll::Completed(NativeShellCaptureCompletion {
            source: request.source,
            result,
        })
    }
}

fn export_copy(
    request: &NativeShellCaptureRequest,
    captured: &NativePooledCapture,
) -> Result<NativeRendererImageSnapshot, NativeGbmScanoutBufferExportDetail> {
    // Duplicated plane descriptors may outlive the native allocation wrapper.
    captured.generation.mark_exported();
    let buffer = &captured.allocation.buffer;
    let mut fds = buffer.export_plane_fds()?.into_plane_fds();
    let pitches = buffer.plane_pitches();
    let offsets = buffer.plane_offsets();
    let planes = std::array::from_fn(|i| {
        fds[i].take().map(|fd| NativeOwnedDmaBufPlane {
            fd,
            offset: offsets[i],
            stride: pitches[i],
        })
    });
    if planes[..usize::from(buffer.plane_count())]
        .iter()
        .any(Option::is_none)
    {
        return Err(NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor);
    }
    Ok(NativeRendererImageSnapshot::uncharged(
        request.image_id,
        NativeOwnedSnapshotData {
            width: buffer.width(),
            height: buffer.height(),
            format: buffer.format(),
            modifier: buffer
                .modifier()
                .unwrap_or(u64::from(gbm::Modifier::Invalid)),
            plane_count: buffer.plane_count(),
            planes,
        },
    ))
}

#[cfg(test)]
#[path = "../../../../tests/support/shell_capture_admission.rs"]
mod tests;
