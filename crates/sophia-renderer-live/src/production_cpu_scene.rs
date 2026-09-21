use sophia_engine::{
    CompositorDisplayCommand, CompositorDisplayList, HeadlessOutput, OutputFrameDamageSnapshot,
    OutputRepaintPlan, OutputRepaintPolicy, output_frame_damage, output_frame_damage_snapshot,
    plan_output_repaint,
};
use sophia_protocol::{BufferSource, CommittedSurfaceState, Point, Rect, Region, Size, SurfaceId};

use crate::{
    LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888, LiveCpuBufferRegistry, LiveCpuBufferSource,
    LiveCpuBufferSourceRef, LiveCpuBufferUpdate, LiveCpuComposedFrame,
    LiveCpuCompositionElementRef, LiveCpuCompositionLayer, LiveCpuCompositionLayerRef,
    LiveCpuCompositionReport, LiveCpuFrameMetricsMode,
    compose_live_cpu_display_list_frame_with_metrics_reusing_damage_and_cursor_asset,
    compose_live_cpu_frame, compose_live_cpu_frame_ref_with_cursor_asset,
};

const RETAINED_PRIMARY_CPU_FRAME_CAPACITY: usize = 3;

#[derive(Clone)]
pub struct LiveProductionComposedFrame {
    pub frame: LiveCpuComposedFrame,
    pub checksum: u64,
    pub nonzero_pixel_bytes: usize,
    pub output_damage_snapshot: Option<OutputFrameDamageSnapshot>,
}

struct RetainedPrimaryCpuFrame {
    bytes: std::sync::Arc<Vec<u8>>,
    output_damage_snapshot: OutputFrameDamageSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveCpuPresentationLayer {
    pub surface: SurfaceId,
    pub geometry: Rect,
    pub buffer: LiveCpuBufferSource,
}

pub struct LiveProductionCpuScene {
    output_size: Size,
    buffers: LiveCpuBufferRegistry,
    last_report: Option<LiveCpuCompositionReport>,
    last_output_damage_snapshot: Option<OutputFrameDamageSnapshot>,
    max_nonzero_pixel_bytes: usize,
    max_layers_composed: usize,
    nonzero_frames: usize,
    exact_pixel_proofs_remaining: usize,
    exact_pixel_metric_frames: usize,
    damage_scoped_metric_frames: usize,
    retained_primary_frames: Vec<RetainedPrimaryCpuFrame>,
    secondary_output_frames: Vec<(usize, HeadlessOutput, LiveProductionComposedFrame)>,
    indicator_strip_cache: crate::IndicatorStripRasterCache,
    text_cache: crate::CompositorTextRasterCache,
    cursor_asset: sophia_engine::CursorAsset,
}

impl LiveProductionCpuScene {
    pub fn new(output_size: Size) -> Self {
        Self {
            output_size,
            buffers: LiveCpuBufferRegistry::new(),
            last_report: None,
            last_output_damage_snapshot: None,
            max_nonzero_pixel_bytes: 0,
            max_layers_composed: 0,
            nonzero_frames: 0,
            exact_pixel_proofs_remaining: 3,
            exact_pixel_metric_frames: 0,
            damage_scoped_metric_frames: 0,
            retained_primary_frames: Vec::new(),
            secondary_output_frames: Vec::new(),
            indicator_strip_cache: Default::default(),
            text_cache: Default::default(),
            cursor_asset: sophia_engine::x11_core_left_ptr_cursor(1),
        }
    }

    pub fn set_cursor_asset(&mut self, asset: sophia_engine::CursorAsset) -> bool {
        if self.cursor_asset == asset {
            return false;
        }
        self.cursor_asset = asset;
        self.force_full_repaint();
        true
    }

    pub const fn cursor_asset(&self) -> &sophia_engine::CursorAsset {
        &self.cursor_asset
    }

    /// Rebind composition to a replacement primary output while retaining
    /// client-owned CPU buffers. Output-sized render products are invalidated;
    /// callers must compose before requesting replacement native frames.
    pub fn reconfigure_output_size(
        &mut self,
        output_size: Size,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if output_size.width <= 0 || output_size.height <= 0 {
            return Err("CPU scene replacement output size must be positive".into());
        }
        if self.output_size == output_size {
            return Ok(false);
        }
        self.output_size = output_size;
        self.last_report = None;
        self.last_output_damage_snapshot = None;
        self.retained_primary_frames.clear();
        self.secondary_output_frames.clear();
        Ok(true)
    }

    pub fn apply_updates(
        &mut self,
        updates: impl IntoIterator<Item = LiveCpuBufferUpdate>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for update in updates {
            self.buffers
                .apply(update)
                .map_err(|error| format!("renderer CPU buffer update failed: {error:?}"))?;
        }
        Ok(())
    }

    pub fn apply_production_updates(
        &mut self,
        updates: impl IntoIterator<Item = LiveCpuBufferUpdate>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for update in updates {
            match self.buffers.apply(update) {
                Ok(_) | Err(crate::LiveCpuBufferRegistryError::MissingPatchBase) => {}
                Err(error) => {
                    return Err(format!("renderer CPU buffer update failed: {error:?}").into());
                }
            }
        }
        Ok(())
    }

    pub fn reconcile_buffer_residency(&mut self, retained_handles: &[u64]) {
        self.buffers
            .retain_handles(|handle| retained_handles.binary_search(&handle).is_ok());
    }

    pub fn resident_buffer_count(&self) -> usize {
        self.buffers.len()
    }

    pub fn contains_buffer(&self, handle: u64) -> bool {
        self.buffers.contains(handle)
    }

    pub fn resident_buffer_bytes(&self) -> usize {
        self.buffers.total_bytes()
    }

    /// How often a client update had to copy because a presentation still held
    /// the bytes it was patching.
    pub const fn cpu_cow_splits(&self) -> u64 {
        self.buffers.cow_splits()
    }

    /// The most this scene ever held at once, which is what a bound is a claim
    /// about. The live figures say whether it drained; these say whether it grew.
    pub const fn peak_resident_buffers(&self) -> usize {
        self.buffers.peak_buffers()
    }

    pub const fn peak_resident_buffer_bytes(&self) -> usize {
        self.buffers.peak_bytes()
    }

    pub fn missing_committed_buffer_count(
        &self,
        committed_surfaces: &[CommittedSurfaceState],
    ) -> usize {
        committed_surfaces
            .iter()
            .flat_map(|surface| surface.content.variants())
            .filter_map(|variant| match variant.source {
                BufferSource::CpuBuffer { handle } => Some(handle),
                _ => None,
            })
            .filter(|handle| !self.buffers.contains(*handle))
            .count()
    }

    pub fn compose(
        &mut self,
        committed_surfaces: &[CommittedSurfaceState],
        raised_surface: Option<SurfaceId>,
        cursor_position: Option<Point>,
    ) -> Result<&LiveCpuCompositionReport, Box<dyn std::error::Error>> {
        self.compose_ordered(committed_surfaces, None, raised_surface, cursor_position)
    }

    /// Invalidates retained damage identity so the next composition repaints
    /// the complete output even when scene facts are otherwise unchanged.
    pub fn force_full_repaint(&mut self) {
        self.last_output_damage_snapshot = None;
    }

    pub fn compose_visible(
        &mut self,
        committed_surfaces: &[CommittedSurfaceState],
        presentation_order: &[SurfaceId],
        raised_surface: Option<SurfaceId>,
        cursor_position: Option<Point>,
    ) -> Result<&LiveCpuCompositionReport, Box<dyn std::error::Error>> {
        self.compose_ordered(
            committed_surfaces,
            Some(presentation_order),
            raised_surface,
            cursor_position,
        )
    }

    pub fn presentation_layers(
        &self,
        committed_surfaces: &[CommittedSurfaceState],
        presentation_order: &[SurfaceId],
    ) -> Vec<LiveCpuPresentationLayer> {
        presentation_order
            .iter()
            .filter_map(|surface| {
                let committed = committed_surfaces
                    .iter()
                    .find(|committed| committed.surface == *surface)?;
                let BufferSource::CpuBuffer { handle } = committed.buffer() else {
                    return None;
                };
                Some(LiveCpuPresentationLayer {
                    surface: *surface,
                    geometry: committed.geometry,
                    buffer: self.buffers.get(handle)?.clone(),
                })
            })
            .collect()
    }

    /// Resolves every resident CPU variant carried by committed surface
    /// content. Multiple entries may name one surface; a head-plan consumer
    /// selects the exact entry by its chosen protocol-neutral buffer handle.
    ///
    /// Each resolved layer shares the registry's allocation rather than copying
    /// it. This runs per variant per composed frame, so the copy it used to make
    /// was the largest single cost on the software presentation path.
    pub fn presentation_variant_layers(
        &self,
        committed_surfaces: &[CommittedSurfaceState],
        presentation_order: &[SurfaceId],
    ) -> Vec<LiveCpuPresentationLayer> {
        presentation_order
            .iter()
            .filter_map(|surface| {
                committed_surfaces
                    .iter()
                    .find(|committed| committed.surface == *surface)
            })
            .flat_map(|committed| {
                committed.content.variants().iter().filter_map(|variant| {
                    let BufferSource::CpuBuffer { handle } = variant.source else {
                        return None;
                    };
                    Some(LiveCpuPresentationLayer {
                        surface: committed.surface,
                        geometry: committed.geometry,
                        buffer: self.buffers.get(handle)?.clone(),
                    })
                })
            })
            .collect()
    }

    pub fn compose_display_list(
        &mut self,
        output: HeadlessOutput,
        committed_surfaces: &[CommittedSurfaceState],
        display_list: &CompositorDisplayList,
        cursor_position: Option<Point>,
    ) -> Result<&LiveCpuCompositionReport, Box<dyn std::error::Error>> {
        if output.size != self.output_size {
            return Err("CPU scene output descriptor has a mismatched size".into());
        }
        let (hotspot_x, hotspot_y) = self.cursor_asset.hotspot();
        let cursor_geometry = cursor_position.map(|position| Rect {
            x: (position.x.floor() as i32)
                .saturating_sub(i32::try_from(hotspot_x).unwrap_or(i32::MAX)),
            y: (position.y.floor() as i32)
                .saturating_sub(i32::try_from(hotspot_y).unwrap_or(i32::MAX)),
            width: i32::try_from(self.cursor_asset.width()).unwrap_or(i32::MAX),
            height: i32::try_from(self.cursor_asset.height()).unwrap_or(i32::MAX),
        });
        let current_output_damage_snapshot = output_frame_damage_snapshot(
            output,
            display_list.clone(),
            committed_surfaces,
            cursor_geometry,
        )?;
        let (reusable_bytes, repaint_damage) =
            self.take_primary_repaint_baseline(&current_output_damage_snapshot);
        let indicator_buffers = display_list
            .commands
            .iter()
            .filter_map(|command| match command {
                CompositorDisplayCommand::IndicatorStrip(strip) => Some(strip),
                CompositorDisplayCommand::Surface { .. }
                | CompositorDisplayCommand::Border(_)
                | CompositorDisplayCommand::Rect(_)
                | CompositorDisplayCommand::Text(_)
                | CompositorDisplayCommand::ContentImage(_) => None,
            })
            .map(|strip| {
                self.indicator_strip_cache.raster_for(
                    &sophia_engine::HeadCompositorIndicatorStrip {
                        node: strip.node,
                        generation: strip.generation,
                        strip: strip.strip.clone(),
                    },
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let text_buffers = display_list
            .commands
            .iter()
            .filter_map(|command| match command {
                CompositorDisplayCommand::Text(text) => Some(text),
                CompositorDisplayCommand::Surface { .. }
                | CompositorDisplayCommand::Border(_)
                | CompositorDisplayCommand::Rect(_)
                | CompositorDisplayCommand::IndicatorStrip(_)
                | CompositorDisplayCommand::ContentImage(_) => None,
            })
            .map(|text| {
                self.text_cache
                    .raster_for(&sophia_engine::HeadCompositorText {
                        node: text.node,
                        generation: text.generation,
                        geometry: text.geometry,
                        text: text.text.clone(),
                        font_size_millis: text.font_size_millis,
                        color: text.color,
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut indicator_index = 0usize;
        let mut text_index = 0usize;
        let mut elements = Vec::with_capacity(display_list.commands.len().saturating_mul(4));
        for command in &display_list.commands {
            match command {
                CompositorDisplayCommand::Surface { surface } => {
                    let committed = committed_surfaces
                        .iter()
                        .find(|committed| committed.surface == *surface);
                    let Some(committed) = committed else {
                        continue;
                    };
                    let BufferSource::CpuBuffer { handle } = committed.buffer() else {
                        continue;
                    };
                    let Some(buffer) = self.buffers.get(handle) else {
                        continue;
                    };
                    elements.push(LiveCpuCompositionElementRef::Layer(
                        LiveCpuCompositionLayerRef {
                            geometry: committed.geometry,
                            buffer: LiveCpuBufferSourceRef {
                                handle: buffer.handle,
                                size: buffer.size,
                                stride: buffer.stride,
                                format: buffer.format,
                                generation: buffer.generation,
                                bytes: &buffer.bytes,
                            },
                        },
                    ));
                }
                CompositorDisplayCommand::Border(border) => {
                    for band in sophia_engine::compositor_border_bands(*border) {
                        if !band.geometry.is_empty() {
                            elements.push(LiveCpuCompositionElementRef::Solid {
                                opacity: 255,
                                geometry: band.geometry,
                                color: band.color,
                            });
                        }
                    }
                }
                CompositorDisplayCommand::Rect(rect) => {
                    if !rect.geometry.is_empty() {
                        elements.push(LiveCpuCompositionElementRef::Solid {
                            opacity: rect.opacity,
                            geometry: rect.geometry,
                            color: rect.color,
                        });
                    }
                }
                CompositorDisplayCommand::Text(text) => {
                    let buffer = text_buffers
                        .get(text_index)
                        .ok_or("compositor text raster set is incomplete")?;
                    text_index = text_index.saturating_add(1);
                    elements.push(LiveCpuCompositionElementRef::Layer(
                        LiveCpuCompositionLayerRef {
                            geometry: text.geometry,
                            buffer: LiveCpuBufferSourceRef {
                                handle: buffer.handle,
                                size: buffer.size,
                                stride: buffer.stride,
                                format: buffer.format,
                                generation: buffer.generation,
                                bytes: buffer.bytes.as_slice(),
                            },
                        },
                    ));
                }
                CompositorDisplayCommand::IndicatorStrip(strip) => {
                    let buffer = indicator_buffers
                        .get(indicator_index)
                        .ok_or("indicator strip raster set is incomplete")?;
                    indicator_index = indicator_index.saturating_add(1);
                    elements.push(LiveCpuCompositionElementRef::Layer(
                        LiveCpuCompositionLayerRef {
                            geometry: strip.strip.geometry,
                            buffer: LiveCpuBufferSourceRef {
                                handle: buffer.handle,
                                size: buffer.size,
                                stride: buffer.stride,
                                format: buffer.format,
                                generation: buffer.generation,
                                bytes: buffer.bytes.as_slice(),
                            },
                        },
                    ));
                }
                CompositorDisplayCommand::ContentImage(content) => {
                    if content.output_size_px != output.size {
                        return Err("shell content targets a stale output size".into());
                    }
                    elements.push(LiveCpuCompositionElementRef::Layer(
                        LiveCpuCompositionLayerRef {
                            geometry: content.geometry_px,
                            buffer: LiveCpuBufferSourceRef {
                                handle: content.resource.description().resource.id,
                                size: content.size_px,
                                stride: content.stride,
                                format: content.format,
                                generation: content.generation,
                                bytes: content.resource.bytes(),
                            },
                        },
                    ));
                }
            }
        }
        let metrics_mode = if self.exact_pixel_proofs_remaining == 0 {
            self.damage_scoped_metric_frames = self.damage_scoped_metric_frames.saturating_add(1);
            LiveCpuFrameMetricsMode::DamageScopedEvidence
        } else {
            self.exact_pixel_proofs_remaining = self.exact_pixel_proofs_remaining.saturating_sub(1);
            self.exact_pixel_metric_frames = self.exact_pixel_metric_frames.saturating_add(1);
            LiveCpuFrameMetricsMode::ExactPixels
        };
        self.last_report = Some(
            compose_live_cpu_display_list_frame_with_metrics_reusing_damage_and_cursor_asset(
                self.output_size,
                &elements,
                cursor_position,
                metrics_mode,
                reusable_bytes,
                repaint_damage.as_ref(),
                &self.cursor_asset,
            )
            .map_err(|error| {
                format!("persistent CPU display-list composition failed: {error:?}")
            })?,
        );
        self.last_output_damage_snapshot = Some(current_output_damage_snapshot);
        self.record_last_report();
        Ok(self.last_report.as_ref().expect("assigned above"))
    }

    fn take_primary_repaint_baseline(
        &mut self,
        current: &OutputFrameDamageSnapshot,
    ) -> (Option<std::sync::Arc<Vec<u8>>>, Option<Region>) {
        let latest = self.last_report.take().and_then(|report| {
            self.last_output_damage_snapshot
                .take()
                .map(|output_damage_snapshot| RetainedPrimaryCpuFrame {
                    bytes: report.frame.bytes,
                    output_damage_snapshot,
                })
        });
        let Some(latest) = latest else {
            self.retained_primary_frames.clear();
            return (None, None);
        };

        if let Some(damage) = retained_primary_repaint_damage(
            self.output_size,
            &latest.output_damage_snapshot,
            current,
        ) && (damage.rects.is_empty() || std::sync::Arc::strong_count(&latest.bytes) == 1)
        {
            return (Some(latest.bytes), Some(damage));
        }

        self.retained_primary_frames.push(RetainedPrimaryCpuFrame {
            bytes: latest.bytes,
            output_damage_snapshot: latest.output_damage_snapshot,
        });
        let reusable = self
            .retained_primary_frames
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, retained)| {
                if std::sync::Arc::strong_count(&retained.bytes) != 1 {
                    return None;
                }
                retained_primary_repaint_damage(
                    self.output_size,
                    &retained.output_damage_snapshot,
                    current,
                )
                .map(|damage| (index, damage))
            });
        if let Some((index, damage)) = reusable {
            let retained = self.retained_primary_frames.remove(index);
            return (Some(retained.bytes), Some(damage));
        }
        while self.retained_primary_frames.len() > RETAINED_PRIMARY_CPU_FRAME_CAPACITY {
            self.retained_primary_frames.remove(0);
        }
        (None, None)
    }

    fn compose_ordered(
        &mut self,
        committed_surfaces: &[CommittedSurfaceState],
        presentation_order: Option<&[SurfaceId]>,
        raised_surface: Option<SurfaceId>,
        cursor_position: Option<Point>,
    ) -> Result<&LiveCpuCompositionReport, Box<dyn std::error::Error>> {
        let mut surface_order = match presentation_order {
            Some(order) => order
                .iter()
                .filter_map(|surface| {
                    committed_surfaces
                        .iter()
                        .find(|committed| committed.surface == *surface)
                })
                .collect::<Vec<_>>(),
            None => committed_surfaces.iter().collect::<Vec<_>>(),
        };
        surface_order.retain(|surface| Some(surface.surface) != raised_surface);
        if let Some(raised) = raised_surface
            && let Some(surface) = committed_surfaces
                .iter()
                .find(|surface| surface.surface == raised)
            && presentation_order.is_none_or(|order| order.contains(&raised))
        {
            surface_order.push(surface);
        }
        let layers = surface_order
            .iter()
            .filter_map(|surface| {
                let BufferSource::CpuBuffer { handle } = surface.buffer() else {
                    return None;
                };
                let buffer = self.buffers.get(handle)?;
                Some(LiveCpuCompositionLayerRef {
                    geometry: surface.geometry,
                    buffer: LiveCpuBufferSourceRef {
                        handle: buffer.handle,
                        size: buffer.size,
                        stride: buffer.stride,
                        format: buffer.format,
                        generation: buffer.generation,
                        bytes: &buffer.bytes,
                    },
                })
            })
            .collect::<Vec<_>>();
        self.last_report = Some(
            compose_live_cpu_frame_ref_with_cursor_asset(
                self.output_size,
                &layers,
                cursor_position,
                &self.cursor_asset,
            )
            .map_err(|error| format!("persistent CPU composition failed: {error:?}"))?,
        );
        self.last_output_damage_snapshot = None;
        self.retained_primary_frames.clear();
        self.record_last_report();
        Ok(self.last_report.as_ref().expect("assigned above"))
    }

    fn record_last_report(&mut self) {
        let report = self.last_report.as_ref().expect("assigned above");
        let nonzero_pixel_bytes = report.nonzero_pixel_bytes;
        // THE MOST THIS SCENE EVER COMPOSED AT ONCE. `last_report` is one
        // composition and is replaced by the next, so at a quiet end it
        // describes an empty frame -- which is the correct picture of that
        // moment and no evidence at all about the session. Read here because
        // every report the scene produces passes through this function.
        let layers_composed = report.layers_composed;
        self.max_nonzero_pixel_bytes = self.max_nonzero_pixel_bytes.max(nonzero_pixel_bytes);
        self.max_layers_composed = self.max_layers_composed.max(layers_composed);
        self.nonzero_frames = self
            .nonzero_frames
            .saturating_add(usize::from(nonzero_pixel_bytes > 0));
    }

    pub fn last_report(&self) -> Option<&LiveCpuCompositionReport> {
        self.last_report.as_ref()
    }

    pub fn max_nonzero_pixel_bytes(&self) -> usize {
        self.max_nonzero_pixel_bytes
    }

    /// The most layers any one composition put on screen. Says composition
    /// happened; says nothing about how many surfaces were up, because a
    /// direct-scanout copy reports one layer whatever the scene holds and a
    /// damage-scoped frame counts only what that frame touched. The surface
    /// count is `runtime_max_surfaces`.
    pub fn max_layers_composed(&self) -> usize {
        self.max_layers_composed
    }

    pub fn nonzero_frames(&self) -> usize {
        self.nonzero_frames
    }

    pub fn exact_pixel_metric_frames(&self) -> usize {
        self.exact_pixel_metric_frames
    }

    pub fn damage_scoped_metric_frames(&self) -> usize {
        self.damage_scoped_metric_frames
    }

    pub fn buffer_checksum(&self) -> u64 {
        self.buffers.checksum()
    }

    pub fn surface_buffer_generation(
        &self,
        committed_surfaces: &[CommittedSurfaceState],
        surface: SurfaceId,
    ) -> Option<u64> {
        let committed = committed_surfaces
            .iter()
            .find(|committed| committed.surface == surface)?;
        let BufferSource::CpuBuffer { handle } = committed.buffer() else {
            return None;
        };
        Some(self.buffers.get(handle)?.generation)
    }

    /// Returns true only when the focused surface contains at least two
    /// visible XRGB pixel values. A newly mapped xterm initially publishes a
    /// uniform background buffer; its prompt or cursor introduces visual
    /// detail once the terminal side is ready for input. Inspecting the
    /// focused surface avoids treating another client's draw as readiness.
    pub fn surface_has_visual_detail(
        &self,
        committed_surfaces: &[CommittedSurfaceState],
        surface: SurfaceId,
    ) -> bool {
        let Some(committed) = committed_surfaces
            .iter()
            .find(|committed| committed.surface == surface)
        else {
            return false;
        };
        let BufferSource::CpuBuffer { handle } = committed.buffer() else {
            return false;
        };
        let Some(buffer) = self.buffers.get(handle) else {
            return false;
        };
        let Ok(width) = usize::try_from(buffer.size.width) else {
            return false;
        };
        let Ok(height) = usize::try_from(buffer.size.height) else {
            return false;
        };
        let Ok(stride) = usize::try_from(buffer.stride) else {
            return false;
        };
        let Some(row_bytes) = width.checked_mul(4) else {
            return false;
        };
        if width == 0 || height == 0 || stride < row_bytes || buffer.bytes.len() < 4 {
            return false;
        }
        let first = &buffer.bytes[..4];
        (0..height).any(|row| {
            let Some(start) = row.checked_mul(stride) else {
                return false;
            };
            let Some(end) = start.checked_add(row_bytes) else {
                return false;
            };
            buffer
                .bytes
                .get(start..end)
                .is_some_and(|bytes| bytes.chunks_exact(4).any(|pixel| pixel != first))
        })
    }

    pub fn frames_for_outputs(
        &mut self,
        outputs: &[HeadlessOutput],
    ) -> Result<Vec<LiveProductionComposedFrame>, Box<dyn std::error::Error>> {
        let primary = self
            .last_report
            .as_ref()
            .ok_or("persistent CPU scene has no composed primary frame")?;
        let primary_frame = primary.frame.clone();
        let primary_checksum = primary.checksum;
        let primary_nonzero_pixel_bytes = primary.nonzero_pixel_bytes;
        let primary_damage_snapshot = self.last_output_damage_snapshot.clone();
        self.secondary_output_frames.retain(|(index, output, _)| {
            *index > 0 && outputs.get(*index).is_some_and(|current| current == output)
        });
        let mut frames = Vec::with_capacity(outputs.len());
        for (index, output) in outputs.iter().enumerate() {
            if index == 0 && output.size == primary_frame.size {
                frames.push(LiveProductionComposedFrame {
                    frame: primary_frame.clone(),
                    checksum: primary_checksum,
                    nonzero_pixel_bytes: primary_nonzero_pixel_bytes,
                    output_damage_snapshot: primary_damage_snapshot
                        .as_ref()
                        .filter(|snapshot| snapshot.output == *output)
                        .cloned(),
                });
                continue;
            }
            if index > 0
                && let Some((_, _, frame)) =
                    self.secondary_output_frames
                        .iter()
                        .find(|(cached_index, cached_output, _)| {
                            *cached_index == index && cached_output == output
                        })
            {
                frames.push(frame.clone());
                continue;
            }
            let marker_size = Size {
                width: output.size.width.clamp(1, 64),
                height: output.size.height.clamp(1, 64),
            };
            let marker_width = usize::try_from(marker_size.width)?;
            let marker_height = usize::try_from(marker_size.height)?;
            let marker_stride = marker_width
                .checked_mul(4)
                .ok_or("marker stride overflow")?;
            let marker_byte = u8::try_from((index + 1).min(255)).unwrap_or(255);
            let marker = LiveCpuCompositionLayer {
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: marker_size.width,
                    height: marker_size.height,
                },
                buffer: LiveCpuBufferSource {
                    handle: 0x5350_4800u64.saturating_add(index as u64),
                    size: marker_size,
                    stride: u32::try_from(marker_stride)?,
                    format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                    generation: 1,
                    bytes: std::sync::Arc::new(vec![
                        marker_byte;
                        marker_stride.saturating_mul(marker_height)
                    ]),
                },
            };
            let report = compose_live_cpu_frame(output.size, &[marker])
                .map_err(|error| format!("secondary output composition failed: {error:?}"))?;
            let frame = LiveProductionComposedFrame {
                frame: report.frame,
                checksum: report.checksum,
                nonzero_pixel_bytes: report.nonzero_pixel_bytes,
                output_damage_snapshot: Some(output_frame_damage_snapshot(
                    *output,
                    CompositorDisplayList::empty(output.id),
                    &[],
                    None,
                )?),
            };
            if index > 0 {
                self.secondary_output_frames
                    .push((index, *output, frame.clone()));
            }
            frames.push(frame);
        }
        Ok(frames)
    }
}

fn retained_primary_repaint_damage(
    output_size: Size,
    retained: &OutputFrameDamageSnapshot,
    current: &OutputFrameDamageSnapshot,
) -> Option<Region> {
    let damage = output_frame_damage(Some(retained), current).ok()?;
    match plan_output_repaint(output_size, &damage, OutputRepaintPolicy::default()).ok()? {
        OutputRepaintPlan::Skip => Some(Region::empty()),
        OutputRepaintPlan::Partial { damage, .. } => Some(damage),
        OutputRepaintPlan::Full { .. } => None,
    }
}
