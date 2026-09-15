use super::*;

pub(super) fn compose_solid_rect(
    frame: &mut LiveCpuComposedFrame,
    geometry: Rect,
    color: CompositorRgb8,
    opacity: u8,
) -> bool {
    compose_solid_rect_clipped(frame, geometry, color, opacity, output_rect(frame.size))
}

pub(super) fn compose_solid_rect_clipped(
    frame: &mut LiveCpuComposedFrame,
    geometry: Rect,
    color: CompositorRgb8,
    opacity: u8,
    clip: Rect,
) -> bool {
    let Some(target) =
        clip_rect(geometry, clip).and_then(|rect| clip_rect(rect, output_rect(frame.size)))
    else {
        return false;
    };
    let Ok(start_x) = usize::try_from(target.x) else {
        return false;
    };
    let Ok(start_y) = usize::try_from(target.y) else {
        return false;
    };
    let Ok(width) = usize::try_from(target.width) else {
        return false;
    };
    let Ok(height) = usize::try_from(target.height) else {
        return false;
    };
    let Ok(stride) = usize::try_from(frame.stride) else {
        return false;
    };
    let premul = |c: u8| ((u16::from(c) * u16::from(opacity) + 127) / 255) as u8;
    let pixel = [
        premul(color.blue),
        premul(color.green),
        premul(color.red),
        opacity,
    ];
    if opacity != 255 {
        for y in start_y..start_y + height {
            for x in start_x..start_x + width {
                blend_premultiplied_pixel(frame, x as i32, y as i32, pixel);
            }
        }
        return true;
    }
    for y in start_y..start_y.saturating_add(height) {
        let row_start = y
            .saturating_mul(stride)
            .saturating_add(start_x.saturating_mul(4));
        let row_end = row_start.saturating_add(width.saturating_mul(4));
        let Some(row) =
            Arc::get_mut(&mut frame.bytes).and_then(|bytes| bytes.get_mut(row_start..row_end))
        else {
            return false;
        };
        for target in row.chunks_exact_mut(4) {
            target.copy_from_slice(&pixel);
        }
    }
    true
}

/// A one-pixel immutable texture; placement supplies coverage and opacity.
pub fn solid_color_buffer(color: CompositorRgb8) -> LiveSharedCpuBufferSource {
    let packed = u64::from(color.red) << 16 | u64::from(color.green) << 8 | u64::from(color.blue);
    LiveSharedCpuBufferSource {
        handle: 0x8300_0000_0000_0000 | packed,
        size: Size {
            width: 1,
            height: 1,
        },
        stride: 4,
        format: LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888,
        generation: 1,
        bytes: Arc::new(vec![color.blue, color.green, color.red, 255]).into(),
    }
}
