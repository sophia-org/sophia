// The window facts read from properties: the window type, transience,
// size hints and output reservations, with their decoders. Included into
// `property.rs`; split out to keep that file within the layout ledger's
// bound (t026).

/// Reduces EWMH functional window types to the presentation distinction the
/// Engine needs. Unknown extension atoms are skipped, as required by EWMH;
/// a missing recognized type falls back to a normal policy-managed toplevel.
pub fn decode_x_window_type_facts(
    record: &XPropertyRecord,
    atoms: &XAtomTable,
    byte_order: XByteOrder,
) -> Option<Result<XWindowTypeFacts, XWindowTypeDecodeError>> {
    if atoms.name(record.property) != Some("_NET_WM_WINDOW_TYPE") {
        return None;
    }
    if atoms.name(record.property_type) != Some("ATOM") {
        return Some(Err(XWindowTypeDecodeError::InvalidType));
    }
    if record.format != 32 {
        return Some(Err(XWindowTypeDecodeError::InvalidFormat));
    }
    if record.bytes.is_empty() || !record.bytes.len().is_multiple_of(4) {
        return Some(Err(XWindowTypeDecodeError::InvalidLength));
    }

    let facts = record.bytes.chunks_exact(4).find_map(|bytes| {
        let atom = byte_order.u32(bytes);
        match atoms.name(atom) {
            Some("_NET_WM_WINDOW_TYPE_NORMAL") => Some(XWindowTypeFacts::default()),
            Some("_NET_WM_WINDOW_TYPE_DESKTOP") | Some("_NET_WM_WINDOW_TYPE_DOCK") => {
                Some(XWindowTypeFacts {
                    kind: LayoutNodeKind::Utility,
                    placement_preference: SurfacePlacementPreference::Default,
                    client_positioned: true,
                })
            }
            Some("_NET_WM_WINDOW_TYPE_TOOLBAR") | Some("_NET_WM_WINDOW_TYPE_UTILITY") => {
                Some(XWindowTypeFacts {
                    kind: LayoutNodeKind::Utility,
                    placement_preference: SurfacePlacementPreference::Floating,
                    client_positioned: false,
                })
            }
            Some("_NET_WM_WINDOW_TYPE_SPLASH") | Some("_NET_WM_WINDOW_TYPE_DIALOG") => {
                Some(XWindowTypeFacts {
                    kind: LayoutNodeKind::Dialog,
                    placement_preference: SurfacePlacementPreference::Floating,
                    client_positioned: false,
                })
            }
            Some("_NET_WM_WINDOW_TYPE_MENU")
            | Some("_NET_WM_WINDOW_TYPE_DROPDOWN_MENU")
            | Some("_NET_WM_WINDOW_TYPE_POPUP_MENU")
            | Some("_NET_WM_WINDOW_TYPE_TOOLTIP")
            | Some("_NET_WM_WINDOW_TYPE_NOTIFICATION")
            | Some("_NET_WM_WINDOW_TYPE_COMBO")
            | Some("_NET_WM_WINDOW_TYPE_DND") => Some(XWindowTypeFacts {
                kind: LayoutNodeKind::Popup,
                placement_preference: SurfacePlacementPreference::Floating,
                client_positioned: false,
            }),
            _ => None,
        }
    });
    Some(Ok(facts.unwrap_or_default()))
}

pub fn decode_x_transient_for(
    record: &XPropertyRecord,
    atoms: &XAtomTable,
    byte_order: XByteOrder,
) -> Option<Result<XResourceId, XTransientForDecodeError>> {
    if atoms.name(record.property) != Some("WM_TRANSIENT_FOR") {
        return None;
    }
    if atoms.name(record.property_type) != Some("WINDOW") {
        return Some(Err(XTransientForDecodeError::InvalidType));
    }
    if record.format != 32 {
        return Some(Err(XTransientForDecodeError::InvalidFormat));
    }
    if record.bytes.len() != 4 {
        return Some(Err(XTransientForDecodeError::InvalidLength));
    }
    let raw = u64::from(byte_order.u32(&record.bytes));
    let owner = XResourceId::new(raw, 1);
    if !owner.is_valid() {
        return Some(Err(XTransientForDecodeError::InvalidWindow));
    }
    Some(Ok(owner))
}

pub fn decode_x_size_hints(
    record: &XPropertyRecord,
    atoms: &XAtomTable,
    byte_order: XByteOrder,
) -> Option<Result<SurfaceConstraints, XSizeHintsDecodeError>> {
    if atoms.name(record.property) != Some("WM_NORMAL_HINTS") {
        return None;
    }
    if atoms.name(record.property_type) != Some("WM_SIZE_HINTS") {
        return Some(Err(XSizeHintsDecodeError::InvalidType));
    }
    if record.format != 32 {
        return Some(Err(XSizeHintsDecodeError::InvalidFormat));
    }
    if record.bytes.len() < 9 * 4 {
        return Some(Err(XSizeHintsDecodeError::InvalidLength));
    }
    let value = |index: usize| byte_order.u32(&record.bytes[index * 4..index * 4 + 4]) as i32;
    let flags = value(0) as u32;
    let extent = |width_index: usize, height_index: usize| {
        let size = Size {
            width: value(width_index),
            height: value(height_index),
        };
        (size.width > 0 && size.height > 0)
            .then_some(size)
            .ok_or(XSizeHintsDecodeError::InvalidExtent)
    };
    const P_MIN_SIZE: u32 = 1 << 4;
    const P_MAX_SIZE: u32 = 1 << 5;
    let min_size = if flags & P_MIN_SIZE != 0 {
        match extent(5, 6) {
            Ok(size) => Some(size),
            Err(error) => return Some(Err(error)),
        }
    } else {
        None
    };
    let max_size = if flags & P_MAX_SIZE != 0 {
        match extent(7, 8) {
            Ok(size) => Some(size),
            Err(error) => return Some(Err(error)),
        }
    } else {
        None
    };
    if matches!((min_size, max_size), (Some(minimum), Some(maximum))
        if minimum.width > maximum.width || minimum.height > maximum.height)
    {
        return Some(Err(XSizeHintsDecodeError::InvalidBounds));
    }
    Some(Ok(SurfaceConstraints { min_size, max_size }))
}

pub fn decode_x_output_reservations(
    record: &XPropertyRecord,
    atoms: &XAtomTable,
    byte_order: XByteOrder,
    root: Rect,
) -> Option<Result<Vec<OutputReservation>, XOutputReservationDecodeError>> {
    let property_name = atoms.name(record.property)?;
    match property_name {
        X_ATOM_NAME_NET_WM_STRUT_PARTIAL => {
            Some(decode_partial_output_reservations(record, byte_order, root))
        }
        X_ATOM_NAME_NET_WM_STRUT => {
            Some(decode_legacy_output_reservations(record, byte_order, root))
        }
        _ => None,
    }
}

pub fn x_output_reservations_for_window(
    properties: &XPropertyTable,
    atoms: &XAtomTable,
    namespace: NamespaceId,
    window: XResourceId,
    byte_order: XByteOrder,
    root: Rect,
) -> Vec<OutputReservation> {
    let partial = atoms
        .atom(X_ATOM_NAME_NET_WM_STRUT_PARTIAL)
        .and_then(|property| properties.get(namespace, window, property))
        .and_then(|record| decode_x_output_reservations(record, atoms, byte_order, root))
        .and_then(Result::ok);
    if let Some(reservations) = partial {
        return reservations;
    }

    atoms
        .atom(X_ATOM_NAME_NET_WM_STRUT)
        .and_then(|property| properties.get(namespace, window, property))
        .and_then(|record| decode_x_output_reservations(record, atoms, byte_order, root))
        .and_then(Result::ok)
        .unwrap_or_default()
}

pub fn metadata_property_candidate(
    record: &XPropertyRecord,
    atoms: &XAtomTable,
) -> Option<XMetadataPropertyCandidate> {
    let property_name = atoms.name(record.property)?;
    if !is_metadata_candidate_name(property_name) {
        return None;
    }
    Some(XMetadataPropertyCandidate {
        namespace: record.namespace,
        window: record.window,
        property: record.property,
        property_name: property_name.to_owned(),
        property_type: record.property_type,
        property_type_name: atoms
            .name(record.property_type)
            .map(std::borrow::ToOwned::to_owned),
        format: record.format,
        byte_len: record.bytes.len(),
        generation: record.generation,
    })
}

fn decode_partial_output_reservations(
    record: &XPropertyRecord,
    byte_order: XByteOrder,
    root: Rect,
) -> Result<Vec<OutputReservation>, XOutputReservationDecodeError> {
    const PARTIAL_CARDINAL_COUNT: usize = 12;
    let values = decode_cardinals(record, byte_order, PARTIAL_CARDINAL_COUNT)?;
    let root_horizontal = root_horizontal_span(root)?;
    let root_vertical = root_vertical_span(root)?;
    let mut reservations = Vec::with_capacity(4);
    push_output_reservation(
        &mut reservations,
        OutputEdge::Left,
        values[0],
        values[4],
        values[5],
        root.width,
        root_vertical,
    )?;
    push_output_reservation(
        &mut reservations,
        OutputEdge::Right,
        values[1],
        values[6],
        values[7],
        root.width,
        root_vertical,
    )?;
    push_output_reservation(
        &mut reservations,
        OutputEdge::Top,
        values[2],
        values[8],
        values[9],
        root.height,
        root_horizontal,
    )?;
    push_output_reservation(
        &mut reservations,
        OutputEdge::Bottom,
        values[3],
        values[10],
        values[11],
        root.height,
        root_horizontal,
    )?;
    Ok(reservations)
}

fn decode_legacy_output_reservations(
    record: &XPropertyRecord,
    byte_order: XByteOrder,
    root: Rect,
) -> Result<Vec<OutputReservation>, XOutputReservationDecodeError> {
    const LEGACY_CARDINAL_COUNT: usize = 4;
    let values = decode_cardinals(record, byte_order, LEGACY_CARDINAL_COUNT)?;
    let horizontal = root_horizontal_span(root)?;
    let vertical = root_vertical_span(root)?;
    let mut reservations = Vec::with_capacity(4);
    push_legacy_output_reservation(
        &mut reservations,
        OutputEdge::Left,
        values[0],
        root.width,
        vertical,
    )?;
    push_legacy_output_reservation(
        &mut reservations,
        OutputEdge::Right,
        values[1],
        root.width,
        vertical,
    )?;
    push_legacy_output_reservation(
        &mut reservations,
        OutputEdge::Top,
        values[2],
        root.height,
        horizontal,
    )?;
    push_legacy_output_reservation(
        &mut reservations,
        OutputEdge::Bottom,
        values[3],
        root.height,
        horizontal,
    )?;
    Ok(reservations)
}

fn decode_cardinals(
    record: &XPropertyRecord,
    byte_order: XByteOrder,
    count: usize,
) -> Result<Vec<u32>, XOutputReservationDecodeError> {
    if record.property_type != X_ATOM_CARDINAL {
        return Err(XOutputReservationDecodeError::InvalidType);
    }
    if record.format != 32 {
        return Err(XOutputReservationDecodeError::InvalidFormat);
    }
    if record.bytes.len() != count.saturating_mul(4) {
        return Err(XOutputReservationDecodeError::InvalidLength);
    }
    Ok(record
        .bytes
        .chunks_exact(4)
        .map(|bytes| byte_order.u32(bytes))
        .collect())
}

fn push_output_reservation(
    reservations: &mut Vec<OutputReservation>,
    edge: OutputEdge,
    depth: u32,
    start: u32,
    inclusive_end: u32,
    maximum_depth: i32,
    root_span: AxisSpan,
) -> Result<(), XOutputReservationDecodeError> {
    if depth == 0 {
        return Ok(());
    }
    let depth = i32::try_from(depth).map_err(|_| XOutputReservationDecodeError::ValueOutOfRange)?;
    let start = i32::try_from(start).map_err(|_| XOutputReservationDecodeError::ValueOutOfRange)?;
    let end = inclusive_end
        .checked_add(1)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or(XOutputReservationDecodeError::ValueOutOfRange)?;
    let span = AxisSpan { start, end };
    if depth > maximum_depth
        || span.is_empty()
        || span.start < root_span.start
        || span.end > root_span.end
    {
        return Err(XOutputReservationDecodeError::ValueOutOfRange);
    }
    reservations.push(OutputReservation { edge, depth, span });
    Ok(())
}

fn push_legacy_output_reservation(
    reservations: &mut Vec<OutputReservation>,
    edge: OutputEdge,
    depth: u32,
    maximum_depth: i32,
    span: AxisSpan,
) -> Result<(), XOutputReservationDecodeError> {
    if depth == 0 {
        return Ok(());
    }
    let depth = i32::try_from(depth).map_err(|_| XOutputReservationDecodeError::ValueOutOfRange)?;
    if depth > maximum_depth {
        return Err(XOutputReservationDecodeError::ValueOutOfRange);
    }
    reservations.push(OutputReservation { edge, depth, span });
    Ok(())
}

fn root_horizontal_span(root: Rect) -> Result<AxisSpan, XOutputReservationDecodeError> {
    if root.is_empty() {
        return Err(XOutputReservationDecodeError::InvalidRoot);
    }
    Ok(AxisSpan {
        start: root.x,
        end: root
            .x
            .checked_add(root.width)
            .ok_or(XOutputReservationDecodeError::InvalidRoot)?,
    })
}

fn root_vertical_span(root: Rect) -> Result<AxisSpan, XOutputReservationDecodeError> {
    if root.is_empty() {
        return Err(XOutputReservationDecodeError::InvalidRoot);
    }
    Ok(AxisSpan {
        start: root.y,
        end: root
            .y
            .checked_add(root.height)
            .ok_or(XOutputReservationDecodeError::InvalidRoot)?,
    })
}
