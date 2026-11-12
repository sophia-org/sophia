//! G3-3: a combined frame job's metadata lowering. A candidate image is a
//! `CompositorImageSource::Provisional`: it is recorded as a placeholder at
//! its final layer index and lowers to no layer, so G3-2's bind fills it
//! with the validated pixels. Every other command lowers through the same
//! arms as conventional lowering, except that rasters are only read from
//! warm caches and a translucent rect declines. Nothing here rasterizes,
//! mints a pixel version, or changes a cache (R5).

use super::{
    LiveHeadCompositionLoweringError, LiveOwnedHeadCompositionSource, Rasters, lower_with,
};
use crate::{LiveCompositionPlacement, LiveOwnedMixedCompositionFrame, LiveSharedCpuBufferSource};
use sophia_engine::{
    CompositorContentImage, CompositorImageSource, CompositorImageSourceIdentity, CompositorNodeId,
    HeadCompositionPlan, HeadCompositorIndicatorStrip, HeadCompositorText,
};
use sophia_protocol::Size;

/// Why the metadata lowering declined a plan: ineligibility, not a fault.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProvisionalDecline {
    /// A text raster not already cached; rasterizing it is pixel work.
    ColdTextRaster,
    /// An indicator strip raster not already cached.
    ColdIndicatorRaster,
    /// A translucent rect's buffer is new pixels under a new version each
    /// time, so no two compositions of one scene would agree.
    TranslucentRect,
    /// The final layer index does not fit.
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
}

pub(super) struct Peek<'a> {
    indicator: &'a crate::IndicatorStripRasterCache,
    text: &'a crate::CompositorTextRasterCache,
    images: &'a mut Vec<LiveImageOccurrence>,
    placeholders: usize,
}

fn decline(reason: LiveProvisionalDecline) -> LiveHeadCompositionLoweringError {
    LiveHeadCompositionLoweringError::ProvisionalIneligible(reason)
}

impl Peek<'_> {
    pub(super) fn text(
        &self,
        text: &HeadCompositorText,
    ) -> Result<LiveSharedCpuBufferSource, LiveHeadCompositionLoweringError> {
        self.text
            .peek(text)
            .ok_or(decline(LiveProvisionalDecline::ColdTextRaster))
    }

    pub(super) fn indicator(
        &self,
        strip: &HeadCompositorIndicatorStrip,
    ) -> Result<LiveSharedCpuBufferSource, LiveHeadCompositionLoweringError> {
        self.indicator
            .peek(strip)
            .ok_or(decline(LiveProvisionalDecline::ColdIndicatorRaster))
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
            .ok_or(decline(LiveProvisionalDecline::LayerIndexOverflow))?;
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
/// rect declines the plan.
pub fn lower_provisional_head_composition_plan(
    plan: &HeadCompositionPlan,
    sources: &[LiveOwnedHeadCompositionSource],
    indicator_cache: &crate::IndicatorStripRasterCache,
    text_cache: &crate::CompositorTextRasterCache,
) -> Result<LiveProvisionalComposition, LiveHeadCompositionLoweringError> {
    let mut images = Vec::new();
    let frame = lower_with(
        plan,
        sources,
        Rasters::Peek(Peek {
            indicator: indicator_cache,
            text: text_cache,
            images: &mut images,
            placeholders: 0,
        }),
    )?;
    Ok(LiveProvisionalComposition { frame, images })
}
