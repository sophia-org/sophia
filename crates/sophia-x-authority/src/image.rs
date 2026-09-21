use sophia_protocol::{NamespaceId, Rect};

mod trace;
mod upload;
pub(crate) use trace::trace_image_pixels;
pub(crate) use upload::decode_upload;

use crate::{
    X_AUTHORITY_SOFTWARE_BUFFER_MAX_BYTES, XAuthorityRuntime, XAuthorityRuntimeError, XByteOrder,
    XClientError, XErrorCode, XResourceId, x11_pixmap_format,
};

pub(crate) const X_IMAGE_FORMAT_XY_PIXMAP: u8 = 1;
pub(crate) const X_IMAGE_FORMAT_Z_PIXMAP: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum XImageLayoutError {
    InvalidFormat,
    InvalidDepth,
    TooLarge,
    AllocationFailed,
    InvalidPixels,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum XImageReadbackError {
    Drawable(crate::runtime::XDrawableImageError),
    Layout(XImageLayoutError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct XImageReadback {
    pub depth: u8,
    pub visual: u32,
    pub data: Vec<u8>,
}

/// Keeps core and MIT-SHM image replies on the same validation and packing path.
pub(crate) fn read_drawable_image(
    runtime: &XAuthorityRuntime,
    namespace: NamespaceId,
    drawable: XResourceId,
    region: Rect,
    format: u8,
    plane_mask: u32,
    byte_order: XByteOrder,
) -> Result<XImageReadback, XImageReadbackError> {
    let descriptor = runtime
        .drawable_image_descriptor(namespace, drawable)
        .map_err(XImageReadbackError::Drawable)?;
    runtime
        .validate_drawable_image_region(descriptor, region)
        .map_err(XImageReadbackError::Drawable)?;
    let width = u16::try_from(region.width).map_err(|_| {
        XImageReadbackError::Drawable(crate::runtime::XDrawableImageError::BadMatch)
    })?;
    let height = u16::try_from(region.height).map_err(|_| {
        XImageReadbackError::Drawable(crate::runtime::XDrawableImageError::BadMatch)
    })?;
    let layout = XImageLayout::new(format, descriptor.depth, width, height, plane_mask)
        .map_err(XImageReadbackError::Layout)?;
    // Readback is passive: absent CPU backing zero-fills instead of creating a
    // renderer or screen-capture path across the Engine boundary.
    let pixels = runtime
        .read_drawable_image_region(drawable, descriptor, region)
        .map_err(XImageReadbackError::Drawable)?;
    let data = layout
        .encode(byte_order, plane_mask, &pixels)
        .map_err(XImageReadbackError::Layout)?;
    Ok(XImageReadback {
        depth: descriptor.depth,
        visual: descriptor.visual,
        data,
    })
}

pub(crate) fn image_client_error(
    sequence: u16,
    major_code: u8,
    minor_code: u16,
    drawable: XResourceId,
    format: u8,
    error: XImageReadbackError,
) -> XClientError {
    let code = match error {
        XImageReadbackError::Drawable(crate::runtime::XDrawableImageError::Access(error)) => {
            match error {
                XAuthorityRuntimeError::InvalidResource
                | XAuthorityRuntimeError::InvalidSurface
                | XAuthorityRuntimeError::UnknownResource
                | XAuthorityRuntimeError::WrongResourceKind => XErrorCode::BadDrawable,
                XAuthorityRuntimeError::InvalidNamespace
                | XAuthorityRuntimeError::CrossNamespaceDenied
                | XAuthorityRuntimeError::StaleGeneration
                | XAuthorityRuntimeError::UnknownRequestorNamespace
                | XAuthorityRuntimeError::UnknownSourceOwner
                | XAuthorityRuntimeError::MissingSourceNamespace
                | XAuthorityRuntimeError::SameNamespace
                | XAuthorityRuntimeError::PortalRejected => XErrorCode::BadAccess,
                XAuthorityRuntimeError::FocusAuthorityUnavailable => XErrorCode::BadImplementation,
                XAuthorityRuntimeError::InvalidValue => XErrorCode::BadValue,
                // A readback names a drawable rather than a focus, so this
                // cannot arise here; it maps the way the protocol names it.
                XAuthorityRuntimeError::WindowNotViewable => XErrorCode::BadMatch,
            }
        }
        XImageReadbackError::Drawable(crate::runtime::XDrawableImageError::BadMatch) => {
            XErrorCode::BadMatch
        }
        XImageReadbackError::Drawable(crate::runtime::XDrawableImageError::AllocationFailed)
        | XImageReadbackError::Layout(XImageLayoutError::TooLarge)
        | XImageReadbackError::Layout(XImageLayoutError::AllocationFailed) => XErrorCode::BadAlloc,
        XImageReadbackError::Layout(XImageLayoutError::InvalidFormat) => XErrorCode::BadValue,
        XImageReadbackError::Layout(XImageLayoutError::InvalidDepth) => XErrorCode::BadMatch,
        XImageReadbackError::Layout(XImageLayoutError::InvalidPixels) => {
            XErrorCode::BadImplementation
        }
    };
    XClientError {
        code,
        sequence,
        resource_id: if code == XErrorCode::BadValue {
            u32::from(format)
        } else {
            u32::try_from(drawable.local.raw()).unwrap_or(0)
        },
        minor_code,
        major_code,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct XImageLayout {
    pub format: u8,
    pub depth: u8,
    pub width: u16,
    pub height: u16,
    pub bits_per_pixel: u8,
    pub row_stride: usize,
    pub plane_count: u32,
    pub payload_len: usize,
    pub canonical_len: usize,
}

impl XImageLayout {
    pub(crate) fn new(
        format: u8,
        depth: u8,
        width: u16,
        height: u16,
        plane_mask: u32,
    ) -> Result<Self, XImageLayoutError> {
        if !matches!(format, X_IMAGE_FORMAT_XY_PIXMAP | X_IMAGE_FORMAT_Z_PIXMAP) {
            return Err(XImageLayoutError::InvalidFormat);
        }
        let pixmap_format = x11_pixmap_format(depth).ok_or(XImageLayoutError::InvalidDepth)?;
        let width = usize::from(width);
        let height = usize::from(height);
        let canonical_len = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(XImageLayoutError::TooLarge)?;
        let (bits_per_pixel, row_stride, plane_count) = if format == X_IMAGE_FORMAT_Z_PIXMAP {
            (
                pixmap_format.bits_per_pixel,
                padded_scanline_bytes(
                    width,
                    usize::from(pixmap_format.bits_per_pixel),
                    usize::from(pixmap_format.scanline_pad),
                )?,
                1,
            )
        } else {
            (
                1,
                padded_scanline_bytes(width, 1, usize::from(pixmap_format.scanline_pad))?,
                (plane_mask & depth_mask(depth)).count_ones(),
            )
        };
        let payload_len = row_stride
            .checked_mul(height)
            .and_then(|plane_len| plane_len.checked_mul(usize::try_from(plane_count).ok()?))
            .ok_or(XImageLayoutError::TooLarge)?;
        if canonical_len > X_AUTHORITY_SOFTWARE_BUFFER_MAX_BYTES
            || payload_len > X_AUTHORITY_SOFTWARE_BUFFER_MAX_BYTES
        {
            return Err(XImageLayoutError::TooLarge);
        }
        Ok(Self {
            format,
            depth,
            width: u16::try_from(width).map_err(|_| XImageLayoutError::TooLarge)?,
            height: u16::try_from(height).map_err(|_| XImageLayoutError::TooLarge)?,
            bits_per_pixel,
            row_stride,
            plane_count,
            payload_len,
            canonical_len,
        })
    }

    pub(crate) fn encode(
        self,
        byte_order: XByteOrder,
        plane_mask: u32,
        pixels: &[u8],
    ) -> Result<Vec<u8>, XImageLayoutError> {
        if pixels.len() != self.canonical_len {
            return Err(XImageLayoutError::InvalidPixels);
        }
        let mut out = Vec::new();
        out.try_reserve_exact(self.payload_len)
            .map_err(|_| XImageLayoutError::AllocationFailed)?;
        out.resize(self.payload_len, 0);
        if self.payload_len == 0 {
            return Ok(out);
        }
        if self.format == X_IMAGE_FORMAT_Z_PIXMAP {
            self.encode_z_pixmap(byte_order, plane_mask, pixels, &mut out)?;
        } else {
            self.encode_xy_pixmap(byte_order, plane_mask, pixels, &mut out)?;
        }
        Ok(out)
    }

    fn encode_z_pixmap(
        self,
        byte_order: XByteOrder,
        plane_mask: u32,
        pixels: &[u8],
        out: &mut [u8],
    ) -> Result<(), XImageLayoutError> {
        let width = usize::from(self.width);
        let height = usize::from(self.height);
        let selected = plane_mask & depth_mask(self.depth);
        for y in 0..height {
            let row_offset = y
                .checked_mul(self.row_stride)
                .ok_or(XImageLayoutError::TooLarge)?;
            for x in 0..width {
                let pixel = canonical_pixel(pixels, width, x, y)? & selected;
                match self.bits_per_pixel {
                    32 => {
                        let offset = row_offset
                            .checked_add(x.checked_mul(4).ok_or(XImageLayoutError::TooLarge)?)
                            .ok_or(XImageLayoutError::TooLarge)?;
                        let bytes = match byte_order {
                            XByteOrder::LittleEndian => pixel.to_le_bytes(),
                            XByteOrder::BigEndian => pixel.to_be_bytes(),
                        };
                        out.get_mut(offset..offset + 4)
                            .ok_or(XImageLayoutError::InvalidPixels)?
                            .copy_from_slice(&bytes);
                    }
                    16 => {
                        let offset = row_offset
                            .checked_add(x.checked_mul(2).ok_or(XImageLayoutError::TooLarge)?)
                            .ok_or(XImageLayoutError::TooLarge)?;
                        let pixel =
                            u16::try_from(pixel).map_err(|_| XImageLayoutError::InvalidPixels)?;
                        let bytes = match byte_order {
                            XByteOrder::LittleEndian => pixel.to_le_bytes(),
                            XByteOrder::BigEndian => pixel.to_be_bytes(),
                        };
                        out.get_mut(offset..offset + 2)
                            .ok_or(XImageLayoutError::InvalidPixels)?
                            .copy_from_slice(&bytes);
                    }
                    8 => {
                        *out.get_mut(row_offset + x)
                            .ok_or(XImageLayoutError::InvalidPixels)? = pixel as u8;
                    }
                    4 => set_packed_pixel(out, row_offset, x, 4, pixel, byte_order)?,
                    1 => set_packed_pixel(out, row_offset, x, 1, pixel, byte_order)?,
                    _ => return Err(XImageLayoutError::InvalidDepth),
                }
            }
        }
        Ok(())
    }

    fn encode_xy_pixmap(
        self,
        byte_order: XByteOrder,
        plane_mask: u32,
        pixels: &[u8],
        out: &mut [u8],
    ) -> Result<(), XImageLayoutError> {
        let width = usize::from(self.width);
        let height = usize::from(self.height);
        let plane_len = self
            .row_stride
            .checked_mul(height)
            .ok_or(XImageLayoutError::TooLarge)?;
        let selected = plane_mask & depth_mask(self.depth);
        let mut plane_index = 0usize;
        for bit in (0..self.depth).rev() {
            let plane = 1u32 << bit;
            if selected & plane == 0 {
                continue;
            }
            let plane_offset = plane_index
                .checked_mul(plane_len)
                .ok_or(XImageLayoutError::TooLarge)?;
            for y in 0..height {
                let row_offset = plane_offset
                    .checked_add(
                        y.checked_mul(self.row_stride)
                            .ok_or(XImageLayoutError::TooLarge)?,
                    )
                    .ok_or(XImageLayoutError::TooLarge)?;
                for x in 0..width {
                    if canonical_pixel(pixels, width, x, y)? & plane != 0 {
                        set_bitmap_bit(out, row_offset, x, byte_order)?;
                    }
                }
            }
            plane_index += 1;
        }
        debug_assert_eq!(u32::try_from(plane_index).ok(), Some(self.plane_count));
        Ok(())
    }
}

fn padded_scanline_bytes(
    width: usize,
    bits_per_pixel: usize,
    scanline_pad: usize,
) -> Result<usize, XImageLayoutError> {
    let bits = width
        .checked_mul(bits_per_pixel)
        .ok_or(XImageLayoutError::TooLarge)?;
    let padded = bits
        .checked_add(scanline_pad.saturating_sub(1))
        .ok_or(XImageLayoutError::TooLarge)?
        / scanline_pad
        * scanline_pad;
    Ok(padded / 8)
}

fn depth_mask(depth: u8) -> u32 {
    if depth >= 32 {
        u32::MAX
    } else {
        (1u32 << depth) - 1
    }
}

fn canonical_pixel(
    pixels: &[u8],
    width: usize,
    x: usize,
    y: usize,
) -> Result<u32, XImageLayoutError> {
    let offset = y
        .checked_mul(width)
        .and_then(|row| row.checked_add(x))
        .and_then(|pixel| pixel.checked_mul(4))
        .ok_or(XImageLayoutError::TooLarge)?;
    Ok(u32::from_le_bytes(
        pixels
            .get(offset..offset + 4)
            .ok_or(XImageLayoutError::InvalidPixels)?
            .try_into()
            .map_err(|_| XImageLayoutError::InvalidPixels)?,
    ))
}

fn set_packed_pixel(
    out: &mut [u8],
    row_offset: usize,
    x: usize,
    bits_per_pixel: usize,
    pixel: u32,
    byte_order: XByteOrder,
) -> Result<(), XImageLayoutError> {
    let pixels_per_byte = 8 / bits_per_pixel;
    let byte = out
        .get_mut(row_offset + x / pixels_per_byte)
        .ok_or(XImageLayoutError::InvalidPixels)?;
    let within = x % pixels_per_byte;
    let shift = match byte_order {
        XByteOrder::LittleEndian => within * bits_per_pixel,
        XByteOrder::BigEndian => (pixels_per_byte - 1 - within) * bits_per_pixel,
    };
    let value_mask = (1u8 << bits_per_pixel) - 1;
    *byte |= ((pixel as u8) & value_mask) << shift;
    Ok(())
}

fn set_bitmap_bit(
    out: &mut [u8],
    row_offset: usize,
    x: usize,
    byte_order: XByteOrder,
) -> Result<(), XImageLayoutError> {
    let bit = match byte_order {
        XByteOrder::LittleEndian => x % 8,
        XByteOrder::BigEndian => 7 - (x % 8),
    };
    *out.get_mut(row_offset + x / 8)
        .ok_or(XImageLayoutError::InvalidPixels)? |= 1 << bit;
    Ok(())
}
