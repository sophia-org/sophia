//! Allocation placement in acknowledged physical coordinates.
use super::*;

pub(super) fn panel_rect(
    request: &sophia_protocol::ContentAllocationRequest,
    logical_output: Size,
    width: i32,
    height: i32,
) -> Result<sophia_protocol::ContentLogicalRect, sophia_runtime::ContentAllocationError> {
    let x = match request.edge {
        2 => logical_output
            .width
            .checked_sub(width)
            .and_then(|value| value.checked_sub(i32::from(request.margins.right))),
        _ => Some(i32::from(request.margins.left)),
    }
    .ok_or(sophia_runtime::ContentAllocationError::Malformed)?;
    let y = match request.edge {
        3 => logical_output
            .height
            .checked_sub(height)
            .and_then(|value| value.checked_sub(i32::from(request.margins.bottom))),
        _ => Some(i32::from(request.margins.top)),
    }
    .ok_or(sophia_runtime::ContentAllocationError::Malformed)?;
    Ok(sophia_protocol::ContentLogicalRect {
        x,
        y,
        width: request.desired_width,
        height: request.desired_height,
    })
}

/// Places a popout wholly inside its parent output. The request edge names the
/// panel edge, so it supplies the first tie-break (away from the panel); the
/// actual side is carried by the resolved rectangle and cannot be changed by
/// the client after acknowledgement.
pub(super) fn popout_rect(
    request: &sophia_protocol::ContentAllocationRequest,
    output: HeadlessOutput,
    parent: &ContentAllocationSnapshot,
) -> Result<
    (
        sophia_protocol::ContentLogicalRect,
        sophia_protocol::ContentPixelRect,
    ),
    sophia_runtime::ContentAllocationError,
> {
    let numerator = i128::from(parent.scale_numerator);
    let denominator = i128::from(parent.scale_denominator);
    if numerator == 0 || numerator > 32 || denominator == 0 || denominator > 4 {
        return Err(sophia_runtime::ContentAllocationError::Malformed);
    }
    let extent = |value: u32| {
        let scaled = i128::from(value) * numerator;
        i32::try_from(scaled / denominator + i128::from(scaled % denominator != 0)).ok()
    };
    let width_px = extent(request.desired_width)
        .filter(|value| *value > 0)
        .ok_or(sophia_runtime::ContentAllocationError::Malformed)?;
    let height_px = extent(request.desired_height)
        .filter(|value| *value > 0)
        .ok_or(sophia_runtime::ContentAllocationError::Malformed)?;
    // Margins quantize their magnitude outwards, then restore the sign.
    let margin = |value: i16| {
        extent(u32::from(value.unsigned_abs()))
            .map(|scaled| if value < 0 { -scaled } else { scaled })
    };
    let [top, right, bottom, left] = [
        margin(request.margins.top),
        margin(request.margins.right),
        margin(request.margins.bottom),
        margin(request.margins.left),
    ]
    .map(|value| value.ok_or(sophia_runtime::ContentAllocationError::Malformed));
    let (top, right, bottom, left) = (top?, right?, bottom?, left?);
    let anchor = request.anchor_parent_rect;
    if anchor.width == 0
        || anchor.height == 0
        || anchor.x < 0
        || anchor.y < 0
        || anchor
            .x
            .saturating_add(i32::try_from(anchor.width).unwrap_or(i32::MAX))
            > i32::try_from(parent.pixel.width).unwrap_or(i32::MAX)
        || anchor
            .y
            .saturating_add(i32::try_from(anchor.height).unwrap_or(i32::MAX))
            > i32::try_from(parent.pixel.height).unwrap_or(i32::MAX)
    {
        return Err(sophia_runtime::ContentAllocationError::Malformed);
    }
    let anchor = Rect {
        x: parent.pixel.x.saturating_add(anchor.x),
        y: parent.pixel.y.saturating_add(anchor.y),
        width: i32::try_from(anchor.width)
            .map_err(|_| sophia_runtime::ContentAllocationError::Malformed)?,
        height: i32::try_from(anchor.height)
            .map_err(|_| sophia_runtime::ContentAllocationError::Malformed)?,
    };
    let output_size = output.size;
    let side_room = [
        (1_u16, anchor.y.saturating_sub(bottom)),
        (
            2,
            output_size
                .width
                .saturating_sub(anchor.x.saturating_add(anchor.width))
                .saturating_sub(left),
        ),
        (
            3,
            output_size
                .height
                .saturating_sub(anchor.y.saturating_add(anchor.height))
                .saturating_sub(top),
        ),
        (4, anchor.x.saturating_sub(right)),
    ];
    let away = match request.edge {
        1 => 3,
        2 => 4,
        3 => 1,
        4 => 2,
        _ => return Err(sophia_runtime::ContentAllocationError::Malformed),
    };
    let rank = |side| {
        if side == away {
            0
        } else {
            match side {
                1 => 1,
                4 => 2,
                3 => 3,
                2 => 4,
                _ => 5,
            }
        }
    };
    let mut sides = side_room;
    sides.sort_by_key(|(side, room)| (std::cmp::Reverse(*room), rank(*side)));
    for (side, room) in sides {
        let required = if matches!(side, 1 | 3) {
            height_px
        } else {
            width_px
        };
        if room < required {
            continue;
        }
        let x = match side {
            2 => anchor.x.saturating_add(anchor.width).saturating_add(left),
            4 => anchor.x.saturating_sub(right).saturating_sub(width_px),
            _ => anchor.x.saturating_add(left),
        };
        let y = match side {
            1 => anchor.y.saturating_sub(bottom).saturating_sub(height_px),
            3 => anchor.y.saturating_add(anchor.height).saturating_add(top),
            _ => anchor.y.saturating_add(top),
        };
        let rect = Rect {
            x,
            y,
            width: width_px,
            height: height_px,
        };
        if rect.x >= 0
            && rect.y >= 0
            && rect.x.saturating_add(rect.width) <= output_size.width
            && rect.y.saturating_add(rect.height) <= output_size.height
        {
            return Ok((
                sophia_protocol::ContentLogicalRect {
                    x: i32::try_from((i128::from(rect.x) * denominator).div_euclid(numerator))
                        .map_err(|_| sophia_runtime::ContentAllocationError::Malformed)?,
                    y: i32::try_from((i128::from(rect.y) * denominator).div_euclid(numerator))
                        .map_err(|_| sophia_runtime::ContentAllocationError::Malformed)?,
                    width: request.desired_width,
                    height: request.desired_height,
                },
                sophia_protocol::ContentPixelRect {
                    x: rect.x,
                    y: rect.y,
                    width: u32::try_from(rect.width)
                        .map_err(|_| sophia_runtime::ContentAllocationError::Malformed)?,
                    height: u32::try_from(rect.height)
                        .map_err(|_| sophia_runtime::ContentAllocationError::Malformed)?,
                },
            ));
        }
    }
    Err(sophia_runtime::ContentAllocationError::Budget)
}

pub(super) fn panel_pixel_thickness(pixel: sophia_protocol::ContentPixelRect, edge: u16) -> u32 {
    if matches!(edge, 1 | 3) {
        pixel.height
    } else {
        pixel.width
    }
}

pub(super) fn quantize(
    logical: sophia_protocol::ContentLogicalRect,
    scale: u32,
) -> Option<sophia_protocol::ContentPixelRect> {
    Some(sophia_protocol::ContentPixelRect {
        x: logical.x.checked_mul(i32::try_from(scale).ok()?)?,
        y: logical.y.checked_mul(i32::try_from(scale).ok()?)?,
        width: logical.width.checked_mul(scale)?,
        height: logical.height.checked_mul(scale)?,
    })
}
