use sophia_engine::{
    HeadCompositionPlan, HeadCompositorCommand, HeadSamplingClass, compositor_border_bands,
    head_output_damage_snapshot, head_sampling_class,
};
use sophia_protocol::{BufferSource, Rect, Size, SurfaceId, Transform};

use crate::{
    LiveCompositionPlacement, LiveCompositionTrace, LiveCpuPresentationLayer,
    LiveOwnedMixedCompositionFrame, LiveOwnedMixedCompositionLayer, LiveOwnedMultiPlaneDmaBufFrame,
    LiveRendererImageId, LiveSharedCpuBufferSource,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveHeadCompositionLoweringError {
    MissingCpuSource(u64),
    UnsupportedSource(BufferSource),
    /// The plan measured this buffer at one size and the source holds another.
    ///
    /// It carries both sizes and the surface because the handle alone cannot
    /// say which record is stale: a plan is written from committed content,
    /// while a retained projection holds the pixels of whatever last reached a
    /// screen. When a surface commits a new buffer that no head has composed
    /// yet, those two disagree, and the session ends over a number nobody can
    /// trace without seeing both.
    SourceSizeMismatch {
        surface: SurfaceId,
        handle: u64,
        planned: Size,
        held: Size,
    },
    DuplicateSurface,
    MissingPlannedSurface,
    MissingSource(BufferSource),
    SourceKindMismatch(BufferSource),
    DmaBufCloneFailed(u64),
    IndicatorRasterFailed,
    TextRasterFailed,
}

impl core::fmt::Display for LiveHeadCompositionLoweringError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for LiveHeadCompositionLoweringError {}

#[derive(Debug)]
pub enum LiveOwnedHeadCompositionSourceKind {
    Cpu(LiveSharedCpuBufferSource),
    DmaBuf {
        image_id: LiveRendererImageId,
        frame: LiveOwnedMultiPlaneDmaBufFrame,
    },
    RendererImage {
        image_id: LiveRendererImageId,
        size: Size,
        format: u32,
    },
}

/// One authority-owned source realization available to the per-head lowerer.
///
/// The logical source identity stays the committed `BufferSource`. DMA-BUF file
/// descriptors are duplicated for each head plan, while retained renderer-image
/// identities refer to independently initialized per-head exporter stores.
#[derive(Debug)]
pub struct LiveOwnedHeadCompositionSource {
    pub surface: SurfaceId,
    pub source: BufferSource,
    pub kind: LiveOwnedHeadCompositionSourceKind,
}

/// Lowers one immutable Engine plan into an owned, head-native renderer frame.
///
/// This first production consumer admits CPU-authority variants and Engine
/// solids. DMA-BUF and retained renderer-image variants remain unavailable
/// here until their lease resolver can return an independently owned source for
/// every head; they are rejected rather than flattened or silently omitted.
/// The plan's verdict, demoted when the lowering chose a source the plane
/// cannot take.
///
/// Without this, the backend's re-derivation refused the frame and counted it
/// as Engine's proof contradicting the pixels -- a defect -- when both sides
/// were right in their own vocabulary. The disagreement was real and lived
/// here, in the seam neither of them owns.
fn lowered_direct_scanout(
    plan: &HeadCompositionPlan,
    layers: &[LiveOwnedMixedCompositionLayer],
) -> sophia_engine::DirectScanoutVerdict {
    if !plan.direct_scanout.is_eligible() {
        return plan.direct_scanout;
    }
    let client_buffer = matches!(
        layers.first(),
        Some(LiveOwnedMixedCompositionLayer::DmaBuf { .. })
    ) && layers.len() == 1;
    if client_buffer {
        plan.direct_scanout
    } else {
        sophia_engine::DirectScanoutVerdict::CompositionRequired("retained_source")
    }
}

pub fn lower_cpu_head_composition_plan(
    plan: &HeadCompositionPlan,
    sources: &[LiveCpuPresentationLayer],
) -> Result<LiveOwnedMixedCompositionFrame, LiveHeadCompositionLoweringError> {
    let mut indicator_cache = crate::IndicatorStripRasterCache::default();
    let mut text_cache = crate::CompositorTextRasterCache::default();
    lower_cpu_head_composition_plan_with_caches(
        plan,
        sources,
        &mut indicator_cache,
        &mut text_cache,
    )
}

pub fn lower_cpu_head_composition_plan_with_indicator_cache(
    plan: &HeadCompositionPlan,
    sources: &[LiveCpuPresentationLayer],
    indicator_cache: &mut crate::IndicatorStripRasterCache,
) -> Result<LiveOwnedMixedCompositionFrame, LiveHeadCompositionLoweringError> {
    let mut text_cache = crate::CompositorTextRasterCache::default();
    lower_cpu_head_composition_plan_with_caches(plan, sources, indicator_cache, &mut text_cache)
}

pub fn lower_cpu_head_composition_plan_with_caches(
    plan: &HeadCompositionPlan,
    sources: &[LiveCpuPresentationLayer],
    indicator_cache: &mut crate::IndicatorStripRasterCache,
    text_cache: &mut crate::CompositorTextRasterCache,
) -> Result<LiveOwnedMixedCompositionFrame, LiveHeadCompositionLoweringError> {
    let sources = sources
        .iter()
        .map(|source| LiveOwnedHeadCompositionSource {
            surface: source.surface,
            source: BufferSource::CpuBuffer {
                handle: source.buffer.handle,
            },
            kind: LiveOwnedHeadCompositionSourceKind::Cpu(source.buffer.clone().into()),
        })
        .collect::<Vec<_>>();
    lower_head_composition_plan_with_caches(plan, &sources, indicator_cache, text_cache)
}

/// Lowers a complete immutable Engine plan using authority-owned source
/// realizations. No geometry is inherited from a primary head: every placement,
/// clip, compositor primitive, and damage snapshot comes from `plan`.
pub fn lower_head_composition_plan(
    plan: &HeadCompositionPlan,
    sources: &[LiveOwnedHeadCompositionSource],
) -> Result<LiveOwnedMixedCompositionFrame, LiveHeadCompositionLoweringError> {
    let mut indicator_cache = crate::IndicatorStripRasterCache::default();
    let mut text_cache = crate::CompositorTextRasterCache::default();
    lower_head_composition_plan_with_caches(plan, sources, &mut indicator_cache, &mut text_cache)
}

pub fn lower_head_composition_plan_with_indicator_cache(
    plan: &HeadCompositionPlan,
    sources: &[LiveOwnedHeadCompositionSource],
    indicator_cache: &mut crate::IndicatorStripRasterCache,
) -> Result<LiveOwnedMixedCompositionFrame, LiveHeadCompositionLoweringError> {
    let mut text_cache = crate::CompositorTextRasterCache::default();
    lower_head_composition_plan_with_caches(plan, sources, indicator_cache, &mut text_cache)
}

pub fn lower_head_composition_plan_with_caches(
    plan: &HeadCompositionPlan,
    sources: &[LiveOwnedHeadCompositionSource],
    indicator_cache: &mut crate::IndicatorStripRasterCache,
    text_cache: &mut crate::CompositorTextRasterCache,
) -> Result<LiveOwnedMixedCompositionFrame, LiveHeadCompositionLoweringError> {
    let mut layers = Vec::with_capacity(plan.compositor.len().saturating_mul(4));
    let mut emitted_surfaces = std::collections::BTreeSet::new();
    for command in &plan.compositor {
        match command {
            HeadCompositorCommand::SurfacePreview(preview) => {
                let mut binding = plan
                    .layers
                    .iter()
                    .find(|b| b.surface == preview.surface)
                    .ok_or(LiveHeadCompositionLoweringError::MissingPlannedSurface)?
                    .clone();
                binding.native_geometry = preview.geometry;
                binding.native_clip = preview.clip;
                binding.requested_sampling = head_sampling_class(
                    binding.source_pixel_size,
                    Size {
                        width: preview.geometry.width,
                        height: preview.geometry.height,
                    },
                );
                let source = sources
                    .iter()
                    .find(|source| {
                        source.surface == preview.surface && source.source == binding.source
                    })
                    .ok_or(LiveHeadCompositionLoweringError::MissingSource(
                        binding.source,
                    ))?;
                layers.push(lower_surface_source(&binding, source)?);
            }
            HeadCompositorCommand::Background(background) => {
                layers.push(LiveOwnedMixedCompositionLayer::Solid {
                    geometry: background.geometry,
                    color: background.color,
                });
            }
            HeadCompositorCommand::Surface { surface } => {
                if !emitted_surfaces.insert(*surface) {
                    return Err(LiveHeadCompositionLoweringError::DuplicateSurface);
                }
                let binding = plan
                    .layers
                    .iter()
                    .find(|binding| binding.surface == *surface)
                    .ok_or(LiveHeadCompositionLoweringError::MissingPlannedSurface)?;
                let source = sources
                    .iter()
                    .find(|source| source.surface == *surface && source.source == binding.source)
                    .ok_or(match binding.source {
                        BufferSource::CpuBuffer { handle } => {
                            LiveHeadCompositionLoweringError::MissingCpuSource(handle)
                        }
                        source => LiveHeadCompositionLoweringError::MissingSource(source),
                    })?;
                layers.push(lower_surface_source(binding, source)?);
            }
            HeadCompositorCommand::Border(border) => {
                for band in compositor_border_bands(sophia_engine::CompositorBorder {
                    node: border.node,
                    generation: border.generation,
                    outer: border.outer,
                    inner: border.inner,
                    color: border.color,
                }) {
                    // Each band clipped, never only the rects they came from. A
                    // band is the difference between `outer` and `inner`, and
                    // clipping those two first does not reliably remove
                    // anything: where the clip leaves them degenerate the
                    // subtraction is still positive, so a window entirely
                    // outside this scene keeps a band at its original off-screen
                    // coordinates. Clipping the result is what bounds it.
                    let geometry = intersect_rect(band.geometry, border.clip);
                    if !geometry.is_empty() {
                        layers.push(LiveOwnedMixedCompositionLayer::Solid {
                            geometry,
                            color: band.color,
                        });
                    }
                }
            }
            HeadCompositorCommand::Rect(rect) => {
                if !rect.geometry.is_empty() {
                    if rect.opacity == 255 {
                        layers.push(LiveOwnedMixedCompositionLayer::Solid {
                            geometry: rect.geometry,
                            color: rect.color,
                        });
                    } else {
                        layers.push(LiveOwnedMixedCompositionLayer::Cpu {
                            buffer: crate::solid_color_buffer(rect.color),
                            placement: LiveCompositionPlacement {
                                target: rect.geometry,
                                clip: Some(rect.geometry),
                                transform: Transform::IDENTITY,
                                alpha: f32::from(rect.opacity) / 255.0,
                                sampling: HeadSamplingClass::Exact,
                            },
                        });
                    }
                }
            }
            HeadCompositorCommand::Text(text) => {
                if !text.geometry.is_empty() {
                    let buffer = text_cache
                        .raster_for(text)
                        .map_err(|_| LiveHeadCompositionLoweringError::TextRasterFailed)?;
                    layers.push(LiveOwnedMixedCompositionLayer::Cpu {
                        buffer,
                        placement: LiveCompositionPlacement {
                            target: text.geometry,
                            clip: Some(text.geometry),
                            transform: Transform::IDENTITY,
                            alpha: 1.0,
                            sampling: HeadSamplingClass::Exact,
                        },
                    });
                }
            }
            HeadCompositorCommand::IndicatorStrip(strip) => {
                let buffer = indicator_cache
                    .raster_for(strip)
                    .map_err(|_| LiveHeadCompositionLoweringError::IndicatorRasterFailed)?;
                layers.push(LiveOwnedMixedCompositionLayer::Cpu {
                    buffer,
                    placement: LiveCompositionPlacement {
                        target: strip.strip.geometry,
                        clip: Some(strip.strip.geometry),
                        transform: Transform::IDENTITY,
                        alpha: 1.0,
                        sampling: HeadSamplingClass::Exact,
                    },
                });
            }
            HeadCompositorCommand::ContentImage(content) => {
                if !content.clip.is_empty() {
                    let identity = content.image.resource.description().resource;
                    layers.push(LiveOwnedMixedCompositionLayer::Cpu {
                        buffer: LiveSharedCpuBufferSource {
                            handle: shell_content_handle(identity),
                            size: content.image.size_px,
                            stride: content.image.stride,
                            format: content.image.format,
                            generation: content.image.generation,
                            bytes: content.image.resource.clone().into(),
                        },
                        placement: LiveCompositionPlacement {
                            target: content.geometry,
                            clip: Some(content.clip),
                            transform: Transform::IDENTITY,
                            alpha: 1.0,
                            sampling: head_sampling_class(
                                content.image.size_px,
                                Size {
                                    width: content.geometry.width,
                                    height: content.geometry.height,
                                },
                            ),
                        },
                    });
                }
            }
        }
    }
    if let Some(cursor) = plan.cursor {
        let source = sources
            .iter()
            .find(|source| source.source == cursor.source)
            .ok_or(match cursor.source {
                BufferSource::CpuBuffer { handle } => {
                    LiveHeadCompositionLoweringError::MissingCpuSource(handle)
                }
                source => LiveHeadCompositionLoweringError::MissingSource(source),
            })?;
        let placement = LiveCompositionPlacement {
            target: cursor.geometry,
            clip: Some(cursor.geometry),
            transform: Transform::IDENTITY,
            alpha: 1.0,
            sampling: HeadSamplingClass::Exact,
        };
        layers.push(lower_owned_source(
            cursor.source,
            source,
            placement,
            source_size(source)?,
        )?);
    }
    let mut output_damage_snapshot = head_output_damage_snapshot(plan);
    for surface in &mut output_damage_snapshot.surfaces {
        if let Some(source) = sources
            .iter()
            .find(|source| source.surface == surface.surface)
        {
            surface.source_size = source_size(source)?;
        }
    }
    let direct_scanout = lowered_direct_scanout(plan, &layers);
    Ok(LiveOwnedMixedCompositionFrame {
        layers,
        output_damage_snapshot: Some(output_damage_snapshot),
        trace: Some(LiveCompositionTrace {
            output: plan.output,
            head: plan.head,
            scene_generation: plan.scene_generation,
        }),
        // Engine's verdict on this exact plan, carried down with the pixels it
        // describes -- but only while it stays true of them. Lowering adds no
        // layer the plan did not command; what it does do is *choose a source
        // kind*, and a retained recompose substitutes the compositor's
        // promoted snapshot for the client's buffer. That frame is
        // structurally eligible and still cannot go to a plane: the snapshot
        // is not the client's memory, and handing it over would display the
        // copy while claiming the ownership contract of the original. Engine
        // cannot see this -- source-kind selection is a lowering decision --
        // so the demotion happens here, where the choice is made.
        direct_scanout,
    })
}

fn shell_content_handle(resource: sophia_protocol::ContentResourceId) -> u64 {
    resource.id.rotate_left(17) ^ resource.generation ^ (1_u64 << 63)
}

/// The overlap of two head-native rects, empty when they do not meet.
fn intersect_rect(first: Rect, second: Rect) -> Rect {
    let left = first.x.max(second.x);
    let top = first.y.max(second.y);
    let right = first
        .x
        .saturating_add(first.width)
        .min(second.x.saturating_add(second.width));
    let bottom = first
        .y
        .saturating_add(first.height)
        .min(second.y.saturating_add(second.height));
    Rect {
        x: left,
        y: top,
        width: right.saturating_sub(left).max(0),
        height: bottom.saturating_sub(top).max(0),
    }
}

fn lower_surface_source(
    binding: &sophia_engine::HeadLayerBinding,
    source: &LiveOwnedHeadCompositionSource,
) -> Result<LiveOwnedMixedCompositionLayer, LiveHeadCompositionLoweringError> {
    lower_owned_source(
        binding.source,
        source,
        LiveCompositionPlacement {
            target: binding.native_geometry,
            clip: Some(binding.native_clip),
            transform: Transform::IDENTITY,
            alpha: f32::from(binding.opacity_millis) / 1_000.0,
            sampling: binding.requested_sampling,
        },
        binding.source_pixel_size,
    )
}

fn source_size(
    source: &LiveOwnedHeadCompositionSource,
) -> Result<Size, LiveHeadCompositionLoweringError> {
    match &source.kind {
        LiveOwnedHeadCompositionSourceKind::Cpu(buffer) => Ok(buffer.size),
        LiveOwnedHeadCompositionSourceKind::DmaBuf { frame, .. } => Ok(Size {
            width: i32::try_from(frame.width)
                .map_err(|_| LiveHeadCompositionLoweringError::SourceKindMismatch(source.source))?,
            height: i32::try_from(frame.height)
                .map_err(|_| LiveHeadCompositionLoweringError::SourceKindMismatch(source.source))?,
        }),
        LiveOwnedHeadCompositionSourceKind::RendererImage { size, .. } => Ok(*size),
    }
}

/// Whether this source is the buffer a plan measured, or a copy of one.
pub const fn requires_exact_source_size(kind: &LiveOwnedHeadCompositionSourceKind) -> bool {
    match kind {
        LiveOwnedHeadCompositionSourceKind::Cpu(_)
        | LiveOwnedHeadCompositionSourceKind::DmaBuf { .. } => true,
        LiveOwnedHeadCompositionSourceKind::RendererImage { .. } => false,
    }
}

fn lower_owned_source(
    expected: BufferSource,
    source: &LiveOwnedHeadCompositionSource,
    mut placement: LiveCompositionPlacement,
    expected_size: Size,
) -> Result<LiveOwnedMixedCompositionLayer, LiveHeadCompositionLoweringError> {
    if source.source != expected {
        return Err(LiveHeadCompositionLoweringError::MissingSource(expected));
    }
    let actual_size = source_size(source)?;
    placement.sampling = head_sampling_class(
        actual_size,
        Size {
            width: placement.target.width,
            height: placement.target.height,
        },
    );
    // A renderer image is the compositor's own copy of an earlier generation,
    // carried under the identity of the surface's committed buffer. Its size is
    // its own fact, not a measurement of that buffer, so comparing the two asks
    // a question with no answer: they agree only while the surface has not
    // resized, and a mirror member already draws every retained image at a size
    // of its own. Placement carries it to the head either way. The comparison
    // stays for CPU and DMA-BUF sources, where the plan measured the very
    // buffer being handed over and a difference means the wrong one arrived.
    if requires_exact_source_size(&source.kind) && actual_size != expected_size {
        return Err(LiveHeadCompositionLoweringError::SourceSizeMismatch {
            surface: source.surface,
            handle: match expected {
                BufferSource::CpuBuffer { handle } | BufferSource::DmaBuf { handle } => handle,
                BufferSource::None | BufferSource::XPixmap { .. } => 0,
            },
            planned: expected_size,
            held: actual_size,
        });
    }
    match (&source.kind, expected) {
        (LiveOwnedHeadCompositionSourceKind::Cpu(buffer), BufferSource::CpuBuffer { .. }) => {
            Ok(LiveOwnedMixedCompositionLayer::Cpu {
                buffer: buffer.clone(),
                placement,
            })
        }
        (
            LiveOwnedHeadCompositionSourceKind::DmaBuf { image_id, frame },
            BufferSource::DmaBuf { handle },
        ) => Ok(LiveOwnedMixedCompositionLayer::DmaBuf {
            image_id: *image_id,
            frame: frame
                .try_clone()
                .map_err(|_| LiveHeadCompositionLoweringError::DmaBufCloneFailed(handle))?,
            placement,
        }),
        (
            LiveOwnedHeadCompositionSourceKind::RendererImage {
                image_id,
                size,
                format,
            },
            BufferSource::DmaBuf { .. },
        ) => Ok(LiveOwnedMixedCompositionLayer::RendererImage {
            image_id: *image_id,
            size: *size,
            format: *format,
            placement,
        }),
        _ => Err(LiveHeadCompositionLoweringError::SourceKindMismatch(
            expected,
        )),
    }
}
