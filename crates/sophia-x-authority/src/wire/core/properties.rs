fn decode_get_property(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_GET_PROPERTY, X_GET_PROPERTY_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::GetProperty(XPropertyRead {
        delete: bytes[1] != 0,
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        property: context.byte_order.u32(&bytes[8..12]),
        property_type: context.byte_order.u32(&bytes[12..16]),
        long_offset: context.byte_order.u32(&bytes[16..20]),
        long_length: context.byte_order.u32(&bytes[20..24]),
    }))
}

fn decode_list_properties(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_LIST_PROPERTIES, X_LIST_PROPERTIES_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::ListProperties {
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
    })
}

fn decode_intern_atom(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_INTERN_ATOM, X_INTERN_ATOM_REQ_LEN, bytes.len())?;
    let name_len = usize::from(context.byte_order.u16(&bytes[4..6]));
    let expected_len = X_INTERN_ATOM_REQ_LEN + padded_len(name_len);
    if bytes.len() != expected_len {
        return Err(XWireParseError::InvalidLength {
            opcode: X_INTERN_ATOM,
            expected_at_least: expected_len,
            actual: bytes.len(),
        });
    }
    let name =
        core::str::from_utf8(&bytes[X_INTERN_ATOM_REQ_LEN..X_INTERN_ATOM_REQ_LEN + name_len])
            .map_err(|_| XWireParseError::InvalidLength {
                opcode: X_INTERN_ATOM,
                expected_at_least: expected_len,
                actual: bytes.len(),
            })?;
    Ok(XWireRequest::InternAtom {
        only_if_exists: bytes[1] != 0,
        name: name.to_owned(),
    })
}

fn decode_get_atom_name(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_GET_ATOM_NAME, X_GET_ATOM_NAME_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::GetAtomName {
        atom: context.byte_order.u32(&bytes[4..8]),
    })
}

fn decode_change_property(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_CHANGE_PROPERTY, X_CHANGE_PROPERTY_REQ_LEN, bytes.len())?;
    let mode = match bytes[1] {
        0 => XPropertyMode::Replace,
        1 => XPropertyMode::Prepend,
        2 => XPropertyMode::Append,
        other => return Err(XWireParseError::InvalidPropertyMode(other)),
    };
    let format = bytes[16];
    validate_wire_property_format(format)?;
    let units = context.byte_order.u32(&bytes[20..24]) as usize;
    let unit_width = usize::from(format / 8);
    let value_len =
        units
            .checked_mul(unit_width)
            .ok_or(XWireParseError::PropertyValueTooLarge {
                len: usize::MAX,
                max: crate::X_PROPERTY_MAX_VALUE_BYTES,
            })?;
    if value_len > crate::X_PROPERTY_MAX_VALUE_BYTES {
        return Err(XWireParseError::PropertyValueTooLarge {
            len: value_len,
            max: crate::X_PROPERTY_MAX_VALUE_BYTES,
        });
    }
    let expected_len = X_CHANGE_PROPERTY_REQ_LEN + padded_len(value_len);
    if bytes.len() != expected_len {
        return Err(XWireParseError::InvalidLength {
            opcode: X_CHANGE_PROPERTY,
            expected_at_least: expected_len,
            actual: bytes.len(),
        });
    }

    Ok(XWireRequest::ChangeProperty(XPropertyChange {
        mode,
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        property: context.byte_order.u32(&bytes[8..12]),
        property_type: context.byte_order.u32(&bytes[12..16]),
        format,
        bytes: bytes[X_CHANGE_PROPERTY_REQ_LEN..X_CHANGE_PROPERTY_REQ_LEN + value_len].to_vec(),
    }))
}

fn decode_set_selection_owner(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(
        X_SET_SELECTION_OWNER,
        X_SET_SELECTION_OWNER_REQ_LEN,
        bytes.len(),
    )?;
    let owner_raw = context.byte_order.u32(&bytes[4..8]);
    let owner = if owner_raw == 0 {
        None
    } else {
        Some(XResourceId::new(u64::from(owner_raw), 1))
    };
    Ok(XWireRequest::Authority(XAuthorityRequestPacket {
        transaction: context.transaction,
        namespace: context.namespace,
        kind: XAuthorityRequestKind::SetSelectionOwner {
            selection: context.byte_order.u32(&bytes[8..12]),
            owner,
            timestamp: context.byte_order.u32(&bytes[12..16]),
            selection_timestamp: context.byte_order.u32(&bytes[12..16]),
            kind: if owner.is_some() {
                XSelectionChangeKind::SetOwner
            } else {
                XSelectionChangeKind::ClearOwner
            },
        },
    }))
}

fn decode_get_selection_owner(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(
        X_GET_SELECTION_OWNER,
        X_GET_SELECTION_OWNER_REQ_LEN,
        bytes.len(),
    )?;
    Ok(XWireRequest::GetSelectionOwner {
        selection: context.byte_order.u32(&bytes[4..8]),
    })
}

fn decode_convert_selection(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(
        X_CONVERT_SELECTION,
        X_CONVERT_SELECTION_REQ_LEN,
        bytes.len(),
    )?;
    let target = context.byte_order.u32(&bytes[12..16]);
    Ok(XWireRequest::Authority(XAuthorityRequestPacket {
        transaction: context.transaction,
        namespace: context.namespace,
        kind: XAuthorityRequestKind::RequestSelection {
            requestor: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
            selection: context.byte_order.u32(&bytes[8..12]),
            target,
            target_name: format!("atom:{target}"),
            property: context.byte_order.u32(&bytes[16..20]),
            time: context.byte_order.u32(&bytes[20..24]),
            transfer: PortalTransferId::from_raw(context.transaction.raw()),
        },
    }))
}

/// RotateProperties: a window, a count of properties, a delta, then the
/// atoms; the count frames the request exactly.
fn decode_rotate_properties(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_ROTATE_PROPERTIES, X_ROTATE_PROPERTIES_REQ_LEN, bytes.len())?;
    let count = usize::from(context.byte_order.u16(&bytes[8..10]));
    require_exact_len(
        X_ROTATE_PROPERTIES,
        X_ROTATE_PROPERTIES_REQ_LEN + count * 4,
        bytes.len(),
    )?;
    let properties = (0..count)
        .map(|index| {
            let at = X_ROTATE_PROPERTIES_REQ_LEN + index * 4;
            context.byte_order.u32(&bytes[at..at + 4])
        })
        .collect();
    Ok(XWireRequest::RotateProperties {
        window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        delta: context.byte_order.i16(&bytes[10..12]),
        properties,
    })
}

fn decode_send_event(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_SEND_EVENT, X_SEND_EVENT_REQ_LEN, bytes.len())?;
    let event_type = bytes[12] & 0x7f;
    if event_type < 9 {
        return Err(XWireParseError::InvalidEventType(event_type));
    }
    let destination = XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1);
    if event_type != 31 {
        let mut event = [0; 32];
        event.copy_from_slice(&bytes[12..44]);
        // Delivered as sent: the protocol sets the most significant bit of
        // the type on every event SendEvent delivers, whatever the client
        // put in its template. That bit is how a recipient tells a sent
        // event from one the server generated, and what toolkits read as
        // send_event. The SelectionNotify form below marks itself.
        event[0] |= 0x80;
        let event_mask = context.byte_order.u32(&bytes[8..12]);
        return Ok(XWireRequest::SendSelectionNotify {
            destination,
            event_mask,
            event: XClientEvent::ClientMessage {
                sequence: 0,
                bytes: event,
                destination,
                event_mask,
                propagate: bytes[1] != 0,
            },
        });
    }
    let requestor = XResourceId::new(u64::from(context.byte_order.u32(&bytes[20..24])), 1);
    Ok(XWireRequest::SendSelectionNotify {
        destination,
        event_mask: context.byte_order.u32(&bytes[8..12]),
        event: XClientEvent::SelectionNotify {
            sequence: 0,
            // X11 SendEvent always marks the delivered event synthetic,
            // regardless of the bit supplied in the client's template.
            synthetic: true,
            time: context.byte_order.u32(&bytes[16..20]),
            requestor,
            selection: context.byte_order.u32(&bytes[24..28]),
            target: context.byte_order.u32(&bytes[28..32]),
            property: context.byte_order.u32(&bytes[32..36]),
        },
    })
}
