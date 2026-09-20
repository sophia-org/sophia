fn decode_query_colors(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_QUERY_COLORS, X_QUERY_COLORS_REQ_LEN, bytes.len())?;
    let pixel_bytes = &bytes[X_QUERY_COLORS_REQ_LEN..];
    if !pixel_bytes.len().is_multiple_of(4) {
        return Err(XWireParseError::InvalidLength {
            opcode: X_QUERY_COLORS,
            expected_at_least: X_QUERY_COLORS_REQ_LEN + ((pixel_bytes.len() + 3) & !3),
            actual: bytes.len(),
        });
    }
    if pixel_bytes.len() / 4 > X_QUERY_COLORS_MAX_PIXELS {
        return Err(XWireParseError::PropertyValueTooLarge {
            len: pixel_bytes.len(),
            max: X_QUERY_COLORS_MAX_PIXELS * 4,
        });
    }

    Ok(XWireRequest::QueryColors {
        colormap: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        pixels: pixel_bytes
            .chunks_exact(4)
            .map(|pixel| context.byte_order.u32(pixel))
            .collect(),
    })
}

fn decode_create_colormap(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_CREATE_COLORMAP, X_CREATE_COLORMAP_REQ_LEN, bytes.len())?;
    let colormap = context.byte_order.u32(&bytes[4..8]);
    context.validate_new_resource_id(colormap)?;
    Ok(XWireRequest::CreateColormap {
        alloc: bytes[1],
        colormap: XResourceId::new(u64::from(colormap), 1),
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        visual: context.byte_order.u32(&bytes[12..16]),
    })
}

fn decode_named_color(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(
        bytes[0],
        X_ALLOC_NAMED_COLOR_REQ_LEN,
        bytes.len(),
    )?;
    let name_len = usize::from(context.byte_order.u16(&bytes[8..10]));
    if name_len > X_ALLOC_NAMED_COLOR_MAX_NAME_BYTES {
        return Err(XWireParseError::PropertyValueTooLarge {
            len: name_len,
            max: X_ALLOC_NAMED_COLOR_MAX_NAME_BYTES,
        });
    }
    let expected_len = X_ALLOC_NAMED_COLOR_REQ_LEN + padded_len(name_len);
    if bytes.len() != expected_len {
        return Err(XWireParseError::InvalidLength {
            opcode: bytes[0],
            expected_at_least: expected_len,
            actual: bytes.len(),
        });
    }
    // Core color names are STRING8 (Latin-1), not UTF-8. Unknown names
    // reach dispatch as BadName rather than becoming framing errors.
    let name = bytes[X_ALLOC_NAMED_COLOR_REQ_LEN..X_ALLOC_NAMED_COLOR_REQ_LEN + name_len]
        .iter().copied().map(char::from).collect();
    let colormap = XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1);
    Ok(if bytes[0] == X_LOOKUP_COLOR {
        XWireRequest::LookupColor { colormap, name }
    } else {
        XWireRequest::AllocNamedColor { colormap, name }
    })
}

fn decode_alloc_color(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_ALLOC_COLOR, X_ALLOC_COLOR_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::AllocColor {
        colormap: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        red: context.byte_order.u16(&bytes[8..10]),
        green: context.byte_order.u16(&bytes[10..12]),
        blue: context.byte_order.u16(&bytes[12..14]),
    })
}

fn decode_create_cursor(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_CREATE_CURSOR, X_CREATE_CURSOR_REQ_LEN, bytes.len())?;
    let cursor = context.byte_order.u32(&bytes[4..8]);
    context.validate_new_resource_id(cursor)?;
    let mask = context.byte_order.u32(&bytes[12..16]);
    Ok(XWireRequest::CreateCursor {
        cursor: XResourceId::new(u64::from(cursor), 1),
        source: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        mask: (mask != 0).then(|| XResourceId::new(u64::from(mask), 1)),
    })
}

fn decode_create_glyph_cursor(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(
        X_CREATE_GLYPH_CURSOR,
        X_CREATE_GLYPH_CURSOR_REQ_LEN,
        bytes.len(),
    )?;
    let cursor = context.byte_order.u32(&bytes[4..8]);
    context.validate_new_resource_id(cursor)?;
    let mask_font = context.byte_order.u32(&bytes[12..16]);
    Ok(XWireRequest::CreateGlyphCursor {
        cursor: XResourceId::new(u64::from(cursor), 1),
        source_font: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        mask_font: (mask_font != 0).then(|| XResourceId::new(u64::from(mask_font), 1)),
    })
}

fn decode_free_cursor(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_FREE_CURSOR, X_FREE_CURSOR_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::FreeCursor {
        cursor: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
    })
}

fn decode_recolor_cursor(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_RECOLOR_CURSOR, X_RECOLOR_CURSOR_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::RecolorCursor {
        cursor: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
    })
}

fn decode_set_clip_rectangles(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(
        X_SET_CLIP_RECTANGLES,
        X_SET_CLIP_RECTANGLES_REQ_LEN,
        bytes.len(),
    )?;
    // Ordering is {UnSorted, YSorted, YXSorted, YXBanded}. The list is
    // clipped the same way under all four, so only the range is judged.
    if bytes[1] > 3 {
        return Err(XWireParseError::InvalidValue(u32::from(bytes[1])));
    }
    let rectangle_bytes = &bytes[X_SET_CLIP_RECTANGLES_REQ_LEN..];
    if !rectangle_bytes.len().is_multiple_of(8) {
        return Err(XWireParseError::InvalidLength {
            opcode: X_SET_CLIP_RECTANGLES,
            expected_at_least: X_SET_CLIP_RECTANGLES_REQ_LEN + ((rectangle_bytes.len() + 7) & !7),
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
    Ok(XWireRequest::SetClipRectangles {
        clip_x_origin: context.byte_order.i16(&bytes[8..10]),
        clip_y_origin: context.byte_order.i16(&bytes[10..12]),
        gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        rectangles,
    })
}
