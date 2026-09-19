fn encode_core_late_reply(
    byte_order: XByteOrder,
    reply: XClientReply,
) -> Result<Vec<u8>, XClientReply> {
    if !matches!(
        &reply,
            XClientReply::BigRequestsEnable { .. }
            | XClientReply::GetInputFocus { .. }
            | XClientReply::QueryPointer { .. }
            | XClientReply::GetModifierMapping { .. }
            | XClientReply::GetPointerMapping { .. }
            | XClientReply::GetKeyboardMapping { .. }
            | XClientReply::GetKeyboardControl { .. }
            | XClientReply::TranslateCoordinates { .. }
            | XClientReply::QueryFont { .. }
            | XClientReply::QueryTextExtents { .. }
            | XClientReply::GetProperty { .. }
            | XClientReply::GetSelectionOwner { .. }
            | XClientReply::AllocNamedColor { .. }
            | XClientReply::LookupColor { .. }
            | XClientReply::AllocColor { .. }
            | XClientReply::ListProperties { .. }
            | XClientReply::QueryColors { .. }
    ) {
        return Err(reply);
    }
    Ok(match reply {
                XClientReply::BigRequestsEnable {
                    sequence,
                    maximum_request_length,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    put_u32(byte_order, &mut out[8..12], maximum_request_length);
                    out
                }
                XClientReply::GetInputFocus {
                    sequence,
                    focus,
                    revert_to,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    out[1] = revert_to;
                    put_resource(byte_order, &mut out[8..12], focus);
                    out
                }
                XClientReply::QueryPointer {
                    sequence,
                    root,
                    child,
                    root_x,
                    root_y,
                    win_x,
                    win_y,
                    mask,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    out[1] = 1;
                    put_resource(byte_order, &mut out[8..12], root);
                    put_resource(byte_order, &mut out[12..16], child);
                    put_i16(byte_order, &mut out[16..18], root_x);
                    put_i16(byte_order, &mut out[18..20], root_y);
                    put_i16(byte_order, &mut out[20..22], win_x);
                    put_i16(byte_order, &mut out[22..24], win_y);
                    put_u16(byte_order, &mut out[24..26], mask);
                    out
                }
                XClientReply::GetModifierMapping {
                    sequence,
                    keycodes_per_modifier,
                    keycodes,
                } => {
                    let padded_keycodes_len = padded_len(keycodes.len());
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN + padded_keycodes_len];
                    write_reply_header(
                        byte_order,
                        &mut out[..X_CLIENT_OUTPUT_RECORD_LEN],
                        sequence,
                        u32::try_from(padded_keycodes_len / 4).unwrap_or(0),
                    );
                    out[1] = keycodes_per_modifier;
                    out[X_CLIENT_OUTPUT_RECORD_LEN..X_CLIENT_OUTPUT_RECORD_LEN + keycodes.len()]
                        .copy_from_slice(&keycodes);
                    out
                }
                XClientReply::GetPointerMapping { sequence, mapping } => {
                    let padded_mapping_len = padded_len(mapping.len());
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN + padded_mapping_len];
                    write_reply_header(
                        byte_order,
                        &mut out[..X_CLIENT_OUTPUT_RECORD_LEN],
                        sequence,
                        u32::try_from(padded_mapping_len / 4).unwrap_or(0),
                    );
                    out[1] = u8::try_from(mapping.len()).unwrap_or(0);
                    out[X_CLIENT_OUTPUT_RECORD_LEN..X_CLIENT_OUTPUT_RECORD_LEN + mapping.len()]
                        .copy_from_slice(&mapping);
                    out
                }
                XClientReply::GetKeyboardMapping {
                    sequence,
                    keysyms_per_keycode,
                    keysyms,
                } => {
                    let keysyms_len = keysyms.len().saturating_mul(4);
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN + keysyms_len];
                    write_reply_header(
                        byte_order,
                        &mut out[..X_CLIENT_OUTPUT_RECORD_LEN],
                        sequence,
                        u32::try_from(keysyms_len / 4).unwrap_or(0),
                    );
                    out[1] = keysyms_per_keycode;
                    let mut offset = X_CLIENT_OUTPUT_RECORD_LEN;
                    for keysym in keysyms {
                        put_u32(byte_order, &mut out[offset..offset + 4], keysym);
                        offset += 4;
                    }
                    out
                }
                XClientReply::GetKeyboardControl { sequence } => {
                    let mut out = vec![0; 52];
                    write_reply_header(byte_order, &mut out, sequence, 5);
                    out[1] = 1;
                    out[13] = 50;
                    put_u16(byte_order, &mut out[14..16], 400);
                    put_u16(byte_order, &mut out[16..18], 100);
                    out[20..52].fill(0xff);
                    out
                }
                XClientReply::TranslateCoordinates {
                    sequence,
                    same_screen,
                    child,
                    dst_x,
                    dst_y,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    out[1] = u8::from(same_screen);
                    put_resource(
                        byte_order,
                        &mut out[8..12],
                        child.unwrap_or(XResourceId::NONE),
                    );
                    put_i16(byte_order, &mut out[12..14], dst_x);
                    put_i16(byte_order, &mut out[14..16], dst_y);
                    out
                }
                XClientReply::QueryFont { sequence, metrics } => {
                    encode_font_info_reply(byte_order, sequence, &metrics, None, true)
                }
                XClientReply::QueryTextExtents { sequence, extents } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    out[1] = extents.draw_direction;
                    put_i16(byte_order, &mut out[8..10], extents.font_ascent);
                    put_i16(byte_order, &mut out[10..12], extents.font_descent);
                    put_i16(byte_order, &mut out[12..14], extents.overall_ascent);
                    put_i16(byte_order, &mut out[14..16], extents.overall_descent);
                    put_i32(byte_order, &mut out[16..20], extents.overall_width);
                    put_i32(byte_order, &mut out[20..24], extents.overall_left);
                    put_i32(byte_order, &mut out[24..28], extents.overall_right);
                    out
                }
                XClientReply::GetProperty {
                    sequence,
                    property_type,
                    format,
                    bytes_after,
                    item_count,
                    bytes,
                } => {
                    let padded_value_len = padded_len(bytes.len());
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN + padded_value_len];
                    write_reply_header(
                        byte_order,
                        &mut out[..X_CLIENT_OUTPUT_RECORD_LEN],
                        sequence,
                        u32::try_from(padded_value_len / 4).unwrap_or(0),
                    );
                    out[1] = format;
                    put_u32(byte_order, &mut out[8..12], property_type);
                    put_u32(byte_order, &mut out[12..16], bytes_after);
                    put_u32(byte_order, &mut out[16..20], item_count);
                    out[X_CLIENT_OUTPUT_RECORD_LEN..X_CLIENT_OUTPUT_RECORD_LEN + bytes.len()]
                        .copy_from_slice(&bytes);
                    out
                }
                XClientReply::GetSelectionOwner { sequence, owner } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    put_u32(
                        byte_order,
                        &mut out[8..12],
                        owner
                            .map(|resource| u32::try_from(resource.local.raw()).unwrap_or(0))
                            .unwrap_or(0),
                    );
                    out
                }
                XClientReply::LookupColor { sequence, exact, screen } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    for (offset, value) in [(8, exact.red), (10, exact.green),
                        (12, exact.blue), (14, screen.red), (16, screen.green),
                        (18, screen.blue)] {
                        put_u16(byte_order, &mut out[offset..offset + 2], value);
                    }
                    out
                }
                XClientReply::AllocNamedColor {
                    sequence,
                    pixel,
                    exact,
                    screen,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    put_u32(byte_order, &mut out[8..12], pixel);
                    put_u16(byte_order, &mut out[12..14], exact.red);
                    put_u16(byte_order, &mut out[14..16], exact.green);
                    put_u16(byte_order, &mut out[16..18], exact.blue);
                    put_u16(byte_order, &mut out[18..20], screen.red);
                    put_u16(byte_order, &mut out[20..22], screen.green);
                    put_u16(byte_order, &mut out[22..24], screen.blue);
                    out
                }
                XClientReply::AllocColor {
                    sequence,
                    pixel,
                    red,
                    green,
                    blue,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    put_u16(byte_order, &mut out[8..10], red);
                    put_u16(byte_order, &mut out[10..12], green);
                    put_u16(byte_order, &mut out[12..14], blue);
                    put_u32(byte_order, &mut out[16..20], pixel);
                    out
                }
                XClientReply::ListProperties { sequence, atoms } => {
                    let atoms_len = atoms.len().saturating_mul(4);
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN + atoms_len];
                    write_reply_header(
                        byte_order,
                        &mut out[..X_CLIENT_OUTPUT_RECORD_LEN],
                        sequence,
                        u32::try_from(atoms.len()).unwrap_or(0),
                    );
                    put_u16(
                        byte_order,
                        &mut out[8..10],
                        u16::try_from(atoms.len()).unwrap_or(0),
                    );
                    let mut offset = X_CLIENT_OUTPUT_RECORD_LEN;
                    for atom in atoms {
                        put_u32(byte_order, &mut out[offset..offset + 4], atom);
                        offset += 4;
                    }
                    out
                }
                XClientReply::QueryColors { sequence, colors } => {
                    let colors_len = colors.len().saturating_mul(8);
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN + colors_len];
                    write_reply_header(
                        byte_order,
                        &mut out[..X_CLIENT_OUTPUT_RECORD_LEN],
                        sequence,
                        u32::try_from(colors_len / 4).unwrap_or(0),
                    );
                    put_u16(
                        byte_order,
                        &mut out[8..10],
                        u16::try_from(colors.len()).unwrap_or(0),
                    );
                    let mut offset = X_CLIENT_OUTPUT_RECORD_LEN;
                    for color in colors {
                        put_u16(byte_order, &mut out[offset..offset + 2], color.red);
                        put_u16(byte_order, &mut out[offset + 2..offset + 4], color.green);
                        put_u16(byte_order, &mut out[offset + 4..offset + 6], color.blue);
                        offset += 8;
                    }
                    out
                }
        _ => unreachable!("reply family checked before encoding"),
    })
}
