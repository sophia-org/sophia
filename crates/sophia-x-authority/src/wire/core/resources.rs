fn decode_free_gc(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_FREE_GC, X_FREE_GC_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::FreeGraphicsContext {
        gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
    })
}

fn decode_create_pixmap(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_CREATE_PIXMAP, X_CREATE_PIXMAP_REQ_LEN, bytes.len())?;
    let pixmap = context.byte_order.u32(&bytes[4..8]);
    context.validate_new_resource_id(pixmap)?;
    Ok(XWireRequest::CreatePixmap {
        depth: bytes[1],
        pixmap: XResourceId::new(u64::from(pixmap), 1),
        drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        width: context.byte_order.u16(&bytes[12..14]),
        height: context.byte_order.u16(&bytes[14..16]),
    })
}

fn decode_open_font(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_OPEN_FONT, X_OPEN_FONT_REQ_LEN, bytes.len())?;
    let name_len = usize::from(context.byte_order.u16(&bytes[8..10]));
    let expected_len = X_OPEN_FONT_REQ_LEN + padded_len(name_len);
    if bytes.len() != expected_len {
        return Err(XWireParseError::InvalidLength {
            opcode: X_OPEN_FONT,
            expected_at_least: expected_len,
            actual: bytes.len(),
        });
    }
    let name = core::str::from_utf8(&bytes[X_OPEN_FONT_REQ_LEN..X_OPEN_FONT_REQ_LEN + name_len])
        .map_err(|_| XWireParseError::InvalidLength {
            opcode: X_OPEN_FONT,
            expected_at_least: expected_len,
            actual: bytes.len(),
        })?;
    let font = context.byte_order.u32(&bytes[4..8]);
    context.validate_new_resource_id(font)?;
    Ok(XWireRequest::OpenFont {
        font: XResourceId::new(u64::from(font), 1),
        name: name.to_owned(),
    })
}

fn decode_close_font(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_CLOSE_FONT, X_CLOSE_FONT_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::CloseFont {
        font: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
    })
}

fn decode_query_font(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_QUERY_FONT, X_QUERY_FONT_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::QueryFont {
        font: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
    })
}

fn decode_list_fonts(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_LIST_FONTS, X_LIST_FONTS_REQ_LEN, bytes.len())?;
    let pattern_len = usize::from(context.byte_order.u16(&bytes[6..8]));
    let expected_len = X_LIST_FONTS_REQ_LEN + padded_len(pattern_len);
    if bytes.len() != expected_len {
        return Err(XWireParseError::InvalidLength {
            opcode: X_LIST_FONTS,
            expected_at_least: expected_len,
            actual: bytes.len(),
        });
    }
    let pattern =
        core::str::from_utf8(&bytes[X_LIST_FONTS_REQ_LEN..X_LIST_FONTS_REQ_LEN + pattern_len])
            .map_err(|_| XWireParseError::InvalidLength {
                opcode: X_LIST_FONTS,
                expected_at_least: expected_len,
                actual: bytes.len(),
            })?;
    Ok(XWireRequest::ListFonts {
        max_names: context.byte_order.u16(&bytes[4..6]),
        pattern: pattern.to_owned(),
    })
}

fn decode_list_fonts_with_info(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(
        X_LIST_FONTS_WITH_INFO,
        X_LIST_FONTS_WITH_INFO_REQ_LEN,
        bytes.len(),
    )?;
    let pattern_len = usize::from(context.byte_order.u16(&bytes[6..8]));
    let expected_len = X_LIST_FONTS_WITH_INFO_REQ_LEN + padded_len(pattern_len);
    if bytes.len() != expected_len {
        return Err(XWireParseError::InvalidLength {
            opcode: X_LIST_FONTS_WITH_INFO,
            expected_at_least: expected_len,
            actual: bytes.len(),
        });
    }
    let pattern = core::str::from_utf8(
        &bytes[X_LIST_FONTS_WITH_INFO_REQ_LEN..X_LIST_FONTS_WITH_INFO_REQ_LEN + pattern_len],
    )
    .map_err(|_| XWireParseError::InvalidLength {
        opcode: X_LIST_FONTS_WITH_INFO,
        expected_at_least: expected_len,
        actual: bytes.len(),
    })?;
    Ok(XWireRequest::ListFontsWithInfo {
        max_names: context.byte_order.u16(&bytes[4..6]),
        pattern: pattern.to_owned(),
    })
}

fn decode_free_pixmap(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_FREE_PIXMAP, X_FREE_PIXMAP_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::FreePixmap {
        pixmap: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
    })
}

fn decode_query_best_size(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_QUERY_BEST_SIZE, X_QUERY_BEST_SIZE_REQ_LEN, bytes.len())?;
    // Class is {Cursor, Tile, Stipple}, judged before the drawable is looked
    // up, as the server's own order has it.
    if bytes[1] > 2 {
        return Err(XWireParseError::InvalidValue(u32::from(bytes[1])));
    }
    Ok(XWireRequest::QueryBestSize {
        class: bytes[1],
        drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        width: context.byte_order.u16(&bytes[8..10]),
        height: context.byte_order.u16(&bytes[10..12]),
    })
}

fn decode_create_gc(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_CREATE_GC, X_CREATE_GC_REQ_LEN, bytes.len())?;
    let gc = context.byte_order.u32(&bytes[4..8]);
    context.validate_new_resource_id(gc)?;
    let value_mask = context.byte_order.u32(&bytes[12..16]);
    // A set bit above the twenty-three components is a Value error carrying
    // the mask, as ChangeGC and CopyGC already answer it, not a length one:
    // the length the extra bit announces is a consequence, not the fault.
    if value_mask & !X_GC_VALUE_MASK != 0 {
        return Err(XWireParseError::InvalidValue(value_mask));
    }
    let value_count = usize::try_from(value_mask.count_ones()).unwrap_or(usize::MAX);
    let expected_len = X_CREATE_GC_REQ_LEN.saturating_add(value_count.saturating_mul(4));
    if bytes.len() != expected_len {
        return Err(XWireParseError::InvalidLength {
            opcode: X_CREATE_GC,
            expected_at_least: expected_len,
            actual: bytes.len(),
        });
    }
    let mut values = XGraphicsContextValues::default();
    let mut cursor = X_CREATE_GC_REQ_LEN;
    let mut next_value = || {
        let value = context.byte_order.u32(&bytes[cursor..cursor + 4]);
        cursor += 4;
        value
    };
    for bit in 0..23 {
        if value_mask & (1 << bit) == 0 {
            continue;
        }
        let value = next_value();
        decode_gc_value(bit, value, &mut values)?;
    }
    Ok(XWireRequest::CreateGraphicsContext {
        gc: XResourceId::new(u64::from(gc), 1),
        drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        values,
    })
}

/// Store one graphics-context value by its mask bit, refusing anything
/// outside the range the protocol gives that component. The refused value
/// travels in the error, as Value errors carry it.
fn decode_gc_value(
    bit: u32,
    value: u32,
    values: &mut XGraphicsContextValues,
) -> Result<(), XWireParseError> {
    let bad = || XWireParseError::InvalidValue(value);
    let bounded = |max: u32| -> Result<u8, XWireParseError> {
        if value > max {
            return Err(bad());
        }
        Ok(u8::try_from(value).unwrap_or(u8::MAX))
    };
    match bit {
        0 => values.function = bounded(15)?,
        1 => values.plane_mask = value,
        2 => values.foreground = value,
        3 => values.background = value,
        4 => values.line_width = u16::try_from(value).map_err(|_| bad())?,
        5 => values.line_style = bounded(2)?,
        6 => values.cap_style = bounded(3)?,
        7 => values.join_style = bounded(2)?,
        8 => values.fill_style = bounded(3)?,
        9 => values.fill_rule = bounded(1)?,
        10 => values.tile = (value != 0).then(|| XResourceId::new(u64::from(value), 1)),
        11 => values.stipple = (value != 0).then(|| XResourceId::new(u64::from(value), 1)),
        12 => values.tile_stipple_x_origin = value as i16,
        13 => values.tile_stipple_y_origin = value as i16,
        14 => values.font = (value != 0).then(|| XResourceId::new(u64::from(value), 1)),
        15 => values.subwindow_mode = bounded(1)?,
        16 => values.graphics_exposures = bounded(1)? != 0,
        17 => values.clip_x_origin = value as i16,
        18 => values.clip_y_origin = value as i16,
        19 => values.clip_mask = (value != 0).then(|| XResourceId::new(u64::from(value), 1)),
        20 => values.dash_offset = u16::try_from(value).map_err(|_| bad())?,
        // One value sets every dash to the same length, which is the
        // shorthand the protocol defines; SetDashes carries a pattern. A
        // zero-length dash would never advance, so it is refused here too.
        21 => {
            let length = u8::try_from(value).map_err(|_| bad())?;
            if length == 0 {
                return Err(bad());
            }
            values.dashes = vec![length, length];
        }
        22 => values.arc_mode = bounded(1)?,
        _ => {}
    }
    Ok(())
}

fn decode_change_gc(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_CHANGE_GC, X_CHANGE_GC_REQ_LEN, bytes.len())?;
    let value_mask = context.byte_order.u32(&bytes[8..12]);
    if value_mask & !0x007f_ffff != 0 {
        return Err(XWireParseError::InvalidValue(value_mask));
    }
    let value_count = usize::try_from(value_mask.count_ones()).unwrap_or(usize::MAX);
    let expected_len = X_CHANGE_GC_REQ_LEN.saturating_add(value_count.saturating_mul(4));
    if bytes.len() != expected_len {
        return Err(XWireParseError::InvalidLength {
            opcode: X_CHANGE_GC,
            expected_at_least: expected_len,
            actual: bytes.len(),
        });
    }
    let mut values = XGraphicsContextValues::default();
    let mut cursor = X_CHANGE_GC_REQ_LEN;
    for bit in 0..23 {
        if value_mask & (1 << bit) == 0 {
            continue;
        }
        let value = context.byte_order.u32(&bytes[cursor..cursor + 4]);
        cursor += 4;
        decode_gc_value(bit, value, &mut values)?;
    }
    Ok(XWireRequest::ChangeGraphicsContext {
        gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        value_mask,
        values,
    })
}

