fn decode_put_image(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_PUT_IMAGE, X_PUT_IMAGE_REQ_LEN, bytes.len())?;
    validate_wire_image_format(bytes[1])?;
    let data_len = bytes.len() - X_PUT_IMAGE_REQ_LEN;
    if data_len > X_PUT_IMAGE_MAX_DATA_BYTES {
        return Err(XWireParseError::PropertyValueTooLarge {
            len: data_len,
            max: X_PUT_IMAGE_MAX_DATA_BYTES,
        });
    }

    Ok(XWireRequest::PutImage {
        format: bytes[1],
        drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        width: context.byte_order.u16(&bytes[12..14]),
        height: context.byte_order.u16(&bytes[14..16]),
        dst_x: context.byte_order.i16(&bytes[16..18]),
        dst_y: context.byte_order.i16(&bytes[18..20]),
        left_pad: bytes[20],
        depth: bytes[21],
        data: bytes[X_PUT_IMAGE_REQ_LEN..].to_vec(),
    })
}

fn decode_get_image(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_GET_IMAGE, X_GET_IMAGE_REQ_LEN, bytes.len())?;
    validate_wire_get_image_format(bytes[1])?;
    Ok(XWireRequest::GetImage {
        format: bytes[1],
        drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        x: context.byte_order.i16(&bytes[8..10]),
        y: context.byte_order.i16(&bytes[10..12]),
        width: context.byte_order.u16(&bytes[12..14]),
        height: context.byte_order.u16(&bytes[14..16]),
        plane_mask: context.byte_order.u32(&bytes[16..20]),
    })
}

/// Walk a `PolyText8` or `PolyText16` item list.
///
/// The two requests differ only in how wide a character is. An item's length
/// byte counts *characters*, not bytes, so a 16-bit item occupies
/// `2 + 2 * len`. A length of 255 is not a string at all but a font shift,
/// five bytes whose font identifier is most significant byte first regardless
/// of the client's own byte order -- the server reassembles it that way
/// explicitly and never swaps it (`dix/dixfonts.c:1206-1209`), and neither do
/// the CHAR2B values themselves.
fn decode_poly_text_items(
    opcode: u8,
    header_len: usize,
    max_bytes: usize,
    bytes: &[u8],
    char_width: usize,
) -> Result<Vec<XPolyTextItem>, XWireParseError> {
    let item_bytes = &bytes[header_len..];
    if item_bytes.len() > max_bytes {
        return Err(XWireParseError::PropertyValueTooLarge {
            len: item_bytes.len(),
            max: max_bytes,
        });
    }

    let mut offset = 0usize;
    let mut items = Vec::new();
    while offset < item_bytes.len() {
        let remaining = item_bytes.len().saturating_sub(offset);
        // A request is padded to four bytes, so up to three zero bytes may
        // trail the last item. They are padding, not a zero-length item.
        if remaining <= 3 && item_bytes[offset..].iter().all(|byte| *byte == 0) {
            break;
        }
        let len = item_bytes[offset];
        if len == u8::MAX {
            if remaining < 5 {
                return Err(XWireParseError::InvalidLength {
                    opcode,
                    expected_at_least: header_len + offset + 5,
                    actual: bytes.len(),
                });
            }
            items.push(XPolyTextItem::Font {
                font: XResourceId::new(
                    u64::from(u32::from_be_bytes(
                        item_bytes[offset + 1..offset + 5]
                            .try_into()
                            .expect("validated font item width"),
                    )),
                    1,
                ),
            });
            offset += 5;
            continue;
        }

        let item_len = 2usize.saturating_add(usize::from(len).saturating_mul(char_width));
        if remaining < item_len {
            return Err(XWireParseError::InvalidLength {
                opcode,
                expected_at_least: header_len + offset + item_len,
                actual: bytes.len(),
            });
        }
        let payload = &item_bytes[offset + 2..offset + item_len];
        let chars = if char_width == 1 {
            payload.iter().map(|byte| u16::from(*byte)).collect()
        } else {
            payload
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect()
        };
        items.push(XPolyTextItem::Text {
            delta: item_bytes[offset + 1] as i8,
            chars,
        });
        offset += item_len;
    }
    Ok(items)
}

fn decode_poly_text8(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_POLY_TEXT8, X_POLY_TEXT8_REQ_LEN, bytes.len())?;
    let items = decode_poly_text_items(
        X_POLY_TEXT8,
        X_POLY_TEXT8_REQ_LEN,
        X_POLY_TEXT8_MAX_BYTES,
        bytes,
        1,
    )?;
    Ok(XWireRequest::PolyText8 {
        drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        x: context.byte_order.i16(&bytes[12..14]),
        y: context.byte_order.i16(&bytes[14..16]),
        items,
    })
}

fn decode_poly_text16(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_POLY_TEXT16, X_POLY_TEXT16_REQ_LEN, bytes.len())?;
    let items = decode_poly_text_items(
        X_POLY_TEXT16,
        X_POLY_TEXT16_REQ_LEN,
        X_POLY_TEXT16_MAX_BYTES,
        bytes,
        2,
    )?;
    Ok(XWireRequest::PolyText16 {
        drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        x: context.byte_order.i16(&bytes[12..14]),
        y: context.byte_order.i16(&bytes[14..16]),
        items,
    })
}

fn decode_image_text8(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_IMAGE_TEXT8, X_IMAGE_TEXT8_REQ_LEN, bytes.len())?;
    let text_len = usize::from(bytes[1]);
    if text_len > X_IMAGE_TEXT8_MAX_BYTES {
        return Err(XWireParseError::PropertyValueTooLarge {
            len: text_len,
            max: X_IMAGE_TEXT8_MAX_BYTES,
        });
    }
    let expected_len = X_IMAGE_TEXT8_REQ_LEN + padded_len(text_len);
    if bytes.len() != expected_len {
        return Err(XWireParseError::InvalidLength {
            opcode: X_IMAGE_TEXT8,
            expected_at_least: expected_len,
            actual: bytes.len(),
        });
    }

    Ok(XWireRequest::ImageText8 {
        drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        x: context.byte_order.i16(&bytes[12..14]),
        y: context.byte_order.i16(&bytes[14..16]),
        text: bytes[X_IMAGE_TEXT8_REQ_LEN..X_IMAGE_TEXT8_REQ_LEN + text_len].to_vec(),
    })
}

fn decode_copy_area(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_COPY_AREA, X_COPY_AREA_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::CopyArea {
        source: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        destination: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[12..16])), 1),
        src_x: context.byte_order.i16(&bytes[16..18]),
        src_y: context.byte_order.i16(&bytes[18..20]),
        dst_x: context.byte_order.i16(&bytes[20..22]),
        dst_y: context.byte_order.i16(&bytes[22..24]),
        width: context.byte_order.u16(&bytes[24..26]),
        height: context.byte_order.u16(&bytes[26..28]),
    })
}

fn decode_poly_segment(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_POLY_SEGMENT, X_POLY_SEGMENT_REQ_LEN, bytes.len())?;
    let segment_bytes = &bytes[X_POLY_SEGMENT_REQ_LEN..];
    if !segment_bytes.len().is_multiple_of(8) {
        return Err(XWireParseError::InvalidLength {
            opcode: X_POLY_SEGMENT,
            expected_at_least: X_POLY_SEGMENT_REQ_LEN + ((segment_bytes.len() + 7) & !7),
            actual: bytes.len(),
        });
    }
    let mut segments = Vec::with_capacity(segment_bytes.len() / 8);
    for segment in segment_bytes.chunks_exact(8) {
        segments.push((
            XPoint {
                x: context.byte_order.i16(&segment[0..2]),
                y: context.byte_order.i16(&segment[2..4]),
            },
            XPoint {
                x: context.byte_order.i16(&segment[4..6]),
                y: context.byte_order.i16(&segment[6..8]),
            },
        ));
    }
    Ok(XWireRequest::PolySegment {
        drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        segments,
    })
}

fn decode_poly_line(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_POLY_LINE, X_POLY_LINE_REQ_LEN, bytes.len())?;
    validate_coordinate_mode(bytes[1])?;
    let point_bytes = &bytes[X_POLY_LINE_REQ_LEN..];
    if !point_bytes.len().is_multiple_of(4) {
        return Err(XWireParseError::InvalidLength {
            opcode: X_POLY_LINE,
            expected_at_least: X_POLY_LINE_REQ_LEN + ((point_bytes.len() + 3) & !3),
            actual: bytes.len(),
        });
    }

    let mut points = Vec::with_capacity(point_bytes.len() / 4);
    let mut previous = XPoint { x: 0, y: 0 };
    for point in point_bytes.chunks_exact(4) {
        let mut decoded = XPoint {
            x: context.byte_order.i16(&point[0..2]),
            y: context.byte_order.i16(&point[2..4]),
        };
        if bytes[1] == 1 && !points.is_empty() {
            decoded.x = previous.x.saturating_add(decoded.x);
            decoded.y = previous.y.saturating_add(decoded.y);
        }
        previous = decoded;
        points.push(decoded);
    }
    Ok(XWireRequest::PolyLine {
        drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        points,
    })
}

fn decode_fill_poly(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_FILL_POLY, X_FILL_POLY_REQ_LEN, bytes.len())?;
    // Shape is {Complex, Nonconvex, Convex}; anything else is a bad value,
    // named in the error, before the coordinate mode is judged.
    if bytes[12] > 2 {
        return Err(XWireParseError::InvalidValue(u32::from(bytes[12])));
    }
    validate_coordinate_mode(bytes[13])?;
    let point_bytes = &bytes[X_FILL_POLY_REQ_LEN..];
    if !point_bytes.len().is_multiple_of(4) {
        return Err(XWireParseError::InvalidLength {
            opcode: X_FILL_POLY,
            expected_at_least: X_FILL_POLY_REQ_LEN + ((point_bytes.len() + 3) & !3),
            actual: bytes.len(),
        });
    }

    Ok(XWireRequest::FillPoly {
        drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        shape: bytes[12],
        coordinate_mode: bytes[13],
        points: decode_points(context, point_bytes),
    })
}

/// A coordinate mode is `Origin` or `Previous`; the protocol lists Value
/// among the errors of every request that carries one.
fn validate_coordinate_mode(mode: u8) -> Result<(), XWireParseError> {
    if mode > 1 {
        return Err(XWireParseError::InvalidValue(u32::from(mode)));
    }
    Ok(())
}

/// Read a list of sixteen-bit coordinate pairs.
fn decode_points(context: XWireClientContext, point_bytes: &[u8]) -> Vec<XPoint> {
    point_bytes
        .chunks_exact(4)
        .map(|point| XPoint {
            x: context.byte_order.i16(&point[0..2]),
            y: context.byte_order.i16(&point[2..4]),
        })
        .collect()
}

/// Resolve `CoordModePrevious` into absolute points.
///
/// The first point is always absolute whichever mode is asked for; only the
/// ones after it are relative. Getting that wrong offsets an entire shape by
/// its own first vertex.
pub(crate) fn absolute_points(points: &[XPoint], coordinate_mode: u8) -> Vec<XPoint> {
    if coordinate_mode == 0 {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(points.len());
    let mut pen = XPoint { x: 0, y: 0 };
    for (index, point) in points.iter().enumerate() {
        pen = if index == 0 {
            *point
        } else {
            XPoint {
                x: pen.x.saturating_add(point.x),
                y: pen.y.saturating_add(point.y),
            }
        };
        out.push(pen);
    }
    out
}

fn decode_poly_fill_rectangle(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(
        X_POLY_FILL_RECTANGLE,
        X_POLY_FILL_RECTANGLE_REQ_LEN,
        bytes.len(),
    )?;
    let rectangle_bytes = &bytes[X_POLY_FILL_RECTANGLE_REQ_LEN..];
    if !rectangle_bytes.len().is_multiple_of(8) {
        return Err(XWireParseError::InvalidLength {
            opcode: X_POLY_FILL_RECTANGLE,
            expected_at_least: X_POLY_FILL_RECTANGLE_REQ_LEN + ((rectangle_bytes.len() + 7) & !7),
            actual: bytes.len(),
        });
    }
    let mut rectangles = Vec::with_capacity(rectangle_bytes.len() / 8);
    for rectangle in rectangle_bytes.chunks_exact(8) {
        rectangles.push(Rect {
            x: i32::from(context.byte_order.i16(&rectangle[0..2])),
            y: i32::from(context.byte_order.i16(&rectangle[2..4])),
            width: i32::from(context.byte_order.u16(&rectangle[4..6])),
            height: i32::from(context.byte_order.u16(&rectangle[6..8])),
        });
    }
    Ok(XWireRequest::PolyFillRectangle {
        drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        rectangles,
    })
}

fn decode_poly_rectangle(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_POLY_RECTANGLE, X_POLY_RECTANGLE_REQ_LEN, bytes.len())?;
    let rectangle_bytes = &bytes[X_POLY_RECTANGLE_REQ_LEN..];
    if !rectangle_bytes.len().is_multiple_of(8) {
        return Err(XWireParseError::InvalidLength {
            opcode: X_POLY_RECTANGLE,
            expected_at_least: X_POLY_RECTANGLE_REQ_LEN + ((rectangle_bytes.len() + 7) & !7),
            actual: bytes.len(),
        });
    }
    let mut rectangles = Vec::with_capacity(rectangle_bytes.len() / 8);
    for rectangle in rectangle_bytes.chunks_exact(8) {
        rectangles.push(Rect {
            x: i32::from(context.byte_order.i16(&rectangle[0..2])),
            y: i32::from(context.byte_order.i16(&rectangle[2..4])),
            width: i32::from(context.byte_order.u16(&rectangle[4..6])),
            height: i32::from(context.byte_order.u16(&rectangle[6..8])),
        });
    }
    Ok(XWireRequest::PolyRectangle {
        drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        rectangles,
    })
}

fn decode_poly_fill_arc(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_POLY_FILL_ARC, X_POLY_FILL_ARC_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::PolyFillArc {
        drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        arcs: decode_arcs(context, X_POLY_FILL_ARC, X_POLY_FILL_ARC_REQ_LEN, bytes)?,
    })
}

fn decode_poly_arc(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_POLY_ARC, X_POLY_ARC_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::PolyArc {
        drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        arcs: decode_arcs(context, X_POLY_ARC, X_POLY_ARC_REQ_LEN, bytes)?,
    })
}

fn decode_poly_point(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_POLY_POINT, X_POLY_POINT_REQ_LEN, bytes.len())?;
    validate_coordinate_mode(bytes[1])?;
    let point_bytes = &bytes[X_POLY_POINT_REQ_LEN..];
    if !point_bytes.len().is_multiple_of(4) {
        return Err(XWireParseError::InvalidLength {
            opcode: X_POLY_POINT,
            expected_at_least: X_POLY_POINT_REQ_LEN + point_bytes.len().div_ceil(4) * 4,
            actual: bytes.len(),
        });
    }
    Ok(XWireRequest::PolyPoint {
        drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        coordinate_mode: bytes[1],
        points: decode_points(context, point_bytes),
    })
}

/// Read a list of twelve-byte arcs.
fn decode_arcs(
    context: XWireClientContext,
    opcode: u8,
    header_len: usize,
    bytes: &[u8],
) -> Result<Vec<crate::XArc>, XWireParseError> {
    let arc_bytes = &bytes[header_len..];
    if !arc_bytes.len().is_multiple_of(12) {
        return Err(XWireParseError::InvalidLength {
            opcode,
            expected_at_least: header_len + arc_bytes.len().div_ceil(12) * 12,
            actual: bytes.len(),
        });
    }
    Ok(arc_bytes
        .chunks_exact(12)
        .map(|arc| crate::XArc {
            x: context.byte_order.i16(&arc[0..2]),
            y: context.byte_order.i16(&arc[2..4]),
            width: context.byte_order.u16(&arc[4..6]),
            height: context.byte_order.u16(&arc[6..8]),
            angle1: context.byte_order.i16(&arc[8..10]),
            angle2: context.byte_order.i16(&arc[10..12]),
        })
        .collect())
}

fn decode_image_text16(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_IMAGE_TEXT16, X_IMAGE_TEXT16_REQ_LEN, bytes.len())?;
    // The header's spare byte carries the count, and for the 16-bit request it
    // counts characters rather than bytes.
    let char_count = usize::from(bytes[1]);
    let text_len = char_count.saturating_mul(2);
    if text_len > X_IMAGE_TEXT16_MAX_BYTES {
        return Err(XWireParseError::PropertyValueTooLarge {
            len: text_len,
            max: X_IMAGE_TEXT16_MAX_BYTES,
        });
    }
    let expected_len = X_IMAGE_TEXT16_REQ_LEN + padded_len(text_len);
    if bytes.len() != expected_len {
        return Err(XWireParseError::InvalidLength {
            opcode: X_IMAGE_TEXT16,
            expected_at_least: expected_len,
            actual: bytes.len(),
        });
    }
    let chars = bytes[X_IMAGE_TEXT16_REQ_LEN..X_IMAGE_TEXT16_REQ_LEN + text_len]
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();

    Ok(XWireRequest::ImageText16 {
        drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        x: context.byte_order.i16(&bytes[12..14]),
        y: context.byte_order.i16(&bytes[14..16]),
        chars,
    })
}

/// `QueryTextExtents` carries no character count.
///
/// Its length is recovered from the request length, which cannot distinguish
/// one trailing character from two -- so the header's spare byte carries an
/// odd-length flag instead (`dix/dispatch.c:1411-1418`).
fn decode_query_text_extents(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(
        X_QUERY_TEXT_EXTENTS,
        X_QUERY_TEXT_EXTENTS_REQ_LEN,
        bytes.len(),
    )?;
    let payload = bytes.len().saturating_sub(X_QUERY_TEXT_EXTENTS_REQ_LEN);
    if !payload.is_multiple_of(4) {
        return Err(XWireParseError::InvalidLength {
            opcode: X_QUERY_TEXT_EXTENTS,
            expected_at_least: X_QUERY_TEXT_EXTENTS_REQ_LEN,
            actual: bytes.len(),
        });
    }
    let mut char_count = payload / 2;
    if bytes[1] != 0 {
        // An odd string leaves one character of padding to discard.
        if char_count == 0 {
            return Err(XWireParseError::InvalidLength {
                opcode: X_QUERY_TEXT_EXTENTS,
                expected_at_least: X_QUERY_TEXT_EXTENTS_REQ_LEN + 4,
                actual: bytes.len(),
            });
        }
        char_count -= 1;
    }
    if char_count > X_QUERY_TEXT_EXTENTS_MAX_CHARS {
        return Err(XWireParseError::PropertyValueTooLarge {
            len: char_count,
            max: X_QUERY_TEXT_EXTENTS_MAX_CHARS,
        });
    }
    let chars = bytes
        [X_QUERY_TEXT_EXTENTS_REQ_LEN..X_QUERY_TEXT_EXTENTS_REQ_LEN + char_count * 2]
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    Ok(XWireRequest::QueryTextExtents {
        fontable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        chars,
    })
}
