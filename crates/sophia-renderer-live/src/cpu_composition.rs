mod pixel_storage;
mod solid;
pub use pixel_storage::LiveCpuPixelStorage;
pub use solid::solid_color_buffer;
use solid::{compose_solid_rect, compose_solid_rect_clipped};
use std::sync::{Arc, OnceLock};

use sophia_engine::{CompositorRgb8, CursorAsset, x11_core_left_ptr_cursor};
use sophia_protocol::{Point, Rect, Region, Size};

use crate::{LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888, LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888};

/// A client's CPU pixels as the registry holds them.
///
/// The bytes are shared with whoever handed them over and with every
/// presentation that has been given this buffer, and are copied only when one
/// of those still reads them. `Arc::make_mut` in the registry's patch path is
/// what keeps a handle's history immutable: a lease reads what it was handed
/// until it retires, and the update that arrives meanwhile lands on a copy.
///
/// Equality compares contents rather than allocations. Two allocations may hold
/// identical pixels, and a source is compared for what it says.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveCpuBufferSource {
    pub handle: u64,
    pub size: Size,
    pub stride: u32,
    pub format: u32,
    pub generation: u64,
    pub bytes: Arc<Vec<u8>>,
}

/// Immutable CPU pixels retained by an owned renderer-composition frame.
///
/// Cloning this record preserves one pixel allocation so a frame can be
/// projected onto multiple scanout heads without cloning its bytes per head.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveSharedCpuBufferSource {
    pub handle: u64,
    pub size: Size,
    pub stride: u32,
    pub format: u32,
    pub generation: u64,
    pub bytes: LiveCpuPixelStorage,
}

impl From<LiveCpuBufferSource> for LiveSharedCpuBufferSource {
    /// A refcount bump, not a copy.
    ///
    /// This conversion runs per density variant per head per composed frame.
    /// It used to move a `Vec` into a fresh `Arc`, which meant the caller had
    /// already cloned the registry's bytes to have a `Vec` to give away.
    fn from(buffer: LiveCpuBufferSource) -> Self {
        Self {
            handle: buffer.handle,
            size: buffer.size,
            stride: buffer.stride,
            format: buffer.format,
            generation: buffer.generation,
            bytes: buffer.bytes.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveCpuCompositionLayer {
    pub geometry: Rect,
    pub buffer: LiveCpuBufferSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveCpuBufferSourceRef<'a> {
    pub handle: u64,
    pub size: Size,
    pub stride: u32,
    pub format: u32,
    pub generation: u64,
    pub bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveCpuCompositionLayerRef<'a> {
    pub geometry: Rect,
    pub buffer: LiveCpuBufferSourceRef<'a>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveCpuCompositionElementRef<'a> {
    Layer(LiveCpuCompositionLayerRef<'a>),
    /// Samples the whole source into geometry, then clips the destination.
    ClippedLayer {
        layer: LiveCpuCompositionLayerRef<'a>,
        clip: Rect,
    },
    Solid {
        opacity: u8,
        geometry: Rect,
        color: CompositorRgb8,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveCpuComposedFrame {
    pub size: Size,
    pub stride: u32,
    pub format: u32,
    pub bytes: Arc<Vec<u8>>,
}

/// Projects one flat CPU scene into a physical head-sized buffer.
///
/// This is the fail-safe mirror bootstrap: initial KMS ownership can be
/// established through the direct CPU path before renderer workers start, so no
/// inline EGL target survives the handoff to a worker context.
pub fn project_live_cpu_composed_frame(
    frame: &LiveCpuComposedFrame,
    destination: Size,
    target: Rect,
) -> std::io::Result<LiveCpuComposedFrame> {
    if frame.size.width <= 0
        || frame.size.height <= 0
        || destination.width <= 0
        || destination.height <= 0
        || target.width <= 0
        || target.height <= 0
        || !matches!(
            frame.format,
            LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888 | LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888
        )
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "CPU mirror projection has invalid geometry or format",
        ));
    }
    let source_stride = usize::try_from(frame.stride)
        .map_err(|_| std::io::Error::other("CPU mirror source stride exceeds address space"))?;
    let source_height = usize::try_from(frame.size.height)
        .map_err(|_| std::io::Error::other("CPU mirror source height exceeds address space"))?;
    let source_width = usize::try_from(frame.size.width)
        .map_err(|_| std::io::Error::other("CPU mirror source width exceeds address space"))?;
    if source_stride < source_width.saturating_mul(4) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "CPU mirror source stride is too small",
        ));
    }
    let source_len = source_stride
        .checked_mul(source_height)
        .ok_or_else(|| std::io::Error::other("CPU mirror source size overflow"))?;
    if frame.bytes.len() < source_len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "CPU mirror source pixels are truncated",
        ));
    }
    if destination == frame.size
        && target
            == (Rect {
                x: 0,
                y: 0,
                width: destination.width,
                height: destination.height,
            })
    {
        return Ok(frame.clone());
    }
    let width = usize::try_from(destination.width)
        .map_err(|_| std::io::Error::other("CPU mirror width exceeds address space"))?;
    let height = usize::try_from(destination.height)
        .map_err(|_| std::io::Error::other("CPU mirror height exceeds address space"))?;
    let stride = width
        .checked_mul(4)
        .ok_or_else(|| std::io::Error::other("CPU mirror stride overflow"))?;
    let len = stride
        .checked_mul(height)
        .ok_or_else(|| std::io::Error::other("CPU mirror allocation overflow"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len)
        .map_err(|_| std::io::Error::other("CPU mirror allocation failed"))?;
    bytes.resize(len, 0);

    let left = target.x.max(0);
    let top = target.y.max(0);
    let right = target.x.saturating_add(target.width).min(destination.width);
    let bottom = target
        .y
        .saturating_add(target.height)
        .min(destination.height);
    for y in top..bottom {
        let source_y = i64::from(y.saturating_sub(target.y)) * i64::from(frame.size.height)
            / i64::from(target.height);
        let source_y = usize::try_from(source_y.clamp(0, i64::from(frame.size.height - 1)))
            .map_err(|_| std::io::Error::other("CPU mirror source row is invalid"))?;
        for x in left..right {
            let source_x = i64::from(x.saturating_sub(target.x)) * i64::from(frame.size.width)
                / i64::from(target.width);
            let source_x = usize::try_from(source_x.clamp(0, i64::from(frame.size.width - 1)))
                .map_err(|_| std::io::Error::other("CPU mirror source column is invalid"))?;
            let source_offset = source_y * source_stride + source_x * 4;
            let destination_y = usize::try_from(y)
                .map_err(|_| std::io::Error::other("CPU mirror destination row is invalid"))?;
            let destination_x = usize::try_from(x)
                .map_err(|_| std::io::Error::other("CPU mirror destination column is invalid"))?;
            let destination_offset = destination_y * stride + destination_x * 4;
            bytes[destination_offset..destination_offset + 4]
                .copy_from_slice(&frame.bytes[source_offset..source_offset + 4]);
        }
    }
    Ok(LiveCpuComposedFrame {
        size: destination,
        stride: u32::try_from(stride)
            .map_err(|_| std::io::Error::other("CPU mirror stride exceeds protocol range"))?,
        format: frame.format,
        bytes: Arc::new(bytes),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveCpuCompositionReport {
    pub frame: LiveCpuComposedFrame,
    pub layers_input: usize,
    pub layers_composed: usize,
    pub nonzero_pixel_bytes: usize,
    /// Stable scheduling identity derived from immutable resource generations,
    /// geometry, compositor primitives, and cursor state.
    pub checksum: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveCpuCompositionError {
    InvalidOutputSize,
    OutputTooLarge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveCpuFrameMetricsMode {
    /// Count exact output bytes while retaining the same scheduling identity
    /// used by the damage-scoped mode.
    ExactPixels,
    /// Use bounded source evidence for the nonzero-pixel proof.
    DamageScopedEvidence,
}

pub fn compose_live_cpu_frame(
    output_size: Size,
    layers: &[LiveCpuCompositionLayer],
) -> Result<LiveCpuCompositionReport, LiveCpuCompositionError> {
    let borrowed = layers
        .iter()
        .map(|layer| LiveCpuCompositionLayerRef {
            geometry: layer.geometry,
            buffer: LiveCpuBufferSourceRef {
                handle: layer.buffer.handle,
                size: layer.buffer.size,
                stride: layer.buffer.stride,
                format: layer.buffer.format,
                generation: layer.buffer.generation,
                bytes: &layer.buffer.bytes,
            },
        })
        .collect::<Vec<_>>();
    compose_live_cpu_frame_ref(output_size, &borrowed)
}

pub fn compose_live_cpu_frame_ref(
    output_size: Size,
    layers: &[LiveCpuCompositionLayerRef<'_>],
) -> Result<LiveCpuCompositionReport, LiveCpuCompositionError> {
    compose_live_cpu_frame_ref_with_cursor(output_size, layers, None)
}

/// Composes CPU-backed client layers and, when present, a compositor-owned
/// software cursor. The cursor is part of the scanout frame, so moving it
/// produces a frame even when the client itself has not committed new pixels.
pub fn compose_live_cpu_frame_ref_with_cursor(
    output_size: Size,
    layers: &[LiveCpuCompositionLayerRef<'_>],
    cursor_position: Option<Point>,
) -> Result<LiveCpuCompositionReport, LiveCpuCompositionError> {
    compose_live_cpu_frame_ref_with_cursor_asset(
        output_size,
        layers,
        cursor_position,
        default_cursor_asset(),
    )
}

pub fn compose_live_cpu_frame_ref_with_cursor_asset(
    output_size: Size,
    layers: &[LiveCpuCompositionLayerRef<'_>],
    cursor_position: Option<Point>,
    cursor_asset: &CursorAsset,
) -> Result<LiveCpuCompositionReport, LiveCpuCompositionError> {
    let elements = layers
        .iter()
        .copied()
        .map(LiveCpuCompositionElementRef::Layer)
        .collect::<Vec<_>>();
    compose_live_cpu_display_list_frame_with_cursor_asset(
        output_size,
        &elements,
        cursor_position,
        cursor_asset,
    )
}

/// Lowers one renderer-neutral ordered display list into the CPU reference
/// frame. Solid rectangles remain interleaved with client surfaces.
pub fn compose_live_cpu_display_list_frame(
    output_size: Size,
    elements: &[LiveCpuCompositionElementRef<'_>],
    cursor_position: Option<Point>,
) -> Result<LiveCpuCompositionReport, LiveCpuCompositionError> {
    compose_live_cpu_display_list_frame_with_cursor_asset(
        output_size,
        elements,
        cursor_position,
        default_cursor_asset(),
    )
}

pub fn compose_live_cpu_display_list_frame_with_cursor_asset(
    output_size: Size,
    elements: &[LiveCpuCompositionElementRef<'_>],
    cursor_position: Option<Point>,
    cursor_asset: &CursorAsset,
) -> Result<LiveCpuCompositionReport, LiveCpuCompositionError> {
    compose_live_cpu_display_list_frame_with_metrics_reusing_damage_and_cursor_asset(
        output_size,
        elements,
        cursor_position,
        LiveCpuFrameMetricsMode::ExactPixels,
        None,
        None,
        cursor_asset,
    )
}

fn default_cursor_asset() -> &'static CursorAsset {
    static ASSET: OnceLock<CursorAsset> = OnceLock::new();
    ASSET.get_or_init(|| x11_core_left_ptr_cursor(1))
}

fn compose_live_cpu_display_list_frame_with_default_cursor(
    output_size: Size,
    elements: &[LiveCpuCompositionElementRef<'_>],
    cursor_position: Option<Point>,
    metrics_mode: LiveCpuFrameMetricsMode,
) -> Result<LiveCpuCompositionReport, LiveCpuCompositionError> {
    compose_live_cpu_display_list_frame_with_metrics_reusing_damage_and_cursor_asset(
        output_size,
        elements,
        cursor_position,
        metrics_mode,
        None,
        None,
        default_cursor_asset(),
    )
}

pub fn compose_live_cpu_display_list_frame_with_metrics(
    output_size: Size,
    elements: &[LiveCpuCompositionElementRef<'_>],
    cursor_position: Option<Point>,
    metrics_mode: LiveCpuFrameMetricsMode,
) -> Result<LiveCpuCompositionReport, LiveCpuCompositionError> {
    compose_live_cpu_display_list_frame_with_default_cursor(
        output_size,
        elements,
        cursor_position,
        metrics_mode,
    )
}

pub fn compose_live_cpu_display_list_frame_with_metrics_reusing(
    output_size: Size,
    elements: &[LiveCpuCompositionElementRef<'_>],
    cursor_position: Option<Point>,
    metrics_mode: LiveCpuFrameMetricsMode,
    reusable_bytes: Option<Arc<Vec<u8>>>,
) -> Result<LiveCpuCompositionReport, LiveCpuCompositionError> {
    compose_live_cpu_display_list_frame_with_metrics_reusing_damage(
        output_size,
        elements,
        cursor_position,
        metrics_mode,
        reusable_bytes,
        None,
    )
}

/// Reuses a retained frame and rebuilds only `repaint_damage` when the
/// retained storage is compatible. Missing or incompatible storage falls
/// back to a full composition.
pub fn compose_live_cpu_display_list_frame_with_metrics_reusing_damage(
    output_size: Size,
    elements: &[LiveCpuCompositionElementRef<'_>],
    cursor_position: Option<Point>,
    metrics_mode: LiveCpuFrameMetricsMode,
    reusable_bytes: Option<Arc<Vec<u8>>>,
    repaint_damage: Option<&Region>,
) -> Result<LiveCpuCompositionReport, LiveCpuCompositionError> {
    compose_live_cpu_display_list_frame_with_metrics_reusing_damage_and_cursor_asset(
        output_size,
        elements,
        cursor_position,
        metrics_mode,
        reusable_bytes,
        repaint_damage,
        default_cursor_asset(),
    )
}

pub fn compose_live_cpu_display_list_frame_with_metrics_reusing_damage_and_cursor_asset(
    output_size: Size,
    elements: &[LiveCpuCompositionElementRef<'_>],
    cursor_position: Option<Point>,
    metrics_mode: LiveCpuFrameMetricsMode,
    reusable_bytes: Option<Arc<Vec<u8>>>,
    repaint_damage: Option<&Region>,
    cursor_asset: &CursorAsset,
) -> Result<LiveCpuCompositionReport, LiveCpuCompositionError> {
    let width = usize::try_from(output_size.width)
        .ok()
        .filter(|width| *width > 0)
        .ok_or(LiveCpuCompositionError::InvalidOutputSize)?;
    let height = usize::try_from(output_size.height)
        .ok()
        .filter(|height| *height > 0)
        .ok_or(LiveCpuCompositionError::InvalidOutputSize)?;
    let stride = width
        .checked_mul(4)
        .ok_or(LiveCpuCompositionError::OutputTooLarge)?;
    let byte_len = stride
        .checked_mul(height)
        .filter(|len| *len <= 64 * 1024 * 1024)
        .ok_or(LiveCpuCompositionError::OutputTooLarge)?;
    let frame_stride =
        u32::try_from(stride).map_err(|_| LiveCpuCompositionError::OutputTooLarge)?;
    let direct = elements.first().and_then(|element| match element {
        LiveCpuCompositionElementRef::Layer(layer)
            if elements.len() == 1
                && layer.geometry
                    == (Rect {
                        x: 0,
                        y: 0,
                        width: output_size.width,
                        height: output_size.height,
                    })
                && layer.buffer.size == output_size
                && layer.buffer.stride == frame_stride
                && layer.buffer.format == LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888
                && layer.buffer.bytes.len() == byte_len =>
        {
            Some(layer)
        }
        _ => None,
    });
    let partial_repaint = repaint_damage.filter(|_| {
        reusable_bytes
            .as_ref()
            .is_some_and(|bytes| bytes.len() == byte_len)
    });
    let repaint_requires_write = partial_repaint.is_some_and(|damage| !damage.rects.is_empty());
    let frame_bytes = reusable_frame_bytes(
        reusable_bytes,
        byte_len,
        partial_repaint.is_some(),
        repaint_requires_write,
    );
    let mut frame = LiveCpuComposedFrame {
        size: output_size,
        stride: frame_stride,
        format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        bytes: frame_bytes,
    };
    let mut layers_composed = 0usize;
    if let Some(damage) = partial_repaint {
        let mut composed_elements = vec![false; elements.len()];
        for clip in damage.rects.iter().copied() {
            if !clear_frame_rect(&mut frame, clip) {
                continue;
            }
            for (index, element) in elements.iter().enumerate() {
                let composed = match element {
                    LiveCpuCompositionElementRef::ClippedLayer {
                        layer,
                        clip: bounds,
                    } => clip_rect(clip, *bounds)
                        .is_some_and(|clip| compose_scaled_layer_clipped(&mut frame, layer, clip)),
                    LiveCpuCompositionElementRef::Layer(layer) => {
                        compose_layer_clipped(&mut frame, layer, clip)
                    }
                    LiveCpuCompositionElementRef::Solid {
                        opacity,
                        geometry,
                        color,
                    } => compose_solid_rect_clipped(&mut frame, *geometry, *color, *opacity, clip),
                };
                composed_elements[index] |= composed;
            }
            if let Some(position) = cursor_position {
                compose_software_cursor_clipped(&mut frame, position, clip, cursor_asset);
            }
        }
        layers_composed = composed_elements
            .into_iter()
            .filter(|composed| *composed)
            .count();
    } else {
        let writable_bytes = Arc::get_mut(&mut frame.bytes)
            .expect("reusable frame bytes must have unique ownership");
        if let Some(layer) = direct {
            writable_bytes.copy_from_slice(layer.buffer.bytes);
            layers_composed = 1;
        } else {
            writable_bytes.fill(0);
            for element in elements {
                let composed = match element {
                    LiveCpuCompositionElementRef::Layer(layer) => compose_layer(&mut frame, layer),
                    LiveCpuCompositionElementRef::ClippedLayer { layer, clip } => {
                        compose_scaled_layer_clipped(&mut frame, layer, *clip)
                    }
                    LiveCpuCompositionElementRef::Solid {
                        opacity,
                        geometry,
                        color,
                    } => compose_solid_rect(&mut frame, *geometry, *color, *opacity),
                };
                if composed {
                    layers_composed = layers_composed.saturating_add(1);
                }
            }
        }
        if let Some(position) = cursor_position {
            compose_software_cursor(&mut frame, position, cursor_asset);
        }
    }
    let (nonzero_evidence, checksum) =
        composition_evidence_metrics(output_size, elements, cursor_position, cursor_asset);
    let nonzero_pixel_bytes = match metrics_mode {
        LiveCpuFrameMetricsMode::ExactPixels => cpu_frame_metrics(&frame.bytes).0,
        LiveCpuFrameMetricsMode::DamageScopedEvidence => nonzero_evidence,
    };
    Ok(LiveCpuCompositionReport {
        frame,
        layers_input: elements.len(),
        layers_composed,
        nonzero_pixel_bytes,
        checksum,
    })
}

fn reusable_frame_bytes(
    reusable_bytes: Option<Arc<Vec<u8>>>,
    byte_len: usize,
    preserve_pixels: bool,
    requires_unique_ownership: bool,
) -> Arc<Vec<u8>> {
    match reusable_bytes {
        Some(bytes)
            if bytes.len() == byte_len
                && (!requires_unique_ownership || Arc::strong_count(&bytes) == 1) =>
        {
            bytes
        }
        Some(bytes) if bytes.len() == byte_len && preserve_pixels => {
            Arc::new(bytes.as_ref().clone())
        }
        _ => Arc::new(vec![0; byte_len]),
    }
}

fn composition_evidence_metrics(
    output_size: Size,
    elements: &[LiveCpuCompositionElementRef<'_>],
    cursor_position: Option<Point>,
    cursor_asset: &CursorAsset,
) -> (usize, u64) {
    let mut checksum = 0xcbf2_9ce4_8422_2325u64;
    for value in [
        u64::try_from(output_size.width).unwrap_or(u64::MAX),
        u64::try_from(output_size.height).unwrap_or(u64::MAX),
        u64::try_from(elements.len()).unwrap_or(u64::MAX),
    ] {
        checksum = evidence_hash(checksum, value);
    }
    let mut nonzero_evidence = 0usize;
    for element in elements {
        if let LiveCpuCompositionElementRef::ClippedLayer { clip, .. } = element {
            for value in [clip.x, clip.y, clip.width, clip.height] {
                checksum = evidence_hash(checksum, value as u32 as u64);
            }
        }
        match element {
            LiveCpuCompositionElementRef::Layer(layer)
            | LiveCpuCompositionElementRef::ClippedLayer { layer, .. } => {
                for value in [
                    layer.buffer.handle,
                    layer.buffer.generation,
                    u64::try_from(layer.geometry.x).unwrap_or(u64::MAX),
                    u64::try_from(layer.geometry.y).unwrap_or(u64::MAX),
                    u64::try_from(layer.geometry.width).unwrap_or(u64::MAX),
                    u64::try_from(layer.geometry.height).unwrap_or(u64::MAX),
                ] {
                    checksum = evidence_hash(checksum, value);
                }
                nonzero_evidence = nonzero_evidence.saturating_add(usize::from(
                    layer.buffer.bytes.iter().any(|byte| *byte != 0),
                ));
            }
            LiveCpuCompositionElementRef::Solid {
                opacity,
                geometry,
                color,
            } => {
                for value in [
                    u64::try_from(geometry.x).unwrap_or(u64::MAX),
                    u64::try_from(geometry.y).unwrap_or(u64::MAX),
                    u64::try_from(geometry.width).unwrap_or(u64::MAX),
                    u64::try_from(geometry.height).unwrap_or(u64::MAX),
                    u64::from(color.red),
                    u64::from(color.green),
                    u64::from(color.blue),
                    u64::from(*opacity),
                ] {
                    checksum = evidence_hash(checksum, value);
                }
                nonzero_evidence =
                    nonzero_evidence.saturating_add(usize::from(!geometry.is_empty()));
            }
        }
    }
    if let Some(position) = cursor_position {
        checksum = evidence_hash(checksum, position.x.to_bits());
        checksum = evidence_hash(checksum, position.y.to_bits());
        nonzero_evidence = nonzero_evidence.saturating_add(1);
        for bytes in cursor_asset.digest().bytes().chunks_exact(8) {
            checksum = evidence_hash(
                checksum,
                u64::from_le_bytes(bytes.try_into().expect("eight-byte digest chunk")),
            );
        }
    }
    (nonzero_evidence, checksum)
}

const fn evidence_hash(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(0x100_0000_01b3)
}

fn compose_software_cursor(frame: &mut LiveCpuComposedFrame, position: Point, asset: &CursorAsset) {
    compose_software_cursor_clipped(frame, position, output_rect(frame.size), asset);
}

fn compose_software_cursor_clipped(
    frame: &mut LiveCpuComposedFrame,
    position: Point,
    clip: Rect,
    asset: &CursorAsset,
) {
    if !position.x.is_finite()
        || !position.y.is_finite()
        || position.x < f64::from(i32::MIN)
        || position.x > f64::from(i32::MAX)
        || position.y < f64::from(i32::MIN)
        || position.y > f64::from(i32::MAX)
    {
        return;
    }
    let origin_x = (position.x.floor().min(f64::from(i32::MAX)) as i32)
        .saturating_sub(i32::try_from(asset.hotspot().0).unwrap_or(i32::MAX));
    let origin_y = (position.y.floor().min(f64::from(i32::MAX)) as i32)
        .saturating_sub(i32::try_from(asset.hotspot().1).unwrap_or(i32::MAX));

    let width = usize::try_from(asset.width()).unwrap_or(0);
    for (row, pixels) in asset
        .pixels()
        .chunks_exact(width.saturating_mul(4))
        .enumerate()
    {
        for (column, color) in pixels.chunks_exact(4).enumerate() {
            if color[3] == 0 {
                continue;
            }
            let x = origin_x.saturating_add(i32::try_from(column).unwrap_or(i32::MAX));
            let y = origin_y.saturating_add(i32::try_from(row).unwrap_or(i32::MAX));
            if rect_contains_point(clip, x, y) {
                blend_premultiplied_pixel(frame, x, y, color.try_into().expect("four-byte pixel"));
            }
        }
    }
}

fn blend_premultiplied_pixel(frame: &mut LiveCpuComposedFrame, x: i32, y: i32, source: [u8; 4]) {
    if source[3] == u8::MAX {
        put_pixel(frame, x, y, source);
        return;
    }
    let Some(target) = pixel_mut(frame, x, y) else {
        return;
    };
    let inverse_alpha = u16::from(u8::MAX - source[3]);
    for channel in 0..3 {
        let background = u16::from(target[channel]);
        let foreground = u16::from(source[channel]);
        target[channel] = foreground
            .saturating_add((background * inverse_alpha + 127) / 255)
            .min(255) as u8;
    }
    target[3] = u8::MAX;
}

fn pixel_mut(frame: &mut LiveCpuComposedFrame, x: i32, y: i32) -> Option<&mut [u8]> {
    let x = usize::try_from(x).ok()?;
    let y = usize::try_from(y).ok()?;
    let width = usize::try_from(frame.size.width).ok()?;
    let height = usize::try_from(frame.size.height).ok()?;
    let stride = usize::try_from(frame.stride).ok()?;
    if x >= width || y >= height {
        return None;
    }
    let offset = y.checked_mul(stride)?.checked_add(x.checked_mul(4)?)?;
    Arc::get_mut(&mut frame.bytes)?.get_mut(offset..offset.checked_add(4)?)
}

fn put_pixel(frame: &mut LiveCpuComposedFrame, x: i32, y: i32, pixel: [u8; 4]) {
    let Ok(x) = usize::try_from(x) else {
        return;
    };
    let Ok(y) = usize::try_from(y) else {
        return;
    };
    let width = usize::try_from(frame.size.width).unwrap_or(0);
    let height = usize::try_from(frame.size.height).unwrap_or(0);
    let stride = usize::try_from(frame.stride).unwrap_or(0);
    if x >= width || y >= height {
        return;
    }
    let Some(offset) = y
        .checked_mul(stride)
        .and_then(|offset| offset.checked_add(x.saturating_mul(4)))
    else {
        return;
    };
    let Some(target) = Arc::get_mut(&mut frame.bytes)
        .and_then(|bytes| bytes.get_mut(offset..offset.saturating_add(4)))
    else {
        return;
    };
    target.copy_from_slice(&pixel);
}

fn cpu_frame_metrics(bytes: &[u8]) -> (usize, u64) {
    // The checksum is an in-process change detector, not a wire format. Hash
    // whole pixels' storage words so full-screen terminal frames do not pay a
    // serial multiply for every byte. Keep the exact nonzero-byte count for
    // the existing presentation evidence.
    let mut checksum = 0xcbf2_9ce4_8422_2325u64;
    let mut nonzero_pixel_bytes = 0usize;
    let mut words = bytes.chunks_exact(std::mem::size_of::<u64>());
    for word_bytes in words.by_ref() {
        nonzero_pixel_bytes = nonzero_pixel_bytes.saturating_add(
            word_bytes
                .iter()
                .map(|byte| usize::from(*byte != 0))
                .sum::<usize>(),
        );
        let word = u64::from_le_bytes(word_bytes.try_into().expect("exact u64 chunk"));
        checksum = (checksum ^ word).wrapping_mul(0x100_0000_01b3);
    }
    for byte in words.remainder() {
        nonzero_pixel_bytes = nonzero_pixel_bytes.saturating_add(usize::from(*byte != 0));
        checksum = (checksum ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3);
    }
    (nonzero_pixel_bytes, checksum)
}

fn compose_scaled_layer_clipped(
    frame: &mut LiveCpuComposedFrame,
    layer: &LiveCpuCompositionLayerRef<'_>,
    clip: Rect,
) -> bool {
    let buffer = layer.buffer;
    if !matches!(
        buffer.format,
        LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888 | LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888
    ) || buffer.size.width <= 0
        || buffer.size.height <= 0
        || u64::from(buffer.stride) < buffer.size.width as u64 * 4
        || (buffer.bytes.len() as u64) < u64::from(buffer.stride) * buffer.size.height as u64
    {
        return false;
    }
    let Some(target) =
        clip_rect(layer.geometry, clip).and_then(|rect| clip_rect(rect, output_rect(frame.size)))
    else {
        return false;
    };
    for y in target.y..target.y + target.height {
        let source_y = (i64::from(y) - i64::from(layer.geometry.y)) * i64::from(buffer.size.height)
            / i64::from(layer.geometry.height);
        for x in target.x..target.x + target.width {
            let source_x = (i64::from(x) - i64::from(layer.geometry.x))
                * i64::from(buffer.size.width)
                / i64::from(layer.geometry.width);
            let offset = source_y as usize * buffer.stride as usize + source_x as usize * 4;
            let mut pixel: [u8; 4] = buffer.bytes[offset..offset + 4].try_into().unwrap();
            if buffer.format == LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888 {
                pixel[3] = 255;
            }
            blend_premultiplied_pixel(frame, x, y, pixel);
        }
    }
    true
}

fn compose_layer(frame: &mut LiveCpuComposedFrame, layer: &LiveCpuCompositionLayerRef<'_>) -> bool {
    compose_layer_clipped(frame, layer, output_rect(frame.size))
}

fn compose_layer_clipped(
    frame: &mut LiveCpuComposedFrame,
    layer: &LiveCpuCompositionLayerRef<'_>,
    clip: Rect,
) -> bool {
    if !matches!(
        layer.buffer.format,
        LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888 | LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888
    ) || layer.geometry.width <= 0
        || layer.geometry.height <= 0
        || layer.buffer.size.width <= 0
        || layer.buffer.size.height <= 0
    {
        return false;
    }
    let source_width = usize::try_from(layer.buffer.size.width).unwrap_or(0);
    let source_height = usize::try_from(layer.buffer.size.height).unwrap_or(0);
    let source_stride = usize::try_from(layer.buffer.stride).unwrap_or(0);
    if source_stride < source_width.saturating_mul(4)
        || layer.buffer.bytes.len() < source_stride.saturating_mul(source_height)
    {
        return false;
    }
    let drawable = Rect {
        x: layer.geometry.x,
        y: layer.geometry.y,
        width: layer.geometry.width.min(layer.buffer.size.width),
        height: layer.geometry.height.min(layer.buffer.size.height),
    };
    let Some(target) =
        clip_rect(drawable, clip).and_then(|rect| clip_rect(rect, output_rect(frame.size)))
    else {
        return false;
    };
    let target_stride = usize::try_from(frame.stride).unwrap_or(0);
    let Ok(source_x) = usize::try_from(i64::from(target.x) - i64::from(layer.geometry.x)) else {
        return false;
    };
    let Ok(source_y) = usize::try_from(i64::from(target.y) - i64::from(layer.geometry.y)) else {
        return false;
    };
    let Ok(target_x) = usize::try_from(target.x) else {
        return false;
    };
    let Ok(target_y) = usize::try_from(target.y) else {
        return false;
    };
    let copy_width = usize::try_from(target.width).unwrap_or(0);
    let copy_height = usize::try_from(target.height).unwrap_or(0);
    if copy_width == 0 || copy_height == 0 {
        return false;
    }
    let mut copied = false;
    let row_bytes = copy_width.saturating_mul(4);
    for row in 0..copy_height {
        let source_offset = source_y
            .saturating_add(row)
            .saturating_mul(source_stride)
            .saturating_add(source_x.saturating_mul(4));
        let target_offset = target_y
            .saturating_add(row)
            .saturating_mul(target_stride)
            .saturating_add(target_x.saturating_mul(4));
        let Some(source) = layer
            .buffer
            .bytes
            .get(source_offset..source_offset.saturating_add(row_bytes))
        else {
            continue;
        };
        let Some(target) = Arc::get_mut(&mut frame.bytes).and_then(|bytes| {
            bytes.get_mut(target_offset..target_offset.saturating_add(row_bytes))
        }) else {
            continue;
        };
        if layer.buffer.format == LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888 {
            for (dst, src) in target.chunks_exact_mut(4).zip(source.chunks_exact(4)) {
                let inverse = u16::from(255 - src[3]);
                for channel in 0..3 {
                    dst[channel] = (u16::from(src[channel])
                        + (u16::from(dst[channel]) * inverse + 127) / 255)
                        .min(255) as u8;
                }
                dst[3] = 255;
            }
        } else {
            target.copy_from_slice(source);
        }
        copied = true;
    }
    copied
}

fn clear_frame_rect(frame: &mut LiveCpuComposedFrame, rect: Rect) -> bool {
    let Some(rect) = clip_rect(rect, output_rect(frame.size)) else {
        return false;
    };
    let Ok(start_x) = usize::try_from(rect.x) else {
        return false;
    };
    let Ok(start_y) = usize::try_from(rect.y) else {
        return false;
    };
    let Ok(width) = usize::try_from(rect.width) else {
        return false;
    };
    let Ok(height) = usize::try_from(rect.height) else {
        return false;
    };
    let Ok(stride) = usize::try_from(frame.stride) else {
        return false;
    };
    let Some(bytes) = Arc::get_mut(&mut frame.bytes) else {
        return false;
    };
    for y in start_y..start_y.saturating_add(height) {
        let row_start = y
            .saturating_mul(stride)
            .saturating_add(start_x.saturating_mul(4));
        let row_end = row_start.saturating_add(width.saturating_mul(4));
        let Some(row) = bytes.get_mut(row_start..row_end) else {
            return false;
        };
        row.fill(0);
    }
    true
}

const fn output_rect(size: Size) -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: size.width,
        height: size.height,
    }
}

fn clip_rect(rect: Rect, clip: Rect) -> Option<Rect> {
    if rect.is_empty() || clip.is_empty() {
        return None;
    }
    let left = i64::from(rect.x).max(i64::from(clip.x));
    let top = i64::from(rect.y).max(i64::from(clip.y));
    let right = i64::from(rect.x)
        .saturating_add(i64::from(rect.width))
        .min(i64::from(clip.x).saturating_add(i64::from(clip.width)));
    let bottom = i64::from(rect.y)
        .saturating_add(i64::from(rect.height))
        .min(i64::from(clip.y).saturating_add(i64::from(clip.height)));
    if left >= right || top >= bottom {
        return None;
    }
    Some(Rect {
        x: i32::try_from(left).ok()?,
        y: i32::try_from(top).ok()?,
        width: i32::try_from(right.saturating_sub(left)).ok()?,
        height: i32::try_from(bottom.saturating_sub(top)).ok()?,
    })
}

fn rect_contains_point(rect: Rect, x: i32, y: i32) -> bool {
    if rect.is_empty() {
        return false;
    }
    let x = i64::from(x);
    let y = i64::from(y);
    let left = i64::from(rect.x);
    let top = i64::from(rect.y);
    x >= left
        && x < left.saturating_add(i64::from(rect.width))
        && y >= top
        && y < top.saturating_add(i64::from(rect.height))
}
