fn encode_randr_reply(
    byte_order: XByteOrder,
    reply: XClientReply,
) -> Result<Vec<u8>, XClientReply> {
    if !matches!(
        &reply,
            XClientReply::RandrQueryVersion { .. }
            | XClientReply::RandrGetScreenSizeRange { .. }
            | XClientReply::RandrGetCrtcGammaSize { .. }
            | XClientReply::RandrGetCrtcGamma { .. }
            | XClientReply::RandrGetCrtcTransform { .. }
            | XClientReply::RandrGetPanning { .. }
            | XClientReply::RandrGetScreenResources { .. }
            | XClientReply::RandrGetOutputInfo { .. }
            | XClientReply::RandrGetOutputProperty { .. }
            | XClientReply::RandrListOutputProperties { .. }
            | XClientReply::RandrQueryOutputProperty { .. }
            | XClientReply::RandrGetCrtcInfo { .. }
            | XClientReply::RandrGetOutputPrimary { .. }
            | XClientReply::RandrGetProviders { .. }
            | XClientReply::RandrGetMonitors { .. }
    ) {
        return Err(reply);
    }
    Ok(match reply {
        XClientReply::RandrListOutputProperties { sequence, atoms } => {
            let mut out = vec![0; 32 + atoms.len() * 4];
            write_reply_header(byte_order, &mut out, sequence, atoms.len() as u32);
            put_u16(byte_order, &mut out[8..10], atoms.len() as u16);
            for (chunk, atom) in out[32..].chunks_exact_mut(4).zip(atoms) {
                put_u32(byte_order, chunk, atom);
            }
            out
        }
        XClientReply::RandrQueryOutputProperty {
            sequence, pending, range, immutable, valid_values,
        } => {
            let mut out = vec![0; 32 + valid_values.len() * 4];
            write_reply_header(byte_order, &mut out, sequence, valid_values.len() as u32);
            out[8] = u8::from(pending);
            out[9] = u8::from(range);
            out[10] = u8::from(immutable);
            for (chunk, value) in out[32..].chunks_exact_mut(4).zip(valid_values) {
                put_u32(byte_order, chunk, value as u32);
            }
            out
        }
                XClientReply::RandrQueryVersion {
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
                XClientReply::RandrGetScreenSizeRange {
                    sequence,
                    min_width,
                    min_height,
                    max_width,
                    max_height,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    put_u16(byte_order, &mut out[8..10], min_width);
                    put_u16(byte_order, &mut out[10..12], min_height);
                    put_u16(byte_order, &mut out[12..14], max_width);
                    put_u16(byte_order, &mut out[14..16], max_height);
                    out
                }
                XClientReply::RandrGetCrtcGammaSize { sequence, size } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    put_u16(byte_order, &mut out[8..10], size);
                    out
                }
                XClientReply::RandrGetCrtcGamma { sequence } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    put_u16(byte_order, &mut out[8..10], 0);
                    out
                }
                XClientReply::RandrGetCrtcTransform { sequence } => {
                    // The fixed-point identity matrices describe the only
                    // supported transform. An empty filter and false
                    // has-transforms flag make this a read-only capability.
                    const FIXED_ONE: u32 = 1 << 16;
                    let identity = [FIXED_ONE, 0, 0, 0, FIXED_ONE, 0, 0, 0, FIXED_ONE];
                    let mut out = vec![0; 96];
                    write_reply_header(byte_order, &mut out, sequence, 16);
                    for (index, value) in identity.into_iter().enumerate() {
                        put_u32(byte_order, &mut out[8 + index * 4..12 + index * 4], value);
                        put_u32(byte_order, &mut out[48 + index * 4..52 + index * 4], value);
                    }
                    out
                }
                XClientReply::RandrGetPanning {
                    sequence,
                    timestamp,
                } => {
                    // A 36-byte RandR reply with zero-sized panning and tracking
                    // rectangles means panning is disabled for this CRTC.
                    let mut out = vec![0; 36];
                    write_reply_header(byte_order, &mut out, sequence, 1);
                    out[1] = 0;
                    put_u32(byte_order, &mut out[8..12], timestamp);
                    out
                }
                XClientReply::RandrGetScreenResources {
                    sequence,
                    timestamp,
                    crtcs,
                    outputs,
                    modes,
                } => {
                    let names_len = modes.iter().map(|mode| mode.name.len()).sum::<usize>();
                    let payload_len = crtcs.len() * 4 + outputs.len() * 4 + modes.len() * 32 + names_len;
                    let padded_payload_len = (payload_len + 3) & !3;
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN + padded_payload_len];
                    write_reply_header(
                        byte_order,
                        &mut out,
                        sequence,
                        u32::try_from(padded_payload_len / 4).unwrap_or(u32::MAX),
                    );
                    put_u32(byte_order, &mut out[8..12], timestamp);
                    put_u32(byte_order, &mut out[12..16], timestamp);
                    put_u16(byte_order, &mut out[16..18], crtcs.len() as u16);
                    put_u16(byte_order, &mut out[18..20], outputs.len() as u16);
                    put_u16(byte_order, &mut out[20..22], modes.len() as u16);
                    put_u16(byte_order, &mut out[22..24], names_len as u16);
                    let mut offset = 32;
                    for id in crtcs.iter().chain(outputs.iter()) {
                        put_u32(byte_order, &mut out[offset..offset + 4], *id);
                        offset += 4;
                    }
                    for mode in &modes {
                        put_u32(byte_order, &mut out[offset..offset + 4], mode.id);
                        put_u16(byte_order, &mut out[offset + 4..offset + 6], mode.width);
                        put_u16(byte_order, &mut out[offset + 6..offset + 8], mode.height);
                        // The mode's own timing where the output reported
                        // one. Where it did not, the fallback declares no
                        // blanking and derives a clock from the nominal
                        // refresh -- a modeline that cannot physically exist,
                        // so a reader can tell it was never measured. It used
                        // to be the only thing sent, which is why a client
                        // asking this server how the display scans has been
                        // told zero blanking since RandR was written here.
                        let timing = mode.timing.filter(|timing| timing.is_valid());
                        let dot_clock = timing.map_or_else(
                            || {
                                u64::from(mode.width)
                                    .saturating_mul(u64::from(mode.height))
                                    .saturating_mul(u64::from(mode.refresh_millihz))
                                    / 1_000
                            },
                            // RandR reports the clock in Hz; DRM keeps kHz.
                            |timing| u64::from(timing.clock_khz).saturating_mul(1_000),
                        );
                        put_u32(
                            byte_order,
                            &mut out[offset + 8..offset + 12],
                            u32::try_from(dot_clock).unwrap_or(u32::MAX),
                        );
                        let (hsync_start, hsync_end, htotal) = timing.map_or(
                            (mode.width, mode.width, mode.width),
                            |timing| (timing.hsync_start, timing.hsync_end, timing.htotal),
                        );
                        let (vsync_start, vsync_end, vtotal) = timing.map_or(
                            (mode.height, mode.height, mode.height),
                            |timing| (timing.vsync_start, timing.vsync_end, timing.vtotal),
                        );
                        put_u16(byte_order, &mut out[offset + 12..offset + 14], hsync_start);
                        put_u16(byte_order, &mut out[offset + 14..offset + 16], hsync_end);
                        put_u16(byte_order, &mut out[offset + 16..offset + 18], htotal);
                        put_u16(byte_order, &mut out[offset + 20..offset + 22], vsync_start);
                        put_u16(byte_order, &mut out[offset + 22..offset + 24], vsync_end);
                        put_u16(byte_order, &mut out[offset + 24..offset + 26], vtotal);
                        put_u16(
                            byte_order,
                            &mut out[offset + 26..offset + 28],
                            mode.name.len() as u16,
                        );
                        offset += 32;
                    }
                    for mode in modes {
                        let end = offset + mode.name.len();
                        out[offset..end].copy_from_slice(&mode.name);
                        offset = end;
                    }
                    out
                }
                XClientReply::RandrGetOutputInfo {
                    sequence,
                    timestamp,
                    crtc,
                    mm_width,
                    mm_height,
                    crtcs,
                    modes,
                    name,
                } => {
                    let payload_len = crtcs.len() * 4 + modes.len() * 4 + name.len();
                    let padded_payload_len = (payload_len + 3) & !3;
                    let mut out = vec![0; 32 + padded_payload_len];
                    write_reply_header(
                        byte_order,
                        &mut out,
                        sequence,
                        (padded_payload_len / 4) as u32,
                    );
                    out[1] = 0;
                    put_u32(byte_order, &mut out[8..12], timestamp);
                    put_u32(byte_order, &mut out[12..16], crtc);
                    put_u32(byte_order, &mut out[16..20], mm_width);
                    put_u32(byte_order, &mut out[20..24], mm_height);
                    out[24] = 0;
                    out[25] = 0;
                    put_u16(byte_order, &mut out[26..28], crtcs.len() as u16);
                    put_u16(byte_order, &mut out[28..30], modes.len() as u16);
                    put_u16(byte_order, &mut out[30..32], u16::from(!modes.is_empty()));
                    out.extend_from_slice(&[0; 4]);
                    put_u16(byte_order, &mut out[34..36], name.len() as u16);
                    let mut payload = Vec::with_capacity(padded_payload_len);
                    for id in crtcs.iter().chain(modes.iter()) {
                        push_u32(byte_order, &mut payload, *id);
                    }
                    payload.extend_from_slice(&name);
                    payload.resize(padded_payload_len, 0);
                    out.truncate(36);
                    out.extend_from_slice(&payload);
                    let reply_units = out.len().saturating_sub(32).div_ceil(4);
                    out.resize(32 + reply_units * 4, 0);
                    put_u32(byte_order, &mut out[4..8], reply_units as u32);
                    out
                }
                XClientReply::RandrGetOutputProperty {
                    sequence,
                    property_type,
                    bytes_after,
                    format,
                    data,
                } => {
                    let padded_len = (data.len() + 3) & !3;
                    let mut out = vec![0; 32 + padded_len];
                    write_reply_header(byte_order, &mut out, sequence, (padded_len / 4) as u32);
                    out[1] = format;
                    put_u32(byte_order, &mut out[8..12], property_type);
                    put_u32(byte_order, &mut out[12..16], bytes_after);
                    let item_width = usize::from(format).checked_div(8).unwrap_or(0);
                    let items = data.len().checked_div(item_width).unwrap_or(0);
                    put_u32(
                        byte_order,
                        &mut out[16..20],
                        u32::try_from(items).unwrap_or(u32::MAX),
                    );
                    out[32..32 + data.len()].copy_from_slice(&data);
                    out
                }
                XClientReply::RandrGetCrtcInfo {
                    sequence,
                    timestamp,
                    x,
                    y,
                    width,
                    height,
                    mode,
                    outputs,
                } => {
                    let payload_len = outputs.len() * 8;
                    let mut out = vec![0; 32 + payload_len];
                    write_reply_header(byte_order, &mut out, sequence, (payload_len / 4) as u32);
                    out[1] = 0;
                    put_u32(byte_order, &mut out[8..12], timestamp);
                    put_i16(byte_order, &mut out[12..14], x);
                    put_i16(byte_order, &mut out[14..16], y);
                    put_u16(byte_order, &mut out[16..18], width);
                    put_u16(byte_order, &mut out[18..20], height);
                    put_u32(byte_order, &mut out[20..24], mode);
                    put_u16(byte_order, &mut out[24..26], 1);
                    put_u16(byte_order, &mut out[26..28], 1);
                    put_u16(byte_order, &mut out[28..30], outputs.len() as u16);
                    put_u16(byte_order, &mut out[30..32], outputs.len() as u16);
                    let mut offset = 32;
                    for id in outputs.iter().chain(outputs.iter()) {
                        put_u32(byte_order, &mut out[offset..offset + 4], *id);
                        offset += 4;
                    }
                    out
                }
                XClientReply::RandrGetOutputPrimary { sequence, output } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    put_u32(byte_order, &mut out[8..12], output);
                    out
                }
                XClientReply::RandrGetProviders {
                    sequence,
                    timestamp,
                } => {
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN];
                    write_reply_header(byte_order, &mut out, sequence, 0);
                    put_u32(byte_order, &mut out[8..12], timestamp);
                    put_u16(byte_order, &mut out[12..14], 0);
                    out
                }
                XClientReply::RandrGetMonitors {
                    sequence,
                    timestamp,
                    monitors,
                } => {
                    let payload_len: usize = monitors
                        .iter()
                        .map(|monitor| 24 + monitor.outputs.len() * 4)
                        .sum();
                    let mut out = vec![0; X_CLIENT_OUTPUT_RECORD_LEN + payload_len];
                    write_reply_header(byte_order, &mut out, sequence, (payload_len / 4) as u32);
                    put_u32(byte_order, &mut out[8..12], timestamp);
                    put_u32(byte_order, &mut out[12..16], monitors.len() as u32);
                    put_u32(
                        byte_order,
                        &mut out[16..20],
                        monitors.iter().map(|m| m.outputs.len() as u32).sum(),
                    );
                    let mut offset = 32;
                    for monitor in monitors {
                        put_u32(byte_order, &mut out[offset..offset + 4], monitor.name);
                        out[offset + 4] = u8::from(monitor.primary);
                        out[offset + 5] = 1;
                        put_u16(
                            byte_order,
                            &mut out[offset + 6..offset + 8],
                            monitor.outputs.len() as u16,
                        );
                        put_i16(byte_order, &mut out[offset + 8..offset + 10], monitor.x);
                        put_i16(byte_order, &mut out[offset + 10..offset + 12], monitor.y);
                        put_u16(
                            byte_order,
                            &mut out[offset + 12..offset + 14],
                            monitor.width,
                        );
                        put_u16(
                            byte_order,
                            &mut out[offset + 14..offset + 16],
                            monitor.height,
                        );
                        put_u32(
                            byte_order,
                            &mut out[offset + 16..offset + 20],
                            monitor.mm_width,
                        );
                        put_u32(
                            byte_order,
                            &mut out[offset + 20..offset + 24],
                            monitor.mm_height,
                        );
                        offset += 24;
                        for output in monitor.outputs {
                            put_u32(byte_order, &mut out[offset..offset + 4], output);
                            offset += 4;
                        }
                    }
                    out
                }
        _ => unreachable!("reply family checked before encoding"),
    })
}
