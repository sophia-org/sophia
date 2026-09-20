//! Validated wire images lowered to the authority's little-endian pixel store.

use std::borrow::Cow;

use super::{
    X_IMAGE_FORMAT_Z_PIXMAP, XImageLayout, XImageLayoutError, depth_mask, padded_scanline_bytes,
};
use crate::{XByteOrder, XErrorCode, XGraphicsContextValues};

/// Setup advertises matching image/bitmap byte and bit orders, with 32-bit
/// bitmap units/padding. Validate the entire payload before any drawable write.
#[allow(clippy::too_many_arguments)]
pub(crate) fn decode_upload<'a>(
    format: u8,
    depth: u8,
    width: u16,
    height: u16,
    left_pad: u8,
    order: XByteOrder,
    gc: &XGraphicsContextValues,
    data: &'a [u8],
) -> Result<Cow<'a, [u8]>, XErrorCode> {
    if format > 2 {
        return Err(XErrorCode::BadValue);
    }
    // Left-pad must be zero for ZPixmap and below the bitmap scanline pad
    // for the XY formats; the protocol makes both a Match error, not Value.
    if (format == X_IMAGE_FORMAT_Z_PIXMAP && left_pad != 0) || left_pad >= 32 {
        return Err(XErrorCode::BadMatch);
    }
    if format == 0 && depth != 1 {
        return Err(XErrorCode::BadMatch);
    }
    let layout =
        XImageLayout::new(format.max(1), depth, width, height, u32::MAX).map_err(|error| {
            match error {
                XImageLayoutError::InvalidDepth => XErrorCode::BadMatch,
                _ => XErrorCode::BadAlloc,
            }
        })?;
    let width = usize::from(width);
    let height = usize::from(height);
    let stride = if format == X_IMAGE_FORMAT_Z_PIXMAP {
        layout.row_stride
    } else {
        padded_scanline_bytes(width + usize::from(left_pad), 1, 32)
            .map_err(|_| XErrorCode::BadAlloc)?
    };
    let planes = if format == 1 { usize::from(depth) } else { 1 };
    let required = stride
        .checked_mul(height)
        .and_then(|n| n.checked_mul(planes))
        .ok_or(XErrorCode::BadLength)?;
    if data.len() != required {
        return Err(XErrorCode::BadLength);
    }
    if format == 2 && layout.bits_per_pixel == 32 && order == XByteOrder::LittleEndian {
        return Ok(Cow::Borrowed(data));
    }
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(layout.canonical_len)
        .map_err(|_| XErrorCode::BadAlloc)?;
    for y in 0..height {
        for x in 0..width {
            let mut pixel = 0u32;
            if format == 2 {
                let row = &data[y * stride..(y + 1) * stride];
                pixel = match layout.bits_per_pixel {
                    32 => order.u32(&row[x * 4..x * 4 + 4]),
                    16 => u32::from(order.u16(&row[x * 2..x * 2 + 2])),
                    8 => u32::from(row[x]),
                    bits => {
                        let bits = usize::from(bits);
                        let per_byte = 8 / bits;
                        let index = x % per_byte;
                        let shift = match order {
                            XByteOrder::LittleEndian => index * bits,
                            XByteOrder::BigEndian => (per_byte - 1 - index) * bits,
                        };
                        u32::from(row[x / per_byte] >> shift) & ((1 << bits) - 1)
                    }
                };
            } else {
                let bit_x = x + usize::from(left_pad);
                let shift = match order {
                    XByteOrder::LittleEndian => bit_x % 8,
                    XByteOrder::BigEndian => 7 - bit_x % 8,
                };
                for plane in 0..planes {
                    let byte = data[(plane * height + y) * stride + bit_x / 8];
                    pixel = (pixel << 1) | u32::from((byte >> shift) & 1);
                }
            }
            pixel &= depth_mask(depth);
            if format == 0 {
                pixel = if pixel != 0 {
                    gc.foreground
                } else {
                    gc.background
                };
            }
            pixels.extend_from_slice(&pixel.to_le_bytes());
        }
    }
    Ok(Cow::Owned(pixels))
}
