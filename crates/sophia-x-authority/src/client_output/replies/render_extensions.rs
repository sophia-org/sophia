fn encode_render_extension_reply(
    byte_order: XByteOrder,
    reply: XClientReply,
) -> Result<Vec<u8>, XClientReply> {
    if !matches!(
        &reply,
            XClientReply::ShmQueryVersion { .. }
            | XClientReply::XCMiscGetVersion { .. }
            | XClientReply::XCMiscGetXIDRange { .. }
            | XClientReply::XCMiscGetXIDList { .. }
            | XClientReply::XF86VidModeQueryVersion { .. }
            | XClientReply::XF86VidModeGetModeLine { .. }
            | XClientReply::ShmCreateSegment { .. }
            | XClientReply::ShmGetImage { .. }
            | XClientReply::Dri3QueryVersion { .. }
            | XClientReply::Dri3Open { .. }
            | XClientReply::Dri3GetSupportedModifiers { .. }
            | XClientReply::Dri3BufferFromPixmap { .. }
            | XClientReply::Dri3BuffersFromPixmap { .. }
            | XClientReply::XfixesQueryVersion { .. }
            | XClientReply::XfixesFetchRegion { .. }
            | XClientReply::ShapeQueryVersion { .. }
            | XClientReply::XTestGetVersion { .. }
            | XClientReply::XTestCompareCursor { .. }
            | XClientReply::ShapeQueryExtents { .. }
            | XClientReply::ShapeInputSelected { .. }
            | XClientReply::ShapeGetRectangles { .. }
            | XClientReply::PresentQueryVersion { .. }
            | XClientReply::PresentQueryCapabilities { .. }
    ) {
        return Err(reply);
    }
    Ok(match reply {
                XClientReply::ShmQueryVersion {
                    sequence,
                    major_version,
                    minor_version,
                    shared_pixmaps,
                    pixmap_format,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    out[1] = u8::from(shared_pixmaps);
                    put_u16(byte_order, &mut out[8..10], major_version);
                    put_u16(byte_order, &mut out[10..12], minor_version);
                    out[16] = pixmap_format;
                    out
                }
                XClientReply::ShmGetImage {
                    sequence,
                    depth,
                    visual,
                    size,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    out[1] = depth;
                    put_u32(byte_order, &mut out[8..12], visual);
                    put_u32(byte_order, &mut out[12..16], size);
                    out
                }
                XClientReply::Dri3QueryVersion {
                    sequence,
                    major_version,
                    minor_version,
                }
                | XClientReply::XfixesQueryVersion {
                    sequence,
                    major_version,
                    minor_version,
                }
                | XClientReply::PresentQueryVersion {
                    sequence,
                    major_version,
                    minor_version,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    put_u32(byte_order, &mut out[8..12], major_version);
                    put_u32(byte_order, &mut out[12..16], minor_version);
                    out
                }
                XClientReply::Dri3Open { sequence } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    out[1] = 1;
                    out
                }
                XClientReply::ShapeQueryVersion {
                    sequence,
                    major_version,
                    minor_version,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    put_u16(byte_order, &mut out[8..10], major_version);
                    put_u16(byte_order, &mut out[10..12], minor_version);
                    out
                }
                XClientReply::XTestGetVersion {
                    sequence,
                    major_version,
                    minor_version,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    // The major rides the detail byte and the minor starts at
                    // eight, which is not where the request carries them.
                    out[1] = major_version;
                    put_u16(byte_order, &mut out[8..10], minor_version);
                    out
                }
                XClientReply::XTestCompareCursor { sequence, same } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    // The answer is the detail byte, which is where a reply
                    // with nothing else to say puts its one value.
                    out[1] = u8::from(same);
                    out
                }
                XClientReply::ShapeQueryExtents {
                    sequence,
                    bounding_shaped,
                    clip_shaped,
                    bounding_extents,
                    clip_extents,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    out[8] = u8::from(bounding_shaped);
                    out[9] = u8::from(clip_shaped);
                    for (offset, extents) in
                        [(12, bounding_extents), (20, clip_extents)]
                    {
                        put_i16(byte_order, &mut out[offset..offset + 2], extents.x as i16);
                        put_i16(
                            byte_order,
                            &mut out[offset + 2..offset + 4],
                            extents.y as i16,
                        );
                        put_u16(
                            byte_order,
                            &mut out[offset + 4..offset + 6],
                            extents.width as u16,
                        );
                        put_u16(
                            byte_order,
                            &mut out[offset + 6..offset + 8],
                            extents.height as u16,
                        );
                    }
                    out
                }
                XClientReply::ShapeInputSelected { sequence, enabled } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    // The status rides in the reply's detail byte, where a
                    // reply with no body carries its one datum.
                    out[1] = u8::from(enabled);
                    out
                }
                XClientReply::ShapeGetRectangles {
                    sequence,
                    ordering,
                    rects,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN + rects.len() * 8];
                    write_reply_header(
                        byte_order,
                        &mut out,
                        sequence,
                        u32::try_from(rects.len() * 2).unwrap_or(0),
                    );
                    out[1] = ordering;
                    put_u32(
                        byte_order,
                        &mut out[8..12],
                        u32::try_from(rects.len()).unwrap_or(0),
                    );
                    let mut offset = X_CLIENT_OUTPUT_RECORD_LEN;
                    for rect in rects {
                        put_i16(byte_order, &mut out[offset..offset + 2], rect.x as i16);
                        put_i16(byte_order, &mut out[offset + 2..offset + 4], rect.y as i16);
                        put_u16(byte_order, &mut out[offset + 4..offset + 6], rect.width as u16);
                        put_u16(
                            byte_order,
                            &mut out[offset + 6..offset + 8],
                            rect.height as u16,
                        );
                        offset += 8;
                    }
                    out
                }
                XClientReply::XfixesFetchRegion {
                    sequence,
                    extents,
                    rects,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN + rects.len() * 8];
                    write_reply_header(
                        byte_order,
                        &mut out,
                        sequence,
                        u32::try_from(rects.len() * 2).unwrap_or(0),
                    );
                    // The reply carries the extents at bytes 8 through 16 and
                    // no count at all: a client derives the number of
                    // rectangles from the reply's length. Writing a count here
                    // put it where the extents' x belongs, so a client read
                    // the rectangle count as a coordinate and zero for the
                    // rest of the bounding box.
                    put_i16(byte_order, &mut out[8..10], extents.x as i16);
                    put_i16(byte_order, &mut out[10..12], extents.y as i16);
                    put_u16(byte_order, &mut out[12..14], extents.width as u16);
                    put_u16(byte_order, &mut out[14..16], extents.height as u16);
                    let mut offset = X_CLIENT_OUTPUT_RECORD_LEN;
                    for rect in rects {
                        put_i16(byte_order, &mut out[offset..offset + 2], rect.x as i16);
                        put_i16(byte_order, &mut out[offset + 2..offset + 4], rect.y as i16);
                        put_u16(byte_order, &mut out[offset + 4..offset + 6], rect.width as u16);
                        put_u16(
                            byte_order,
                            &mut out[offset + 6..offset + 8],
                            rect.height as u16,
                        );
                        offset += 8;
                    }
                    out
                }
                XClientReply::XCMiscGetVersion {
                    sequence,
                    major_version,
                    minor_version,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    put_u16(byte_order, &mut out[8..10], major_version);
                    put_u16(byte_order, &mut out[10..12], minor_version);
                    out
                }
                XClientReply::XCMiscGetXIDRange {
                    sequence,
                    start_id,
                    count,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    put_u32(byte_order, &mut out[8..12], start_id);
                    put_u32(byte_order, &mut out[12..16], count);
                    out
                }
                XClientReply::XCMiscGetXIDList { sequence, ids } => {
                    // The list follows the fixed reply, one word each, so the
                    // length field counts the identifiers exactly.
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN + ids.len() * 4];
                    let length = u32::try_from(ids.len()).unwrap_or(0);
                    write_reply_header(byte_order, &mut out, sequence, length);
                    put_u32(byte_order, &mut out[8..12], length);
                    for (index, id) in ids.iter().enumerate() {
                        let start = X_CLIENT_OUTPUT_RECORD_LEN + index * 4;
                        put_u32(byte_order, &mut out[start..start + 4], *id);
                    }
                    out
                }
                XClientReply::XF86VidModeQueryVersion {
                    sequence,
                    major_version,
                    minor_version,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    put_u16(byte_order, &mut out[8..10], major_version);
                    put_u16(byte_order, &mut out[10..12], minor_version);
                    out
                }
                XClientReply::XF86VidModeGetModeLine { sequence, timing } => {
                    // The version 2 reply is fifty-two bytes: thirty-two of
                    // header and body, then five more words. `privsize` is the
                    // last of them and stays zero -- private timing data is an
                    // XFree86 driver notion with nothing behind it here.
                    let mut out = vec![0; 52];
                    write_reply_header(byte_order, &mut out, sequence, 5);
                    put_u32(byte_order, &mut out[8..12], timing.clock_khz);
                    put_u16(byte_order, &mut out[12..14], timing.hdisplay);
                    put_u16(byte_order, &mut out[14..16], timing.hsync_start);
                    put_u16(byte_order, &mut out[16..18], timing.hsync_end);
                    put_u16(byte_order, &mut out[18..20], timing.htotal);
                    put_u16(byte_order, &mut out[20..22], timing.hskew);
                    put_u16(byte_order, &mut out[22..24], timing.vdisplay);
                    put_u16(byte_order, &mut out[24..26], timing.vsync_start);
                    put_u16(byte_order, &mut out[26..28], timing.vsync_end);
                    put_u16(byte_order, &mut out[28..30], timing.vtotal);
                    put_u32(byte_order, &mut out[32..36], timing.flags);
                    out
                }
                XClientReply::ShmCreateSegment { sequence } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    // `nfd`, as `Dri3Open` above: one descriptor accompanies
                    // this reply.
                    out[1] = 1;
                    out
                }
                XClientReply::Dri3GetSupportedModifiers {
                    sequence,
                    window_modifiers,
                    screen_modifiers,
                } => {
                    let modifier_count = window_modifiers
                        .len()
                        .saturating_add(screen_modifiers.len());
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN + modifier_count.saturating_mul(8)];
                    write_reply_header(
                        byte_order,
                        &mut out,
                        sequence,
                        u32::try_from(modifier_count.saturating_mul(2)).unwrap_or(u32::MAX),
                    );
                    put_u32(
                        byte_order,
                        &mut out[8..12],
                        u32::try_from(window_modifiers.len()).unwrap_or(u32::MAX),
                    );
                    put_u32(
                        byte_order,
                        &mut out[12..16],
                        u32::try_from(screen_modifiers.len()).unwrap_or(u32::MAX),
                    );
                    let mut offset = X_CLIENT_OUTPUT_RECORD_LEN;
                    for modifier in window_modifiers.into_iter().chain(screen_modifiers) {
                        put_u64(byte_order, &mut out[offset..offset + 8], modifier);
                        offset += 8;
                    }
                    out
                }
                XClientReply::Dri3BufferFromPixmap {
                    sequence,
                    size_bytes,
                    width,
                    height,
                    stride,
                    depth,
                    bits_per_pixel,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    // One descriptor follows out of band.
                    out[1] = 1;
                    put_u32(byte_order, &mut out[8..12], size_bytes);
                    put_u16(byte_order, &mut out[12..14], width);
                    put_u16(byte_order, &mut out[14..16], height);
                    put_u16(byte_order, &mut out[16..18], stride);
                    out[18] = depth;
                    out[19] = bits_per_pixel;
                    out
                }
                XClientReply::Dri3BuffersFromPixmap {
                    sequence,
                    width,
                    height,
                    modifier,
                    depth,
                    bits_per_pixel,
                    strides,
                    offsets,
                } => {
                    // `nfd` is the promise the trailing lists must keep: the
                    // client reads exactly this many strides, offsets, and
                    // descriptors. Deriving it from the list itself is what
                    // keeps the three counts from disagreeing.
                    let plane_count = strides.len().min(offsets.len());
                    let mut out =
                        vec![0; X_CLIENT_OUTPUT_RECORD_LEN + plane_count.saturating_mul(8)];
                    write_reply_header(
                        byte_order,
                        &mut out,
                        sequence,
                        u32::try_from(plane_count.saturating_mul(2)).unwrap_or(u32::MAX),
                    );
                    out[1] = u8::try_from(plane_count).unwrap_or(u8::MAX);
                    put_u16(byte_order, &mut out[8..10], width);
                    put_u16(byte_order, &mut out[10..12], height);
                    put_u64(byte_order, &mut out[16..24], modifier);
                    out[24] = depth;
                    out[25] = bits_per_pixel;
                    let mut offset = X_CLIENT_OUTPUT_RECORD_LEN;
                    for stride in strides.into_iter().take(plane_count) {
                        put_u32(byte_order, &mut out[offset..offset + 4], stride);
                        offset += 4;
                    }
                    for plane_offset in offsets.into_iter().take(plane_count) {
                        put_u32(byte_order, &mut out[offset..offset + 4], plane_offset);
                        offset += 4;
                    }
                    out
                }
                XClientReply::PresentQueryCapabilities {
                    sequence,
                    capabilities,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    put_u32(byte_order, &mut out[8..12], capabilities);
                    out
                }
        _ => unreachable!("reply family checked before encoding"),
    })
}
