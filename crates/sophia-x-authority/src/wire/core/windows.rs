/// Reads a `BackPixmap` attribute value. Zero is None, one is ParentRelative
/// and anything else names a pixmap to tile the window with.
fn background_from_pixmap_value(value: u32) -> crate::XWindowBackground {
    match value {
        0 => crate::XWindowBackground::Undefined,
        1 => crate::XWindowBackground::ParentRelative,
        _ => crate::XWindowBackground::Pixmap(XResourceId::new(u64::from(value), 1)),
    }
}

fn decode_query_tree(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_QUERY_TREE, X_QUERY_TREE_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::QueryTree {
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
    })
}

fn decode_unmap_window(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_UNMAP_WINDOW, X_UNMAP_WINDOW_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::UnmapWindow {
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
    })
}

fn decode_configure_window(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_CONFIGURE_WINDOW, X_CONFIGURE_WINDOW_REQ_LEN, bytes.len())?;
    let value_mask = context.byte_order.u16(&bytes[8..10]);
    if value_mask & !X_CONFIGURE_WINDOW_VALUE_MASK != 0 {
        return Err(XWireParseError::InvalidValue(u32::from(value_mask)));
    }
    let value_count = usize::try_from(value_mask.count_ones()).unwrap_or(usize::MAX);
    let expected_len = X_CONFIGURE_WINDOW_REQ_LEN + value_count.saturating_mul(4);
    if bytes.len() != expected_len {
        return Err(XWireParseError::InvalidLength {
            opcode: X_CONFIGURE_WINDOW,
            expected_at_least: expected_len,
            actual: bytes.len(),
        });
    }
    let mut cursor = X_CONFIGURE_WINDOW_REQ_LEN;
    let mut next_value = || {
        let value = context.byte_order.u32(&bytes[cursor..cursor + 4]);
        cursor += 4;
        value
    };
    let x = (value_mask & 0x0001 != 0).then(|| next_value() as i16);
    let y = (value_mask & 0x0002 != 0).then(|| next_value() as i16);
    let width = (value_mask & 0x0004 != 0).then(|| next_value() as u16);
    let height = (value_mask & 0x0008 != 0).then(|| next_value() as u16);
    let border_width = (value_mask & 0x0010 != 0).then(|| next_value() as u16);
    let sibling = (value_mask & 0x0020 != 0).then(|| XResourceId::new(u64::from(next_value()), 1));
    let stack_mode = (value_mask & 0x0040 != 0).then(|| next_value() as u8);

    Ok(XWireRequest::ConfigureWindow {
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        value_mask,
        x,
        y,
        width,
        height,
        border_width,
        sibling,
        stack_mode,
    })
}

fn decode_get_window_attributes(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(
        X_GET_WINDOW_ATTRIBUTES,
        X_GET_WINDOW_ATTRIBUTES_REQ_LEN,
        bytes.len(),
    )?;
    Ok(XWireRequest::GetWindowAttributes {
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
    })
}

fn decode_warp_pointer(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_WARP_POINTER, X_WARP_POINTER_REQ_LEN, bytes.len())?;
    // Zero is `None` for either window and stays zero here: which of the two
    // is absent changes what the request means, so the decision belongs to
    // the dispatcher that can act on it, not to a resource id minted early.
    Ok(XWireRequest::WarpPointer {
        source: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        destination: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        src_x: context.byte_order.i16(&bytes[12..14]),
        src_y: context.byte_order.i16(&bytes[14..16]),
        src_width: context.byte_order.u16(&bytes[16..18]),
        src_height: context.byte_order.u16(&bytes[18..20]),
        dst_x: context.byte_order.i16(&bytes[20..22]),
        dst_y: context.byte_order.i16(&bytes[22..24]),
    })
}

fn decode_translate_coordinates(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(
        X_TRANSLATE_COORDINATES,
        X_TRANSLATE_COORDINATES_REQ_LEN,
        bytes.len(),
    )?;
    Ok(XWireRequest::TranslateCoordinates {
        source: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        destination: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        src_x: context.byte_order.i16(&bytes[12..14]),
        src_y: context.byte_order.i16(&bytes[14..16]),
    })
}

fn decode_get_geometry(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_GET_GEOMETRY, X_GET_GEOMETRY_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::GetGeometry {
        drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
    })
}

fn decode_clear_area(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_CLEAR_AREA, X_CLEAR_AREA_REQ_LEN, bytes.len())?;
    // Exposures is a BOOL, and the protocol lists Value among ClearArea's
    // errors: a client that sends 2 is told which value was refused.
    if bytes[1] > 1 {
        return Err(XWireParseError::InvalidValue(u32::from(bytes[1])));
    }
    Ok(XWireRequest::ClearArea {
        exposures: bytes[1] != 0,
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        x: context.byte_order.i16(&bytes[8..10]),
        y: context.byte_order.i16(&bytes[10..12]),
        width: context.byte_order.u16(&bytes[12..14]),
        height: context.byte_order.u16(&bytes[14..16]),
    })
}

fn decode_destroy_window(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_DESTROY_WINDOW, X_DESTROY_WINDOW_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::DestroyWindow {
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
    })
}

fn decode_destroy_subwindows(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(
        X_DESTROY_SUBWINDOWS,
        X_DESTROY_SUBWINDOWS_REQ_LEN,
        bytes.len(),
    )?;
    Ok(XWireRequest::DestroySubwindows {
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
    })
}

fn decode_change_window_attributes(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(
        X_CHANGE_WINDOW_ATTRIBUTES,
        X_CHANGE_WINDOW_ATTRIBUTES_REQ_LEN,
        bytes.len(),
    )?;
    let value_mask = context.byte_order.u32(&bytes[8..12]);
    if value_mask & !X_CREATE_WINDOW_VALUE_MASK != 0 {
        return Err(XWireParseError::InvalidValue(value_mask));
    }
    let value_count = usize::try_from(value_mask.count_ones()).unwrap_or(usize::MAX);
    let expected_len =
        X_CHANGE_WINDOW_ATTRIBUTES_REQ_LEN.saturating_add(value_count.saturating_mul(4));
    if bytes.len() != expected_len {
        return Err(XWireParseError::InvalidLength {
            opcode: X_CHANGE_WINDOW_ATTRIBUTES,
            expected_at_least: expected_len,
            actual: bytes.len(),
        });
    }
    let mut event_mask = None;
    let mut do_not_propagate_mask = None;
    let mut override_redirect = None;
    let mut cursor = None;
    let mut colormap = None;
    let mut background_pixmap = None;
    let mut background_pixel = None;
    let mut value_cursor = X_CHANGE_WINDOW_ATTRIBUTES_REQ_LEN;
    let mut bit_gravity = None;
    let mut border_pixmap = None;
    let mut border_pixel = None;
    let mut win_gravity = None;
    for bit in 0..15 {
        if value_mask & (1 << bit) == 0 {
            continue;
        }
        let value = context
            .byte_order
            .u32(&bytes[value_cursor..value_cursor + 4]);
        value_cursor += 4;
        match bit {
            0 => background_pixmap = Some(background_from_pixmap_value(value)),
            1 => background_pixel = Some(value),
            2 => border_pixmap = Some(value),
            3 => border_pixel = Some(value),
            9 => override_redirect = Some(value != 0),
            11 => event_mask = Some(value),
            4 => bit_gravity = Some(gravity_value(value)?),
            5 => win_gravity = Some(gravity_value(value)?),
            12 => do_not_propagate_mask = Some(value),
            13 => colormap = Some(value),
            14 => cursor = Some(value),
            _ => {}
        }
    }
    Ok(XWireRequest::ChangeWindowAttributes {
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        background_pixmap,
        background_pixel,
        override_redirect,
        event_mask,
        do_not_propagate_mask,
        cursor,
        colormap,
        bit_gravity,
        win_gravity,
        border_pixmap,
        border_pixel,
    })
}

fn decode_create_window(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_CREATE_WINDOW, X_CREATE_WINDOW_REQ_LEN, bytes.len())?;
    let value_mask = context.byte_order.u32(&bytes[28..32]);
    // A set bit the protocol does not define is a Value error carrying the
    // mask, before the length is judged by it: the extra value it would
    // announce is not a framing fault but a request nobody can mean.
    if value_mask & !X_CREATE_WINDOW_VALUE_MASK != 0 {
        return Err(XWireParseError::InvalidValue(value_mask));
    }
    let value_count = usize::try_from(value_mask.count_ones()).unwrap_or(usize::MAX);
    let expected_len = X_CREATE_WINDOW_REQ_LEN.saturating_add(value_count.saturating_mul(4));
    if bytes.len() != expected_len {
        return Err(XWireParseError::InvalidLength {
            opcode: X_CREATE_WINDOW,
            expected_at_least: expected_len,
            actual: bytes.len(),
        });
    }
    let mut value_cursor = X_CREATE_WINDOW_REQ_LEN;
    let mut background_pixmap = None;
    let mut background_pixel = None;
    let mut event_mask = None;
    let mut do_not_propagate_mask = None;
    let mut colormap = None;
    let mut cursor = None;
    let mut override_redirect = false;
    let mut bit_gravity = None;
    let mut border_pixmap = None;
    let mut border_pixel = None;
    let mut win_gravity = None;
    for bit in 0..15 {
        if value_mask & (1 << bit) == 0 {
            continue;
        }
        let value = context
            .byte_order
            .u32(&bytes[value_cursor..value_cursor + 4]);
        value_cursor += 4;
        match bit {
            0 => background_pixmap = Some(background_from_pixmap_value(value)),
            1 => background_pixel = Some(value),
            2 => border_pixmap = Some(value),
            3 => border_pixel = Some(value),
            9 => override_redirect = value != 0,
            11 => event_mask = Some(value),
            4 => bit_gravity = Some(gravity_value(value)?),
            5 => win_gravity = Some(gravity_value(value)?),
            12 => do_not_propagate_mask = Some(value),
            13 => colormap = Some(XResourceId::new(u64::from(value), 1)),
            14 => cursor = Some(value),
            _ => {}
        }
    }
    let window_raw = context.byte_order.u32(&bytes[4..8]);
    context.validate_new_resource_id(window_raw)?;
    let window = XResourceId::new(u64::from(window_raw), 1);
    // Class is {CopyFromParent, InputOutput, InputOnly}. An InputOnly window
    // is recorded so that the drawing family can refuse it: it has no pixels.
    let border_width = context.byte_order.u16(&bytes[20..22]);
    let class = context.byte_order.u16(&bytes[22..24]);
    if class > 2 {
        return Err(XWireParseError::InvalidValue(u32::from(class)));
    }
    Ok(XWireRequest::CreateWindow {
        packet: XAuthorityRequestPacket {
            transaction: context.transaction,
            namespace: context.namespace,
            kind: XAuthorityRequestKind::CreateWindow {
                window,
                surface: SurfaceId::new(window_raw, 1),
                geometry: Rect {
                    x: i32::from(context.byte_order.i16(&bytes[12..14])),
                    y: i32::from(context.byte_order.i16(&bytes[14..16])),
                    width: i32::from(context.byte_order.u16(&bytes[16..18])),
                    height: i32::from(context.byte_order.u16(&bytes[18..20])),
                },
                constraints: SurfaceConstraints {
                    min_size: None,
                    max_size: None,
                },
                generation: 1,
            },
        },
        parent: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        depth: bytes[1],
        visual: context.byte_order.u32(&bytes[24..28]),
        colormap,
        background_pixmap,
        background_pixel,
        override_redirect,
        event_mask,
        do_not_propagate_mask,
        cursor,
        border_width,
        copy_class_from_parent: class == 0,
        input_only: class == 2,
        bit_gravity,
        win_gravity,
        border_pixmap,
        border_pixel,
    })
}

fn decode_map_window(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_MAP_WINDOW, X_MAP_WINDOW_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::Authority(XAuthorityRequestPacket {
        transaction: context.transaction,
        namespace: context.namespace,
        kind: XAuthorityRequestKind::MapWindow {
            window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
            generation: 2,
        },
    }))
}

fn decode_reparent_window(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_REPARENT_WINDOW, X_REPARENT_WINDOW_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::ReparentWindow {
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        parent: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
        x: context.byte_order.i16(&bytes[12..14]),
        y: context.byte_order.i16(&bytes[14..16]),
    })
}

fn decode_map_subwindows(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_MAP_SUBWINDOWS, X_MAP_SUBWINDOWS_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::MapSubwindows {
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        withheld: Vec::new(),
    })
}

/// A gravity value: Forget or Unmap (0) through Static (10); anything past
/// Static is a Value error.
fn gravity_value(value: u32) -> Result<u8, XWireParseError> {
    u8::try_from(value)
        .ok()
        .filter(|gravity| *gravity <= 10)
        .ok_or(XWireParseError::InvalidValue(value))
}
