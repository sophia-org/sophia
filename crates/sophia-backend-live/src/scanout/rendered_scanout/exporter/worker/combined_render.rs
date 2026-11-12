//! G3-2: the renderer binds a combined job's validated pixels into its frame
//! only after it holds a slot (GATE3-SCOPE.md G3-2 and its source decisions).
//! The plan's partial frame never leaves this module unbound: `bind` is the
//! only way to a `BoundFrame`, and it fills every placeholder or none.

use super::super::super::LiveRendererFrameSlotAcquire;
use super::*;
use sophia_protocol::ContentResourceBegin;
use sophia_renderer_live::{
    LiveCompositionPlacement, LiveCpuPixelStorage, LiveOwnedMixedCompositionLayer,
    LiveSharedCpuBufferSource,
};

/// One lowered occurrence of the copied resource, frozen at service 1. The
/// bound layer is exactly the Cpu layer lowering makes for a resident image,
/// with the validated pixels as its bytes.
#[derive(Clone, Copy, Debug)]
pub struct CombinedPlaceholder {
    /// Where the layer sits in the bound frame's final layer list.
    pub(crate) index: usize,
    // Read by G3-3's scene key; G3-2 binds by index and reads it nowhere.
    #[allow(dead_code)]
    pub(crate) node: sophia_engine::CompositorNodeId,
    pub(crate) placement: LiveCompositionPlacement,
    pub(crate) size: sophia_protocol::Size,
    pub(crate) stride: u32,
    pub(crate) format: u32,
    pub(crate) handle: u64,
    pub(crate) generation: u64,
}

/// The renderer's format for a protocol image format. The protocol defines
/// only format 1, premultiplied ARGB, which session projection also lowers
/// to DRM ARGB8888; the two encodings are never compared directly.
pub(in super::super) fn renderer_format(pixel_format: u16) -> Option<u32> {
    (pixel_format == 1).then_some(sophia_protocol::DRM_FORMAT_ARGB8888)
}

/// Whether the job's admitted description admits these placeholders and
/// bytes. A job without a description (a sealed ISB job) may bind nothing.
pub(in super::super) fn admits(
    placeholders: &[CombinedPlaceholder],
    description: Option<&ContentResourceBegin>,
    bytes: usize,
) -> bool {
    let Some(description) = description else {
        return placeholders.is_empty();
    };
    if u64::try_from(bytes).ok() != Some(description.total_bytes) {
        return false;
    }
    let (Some(stride), Some(format)) = (
        description.width_px.checked_mul(4),
        renderer_format(description.pixel_format),
    ) else {
        return false;
    };
    placeholders.iter().all(|placeholder| {
        u32::try_from(placeholder.size.width).ok() == Some(description.width_px)
            && u32::try_from(placeholder.size.height).ok() == Some(description.height_px)
            && placeholder.stride == stride
            && placeholder.format == format
    })
}

/// A frame with every placeholder filled. Only `bind` makes one.
pub(in super::super) struct BoundFrame(PendingRenderedFrame);

impl BoundFrame {
    // Production renders the frame by value; tests read it in place.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(in super::super) fn frame(&self) -> &PendingRenderedFrame {
        &self.0
    }

    pub(in super::super) fn into_frame(self) -> PendingRenderedFrame {
        self.0
    }
}

/// Fills every placeholder, or returns the frame untouched. Every index is
/// checked before the first insertion, so a bad later occurrence leaves no
/// partial frame and no clone of `pixels`.
// The refused frame comes back whole and unchanged, by value.
#[allow(clippy::result_large_err)]
pub(in super::super) fn bind(
    frame: PendingRenderedFrame,
    placeholders: &[CombinedPlaceholder],
    pixels: &Arc<ValidatedIsbPixels>,
) -> Result<BoundFrame, PendingRenderedFrame> {
    if placeholders.is_empty() {
        return Ok(BoundFrame(frame));
    }
    let PendingRenderedFrame::Mixed(mut mixed, identity) = frame else {
        return Err(frame);
    };
    let fits = mixed
        .layers
        .len()
        .checked_add(placeholders.len())
        .is_some_and(|total| {
            placeholders
                .windows(2)
                .all(|pair| pair[0].index < pair[1].index)
                && placeholders.last().is_some_and(|last| last.index < total)
        });
    if !fits {
        return Err(PendingRenderedFrame::Mixed(mixed, identity));
    }
    // Ascending final indices: each insertion point is within the list as
    // it stands, because the last index is below the final length.
    for placeholder in placeholders {
        mixed.layers.insert(
            placeholder.index,
            LiveOwnedMixedCompositionLayer::Cpu {
                buffer: LiveSharedCpuBufferSource {
                    pixel_version: None,
                    handle: placeholder.handle,
                    size: placeholder.size,
                    stride: placeholder.stride,
                    format: placeholder.format,
                    generation: placeholder.generation,
                    bytes: LiveCpuPixelStorage::Validated(Arc::clone(pixels)),
                },
                placement: placeholder.placement,
            },
        );
    }
    Ok(BoundFrame(PendingRenderedFrame::Mixed(mixed, identity)))
}

/// The output's real slot table and damage, as the combined attempt uses
/// them. Production implements it for the worker's output state.
pub(in super::super) trait CombinedSlots {
    fn try_acquire(&mut self) -> LiveRendererFrameSlotAcquire;
    /// Nothing was written: the slot keeps its content and damage.
    fn release_unused(&mut self, token: LiveRendererFrameSlotToken);
    /// A failed render may have written part of its damage.
    fn release_failed(&mut self, token: LiveRendererFrameSlotToken);
}

/// What the render function is handed: a bound frame in its slot, or the
/// failure to report without rendering.
// Moved once per attempt and never stored, as RenderAttempt.
#[allow(clippy::large_enum_variant)]
pub(in super::super) enum RenderInput {
    Bound(BoundFrame, LiveRendererFrameSlotToken),
    Failed(LiveRendererScanoutBufferExportDetail),
}

/// The plan's target and output request, apart from its frame.
pub(in super::super) struct RenderTarget {
    pub(in super::super) target: LiveGbmEglFrameTargetRecord,
    pub(in super::super) preferred_modifiers: Vec<u64>,
    pub(in super::super) output_format: Option<sophia_renderer_live::LiveCompositionFormatRequest>,
}

/// Slot, then the description, then bind and render, in that order. The
/// caller has already checked the deadline and the stop flag. `render`
/// consumes the bound frame, so every clone of the pixels is gone when it
/// returns.
pub(in super::super) fn attempt<S: CombinedSlots>(
    slots: &mut S,
    plan: CombinedRenderPlan,
    pixels: ValidatedIsbPixels,
    description: Option<&ContentResourceBegin>,
    render: impl FnOnce(&mut S, RenderInput, RenderTarget) -> WorkerResult,
) -> RenderAttempt {
    let token = match slots.try_acquire() {
        LiveRendererFrameSlotAcquire::Acquired(token) => token,
        LiveRendererFrameSlotAcquire::Deferred => return RenderAttempt::NoSlot(plan, pixels),
        LiveRendererFrameSlotAcquire::IncarnationExhausted => {
            let input = RenderInput::Failed(
                LiveRendererScanoutBufferExportDetail::FrameSlotIncarnationExhausted,
            );
            let result = render(slots, input, plan.render_target());
            return RenderAttempt::Done(CombinedOutcome::Rendered(result), pixels);
        }
    };
    if !admits(&plan.placeholders, description, pixels.bytes().len()) {
        slots.release_unused(token);
        return RenderAttempt::Done(CombinedOutcome::Mismatch, pixels);
    }
    let CombinedRenderPlan {
        frame,
        placeholders,
        target,
        preferred_modifiers,
        output_format,
        ..
    } = plan;
    let request = RenderTarget {
        target,
        preferred_modifiers,
        output_format,
    };
    let pixels = Arc::new(pixels);
    let bound = match bind(frame, &placeholders, &pixels) {
        Ok(bound) => bound,
        Err(unbound) => {
            // The owner's plan, not the client's pixels, is at fault: the same
            // ComposeRefused a conventional frame gets for layers the renderer
            // cannot serve. Nothing was written to the slot.
            drop(unbound);
            slots.release_unused(token);
            let input = RenderInput::Failed(LiveRendererScanoutBufferExportDetail::ComposeRefused);
            let result = render(slots, input, request);
            return RenderAttempt::Done(CombinedOutcome::Rendered(result), unwrap_pixels(pixels));
        }
    };
    drop(placeholders);
    let result = render(slots, RenderInput::Bound(bound, token), request);
    if matches!(result.outcome, WorkerOutcome::Failed(_)) {
        slots.release_failed(token);
    }
    RenderAttempt::Done(CombinedOutcome::Rendered(result), unwrap_pixels(pixels))
}

/// Stopping condition 1 (GATE3-SCOPE D4): a reference that outlives the
/// bound frame ends the shared renderer worker. It is never copied around,
/// settled or released as if the pixels came home.
fn unwrap_pixels(pixels: Arc<ValidatedIsbPixels>) -> ValidatedIsbPixels {
    match Arc::try_unwrap(pixels) {
        Ok(pixels) => pixels,
        Err(_) => {
            panic!("combined frame job: a bound frame outlived its render (stopping condition 1)")
        }
    }
}
