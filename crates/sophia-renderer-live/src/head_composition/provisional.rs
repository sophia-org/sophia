//! G3-3: a combined frame job's metadata lowering. A candidate image is a
//! `CompositorImageSource::Provisional`: it is recorded as a placeholder at
//! its final layer index and lowers to no layer, so G3-2's bind fills it
//! with the validated pixels. Every other command lowers through the same
//! arms as conventional lowering, except that rasters are only read from
//! warm caches. A cold raster or a translucent rect is recorded as the
//! composition's decline and lowers to no layer, and lowering goes on, so a
//! declined composition still carries every other fact for classification.
//! Nothing here rasterizes, mints a pixel version, or changes a cache (R5).

use super::{
    LiveHeadCompositionLoweringError, LiveOwnedHeadCompositionSource, Rasters, lower_with,
};
use crate::{LiveCompositionPlacement, LiveOwnedMixedCompositionFrame, LiveSharedCpuBufferSource};
use sophia_engine::{
    CompositorContentImage, CompositorImageSource, CompositorImageSourceIdentity, CompositorNodeId,
    HeadCompositionPlan, HeadCompositorIndicatorStrip, HeadCompositorText,
};
use sophia_protocol::Size;

/// Why a metadata lowering cannot be bound: ineligibility, not a fault.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProvisionalDecline {
    /// A text raster not already cached; rasterizing it is pixel work.
    ColdTextRaster,
    /// An indicator strip raster not already cached.
    ColdIndicatorRaster,
    /// A translucent rect's buffer is new pixels under a new version each
    /// time, so no two compositions of one scene would agree.
    TranslucentRect,
    /// The final layer index does not fit. This one is returned as an error.
    LayerIndexOverflow,
}

/// One content image occurrence that survived the lowering's skip rules,
/// with everything its layer is rendered from.
#[derive(Clone, Debug)]
pub struct LiveImageOccurrence {
    /// Its position in the frame once every earlier placeholder is bound.
    pub index: usize,
    pub node: CompositorNodeId,
    pub resource: CompositorImageSourceIdentity,
    pub placement: LiveCompositionPlacement,
    pub size: Size,
    pub stride: u32,
    pub format: u32,
    pub handle: u64,
    pub generation: u64,
    /// A candidate placeholder: recorded, not lowered as a layer.
    pub provisional: bool,
}

/// A provisional frame: every layer but the placeholders, plus every image
/// occurrence in order.
pub struct LiveProvisionalComposition {
    pub frame: LiveOwnedMixedCompositionFrame,
    pub images: Vec<LiveImageOccurrence>,
    /// The first layer the lowering could not read. Its layer is absent, so
    /// such a frame is never bound; it is only classified.
    pub declined: Option<LiveProvisionalDecline>,
}

pub(super) struct Peek<'a> {
    indicator: &'a crate::IndicatorStripRasterCache,
    text: &'a crate::CompositorTextRasterCache,
    images: &'a mut Vec<LiveImageOccurrence>,
    declined: &'a mut Option<LiveProvisionalDecline>,
    placeholders: usize,
}

fn ineligible(reason: LiveProvisionalDecline) -> LiveHeadCompositionLoweringError {
    LiveHeadCompositionLoweringError::ProvisionalIneligible(reason)
}

impl Peek<'_> {
    /// Records the composition's first decline; its layer lowers to none.
    pub(super) fn decline(
        &mut self,
        reason: LiveProvisionalDecline,
    ) -> Option<LiveSharedCpuBufferSource> {
        self.declined.get_or_insert(reason);
        None
    }

    pub(super) fn text(&mut self, text: &HeadCompositorText) -> Option<LiveSharedCpuBufferSource> {
        let warm = self.text.peek(text);
        warm.or_else(|| self.decline(LiveProvisionalDecline::ColdTextRaster))
    }

    pub(super) fn indicator(
        &mut self,
        strip: &HeadCompositorIndicatorStrip,
    ) -> Option<LiveSharedCpuBufferSource> {
        let warm = self.indicator.peek(strip);
        warm.or_else(|| self.decline(LiveProvisionalDecline::ColdIndicatorRaster))
    }

    /// Records one surviving occurrence placed after `layers` lowered
    /// layers; true when it is a placeholder.
    pub(super) fn record(
        &mut self,
        layers: usize,
        image: &CompositorContentImage,
        placement: LiveCompositionPlacement,
        handle: u64,
    ) -> Result<bool, LiveHeadCompositionLoweringError> {
        let index = layers
            .checked_add(self.placeholders)
            .ok_or(ineligible(LiveProvisionalDecline::LayerIndexOverflow))?;
        let provisional = matches!(image.resource, CompositorImageSource::Provisional(_));
        self.images.push(LiveImageOccurrence {
            index,
            node: image.node,
            resource: image.resource.identity(),
            placement,
            size: image.size_px,
            stride: image.stride,
            format: image.format,
            handle,
            generation: image.generation,
            provisional,
        });
        self.placeholders += usize::from(provisional);
        Ok(provisional)
    }
}

/// Lowers `plan` for a combined frame job without pixels for its candidate
/// images, reading the raster caches only. A cold raster or a translucent
/// rect is recorded in `declined`; only an index overflow is an error.
pub fn lower_provisional_head_composition_plan(
    plan: &HeadCompositionPlan,
    sources: &[LiveOwnedHeadCompositionSource],
    indicator_cache: &crate::IndicatorStripRasterCache,
    text_cache: &crate::CompositorTextRasterCache,
) -> Result<LiveProvisionalComposition, LiveHeadCompositionLoweringError> {
    let mut images = Vec::new();
    let mut declined = None;
    let frame = lower_with(
        plan,
        sources,
        Rasters::Peek(Peek {
            indicator: indicator_cache,
            text: text_cache,
            images: &mut images,
            declined: &mut declined,
            placeholders: 0,
        }),
    )?;
    Ok(LiveProvisionalComposition {
        frame,
        images,
        declined,
    })
}
