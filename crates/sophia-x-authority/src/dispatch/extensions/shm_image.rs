// Cutting a rectangle out of a client's shared-memory image for ShmPutImage.
// Included by dispatch.rs; one module with it.

struct XShmImageCopy {
    byte_order: XByteOrder,
    offset: u32,
    total_width: u16,
    total_height: u16,
    src_x: u16,
    src_y: u16,
    src_width: u16,
    src_height: u16,
    depth: u8,
    format: u8,
}

/// Cuts a rectangle out of a client's shared-memory image.
///
/// The bytes arrive through `read` rather than from a SysV id, because a
/// segment can be named either way now and the arithmetic below is the same
/// for both. `read` is handed the offset and length this validated, so a
/// caller cannot be asked for a region the checks here did not approve.
fn copy_shm_image_region(
    copy: XShmImageCopy,
    read: impl FnOnce(usize, usize) -> Option<Vec<u8>>,
) -> Option<Vec<u8>> {
    let XShmImageCopy {
        byte_order,
        offset,
        total_width,
        total_height,
        src_x,
        src_y,
        src_width,
        src_height,
        depth,
        format,
    } = copy;
    const Z_PIXMAP: u8 = 2;
    const BYTES_PER_PIXEL: usize = 4;
    const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
    if format != Z_PIXMAP {
        return None;
    }
    let total_width = usize::from(total_width);
    let total_height = usize::from(total_height);
    let src_x = usize::from(src_x);
    let src_y = usize::from(src_y);
    let src_width = usize::from(src_width);
    let src_height = usize::from(src_height);
    if src_x.checked_add(src_width)? > total_width || src_y.checked_add(src_height)? > total_height
    {
        return None;
    }
    // Shared images use the setup-advertised pixel format and scanline pad,
    // including packed one-bit masks used by GTK during startup. Decode through
    // the core upload path before cropping into the canonical pixel store.
    let layout = crate::image::XImageLayout::new(
        format,
        depth,
        u16::try_from(total_width).ok()?,
        u16::try_from(total_height).ok()?,
        u32::MAX,
    )
    .ok()?;
    if layout.payload_len > MAX_IMAGE_BYTES {
        return None;
    }
    let source = read(usize::try_from(offset).ok()?, layout.payload_len)?;
    let source = crate::image::decode_upload(
        format,
        depth,
        u16::try_from(total_width).ok()?,
        u16::try_from(total_height).ok()?,
        0,
        byte_order,
        &crate::XGraphicsContextValues::default(),
        &source,
    )
    .ok()?;
    let stride = total_width.checked_mul(BYTES_PER_PIXEL)?;
    let row_len = src_width.checked_mul(BYTES_PER_PIXEL)?;
    let mut image = Vec::with_capacity(row_len.checked_mul(src_height)?);
    for row in src_y..src_y.checked_add(src_height)? {
        let start = row
            .checked_mul(stride)?
            .checked_add(src_x.checked_mul(BYTES_PER_PIXEL)?)?;
        image.extend_from_slice(source.get(start..start.checked_add(row_len)?)?);
    }
    Some(image)
}
