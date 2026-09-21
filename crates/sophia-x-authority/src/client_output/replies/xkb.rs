fn encode_xkb_reply(
    byte_order: XByteOrder,
    reply: XClientReply,
) -> Result<Vec<u8>, XClientReply> {
    if !matches!(
        &reply,
            XClientReply::XkbUseExtension { .. }
            | XClientReply::XkbGetMap { .. }
            | XClientReply::XkbGetCompatMap { .. }
            | XClientReply::XkbGetIndicatorMap { .. }
            | XClientReply::XkbGetState { .. }
            | XClientReply::XkbGetControls { .. }
            | XClientReply::XkbGetNames { .. }
            | XClientReply::XkbGetDeviceInfo { .. }
            | XClientReply::XkbPerClientFlags { .. }
    ) {
        return Err(reply);
    }
    Ok(match reply {
                XClientReply::XkbUseExtension {
                    sequence,
                    supported,
                    server_major,
                    server_minor,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    out[1] = u8::from(supported);
                    put_u16(byte_order, &mut out[8..10], server_major);
                    put_u16(byte_order, &mut out[10..12], server_minor);
                    out
                }
                XClientReply::XkbGetMap {
                    sequence,
                    present,
                    keysyms,
                    modifier_map,
                } => {
                    let include_types = present & 1 != 0;
                    let include_syms = present & 2 != 0;
                    let include_modmap = present & 0x04 != 0;
                    let mut body = Vec::new();
                    if include_types {
                        for _ in 0..4 {
                            body.extend_from_slice(&[1, 1]);
                            push_u16(byte_order, &mut body, 0);
                            body.extend_from_slice(&[2, 1, 0, 0]);
                            body.extend_from_slice(&[1, 1, 1, 1]);
                            push_u16(byte_order, &mut body, 0);
                            body.extend_from_slice(&[0, 0]);
                        }
                    }
                    if include_syms {
                        for syms in &keysyms {
                            // One group with two levels. XKB encodes the group count
                            // directly in the low nibble of groupInfo.
                            body.extend_from_slice(&[0, 0, 0, 0, 1, 2]);
                            push_u16(byte_order, &mut body, 2);
                            push_u32(byte_order, &mut body, syms[0]);
                            push_u32(byte_order, &mut body, syms[1]);
                        }
                    }
                    if present & 0x10 != 0 {
                        // One zero action-count byte for every key in the advertised
                        // keycode range. No action records follow those counts.
                        body.resize(body.len().saturating_add(keysyms.len()), 0);
                        body.resize(padded_len(body.len()), 0);
                    }
                    if include_modmap {
                        for (keycode, modifiers) in &modifier_map {
                            body.extend_from_slice(&[*keycode, *modifiers]);
                        }
                        body.resize(padded_len(body.len()), 0);
                    }
                    let fixed_extra_len = 8usize;
                    let reply_units = u32::try_from((fixed_extra_len + body.len()) / 4)
                        .expect("bounded XKB map reply length");
                    let mut out = vec![0; 40];
                    out[0] = 1;
                    out[1] = 3;
                    put_u16(byte_order, &mut out[2..4], sequence);
                    put_u32(byte_order, &mut out[4..8], reply_units);
                    out[10] = 8;
                    out[11] = u8::MAX;
                    put_u16(byte_order, &mut out[12..14], present);
                    out[14] = 0;
                    out[15] = if include_types { 4 } else { 0 };
                    out[16] = if include_types { 4 } else { 0 };
                    out[17] = 8;
                    put_u16(
                        byte_order,
                        &mut out[18..20],
                        if include_syms {
                            u16::try_from(keysyms.len().saturating_mul(2)).unwrap_or(u16::MAX)
                        } else {
                            0
                        },
                    );
                    out[20] = if include_syms {
                        u8::try_from(keysyms.len()).unwrap_or(u8::MAX)
                    } else {
                        0
                    };
                    out[21] = if present & 0x10 != 0 { 8 } else { 0 };
                    out[24] = if present & 0x10 != 0 {
                        u8::try_from(keysyms.len()).unwrap_or(u8::MAX)
                    } else {
                        0
                    };
                    out[25] = if present & 0x20 != 0 { 8 } else { 0 };
                    out[28] = if present & 0x08 != 0 { 8 } else { 0 };
                    out[31] = if include_modmap { 8 } else { 0 };
                    out[32] = if include_modmap { 248 } else { 0 };
                    out[33] = if include_modmap {
                        u8::try_from(modifier_map.len()).unwrap_or(u8::MAX)
                    } else {
                        0
                    };
                    out[34] = if present & 0x80 != 0 { 8 } else { 0 };
                    out.extend_from_slice(&body);
                    out
                }
                XClientReply::XkbGetCompatMap {
                    sequence,
                    device_id,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    out[1] = device_id;
                    out
                }
                XClientReply::XkbGetIndicatorMap {
                    sequence,
                    device_id,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    out[1] = device_id;
                    out
                }
                XClientReply::XkbGetState {
                    sequence,
                    modifiers,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    out[1] = 3;
                    out[8] = modifiers;
                    out[9] = modifiers;
                    out[18] = modifiers;
                    out[20] = modifiers;
                    out
                }
                XClientReply::XkbGetControls { sequence } => {
                    let mut out = vec![0; 92];
                    write_reply_header(byte_order, &mut out[..32], sequence, 15);
                    out[1] = 3;
                    out[9] = 1;
                    put_u16(
                        byte_order,
                        &mut out[20..22],
                        crate::XKB_DEFAULT_REPEAT_DELAY_MSEC,
                    );
                    put_u16(
                        byte_order,
                        &mut out[22..24],
                        crate::XKB_DEFAULT_REPEAT_INTERVAL_MSEC,
                    );
                    out
                }
                XClientReply::XkbGetNames {
                    sequence,
                    which,
                    min_keycode,
                    max_keycode,
                    component_atoms,
                    type_atoms,
                    level_atoms,
                    key_names,
                } => {
                    let mut body = Vec::new();
                    for atom in component_atoms {
                        push_u32(byte_order, &mut body, atom);
                    }
                    if which & 0x40 != 0 {
                        for atom in &type_atoms {
                            push_u32(byte_order, &mut body, *atom);
                        }
                    }
                    if which & 0x80 != 0 {
                        // The count for each type must match the numLevels advertised
                        // by XkbGetMap; omitting the level slots makes the two replies
                        // structurally inconsistent and strict xkbcommon rejects the
                        // entire keymap. The names themselves are real atoms rather
                        // than None, because a client is entitled to ask the server
                        // what each one is called and being handed None is fatal to it.
                        body.extend(std::iter::repeat_n(2, type_atoms.len()));
                        body.resize(padded_len(body.len()), 0);
                        for atom in &level_atoms {
                            push_u32(byte_order, &mut body, *atom);
                        }
                    }
                    if which & 0x200 != 0 {
                        for name in &key_names {
                            body.extend_from_slice(name);
                        }
                    }
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(
                        byte_order,
                        &mut out,
                        sequence,
                        u32::try_from(body.len() / 4).unwrap_or(u32::MAX),
                    );
                    out[1] = 3;
                    put_u32(byte_order, &mut out[8..12], which);
                    out[12] = min_keycode;
                    out[13] = max_keycode;
                    out[14] = u8::try_from(type_atoms.len()).unwrap_or(u8::MAX);
                    out[18] = min_keycode;
                    out[19] = u8::try_from(key_names.len()).unwrap_or(u8::MAX);
                    out.extend_from_slice(&body);
                    out
                }
                XClientReply::XkbGetDeviceInfo {
                    sequence,
                    device_id,
                    supported,
                    unsupported,
                } => {
                    let mut out = vec![0; 36];
                    write_reply_header(byte_order, &mut out[..32], sequence, 1);
                    out[1] = device_id;
                    put_u16(byte_order, &mut out[10..12], supported);
                    put_u16(byte_order, &mut out[12..14], unsupported);
                    out[21] = 1;
                    out
                }
                XClientReply::XkbPerClientFlags {
                    sequence,
                    supported,
                    value,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    out[1] = 3;
                    put_u32(byte_order, &mut out[8..12], supported);
                    put_u32(byte_order, &mut out[12..16], value);
                    out
                }
        _ => unreachable!("reply family checked before encoding"),
    })
}
