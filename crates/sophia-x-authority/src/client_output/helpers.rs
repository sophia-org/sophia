/// Encode the 60-byte font description both `QueryFont` and
/// `ListFontsWithInfo` carry, optionally followed by a name.
///
/// `QueryFont` appends a per-character array whenever the face's ink bounds
/// differ, because a client that sees identical bounds is entitled to assume
/// every character measures alike (`dix/dispatch.c:1362-1370`). A face with
/// constant metrics therefore sends 60 bytes where one with varying ink sends
/// one entry per matrix cell.
fn encode_font_info_reply(
    byte_order: XByteOrder,
    sequence: u16,
    metrics: &crate::XFontMetrics,
    name: Option<&[u8]>,
    with_char_infos: bool,
) -> Vec<u8> {
    let name = name.unwrap_or_default();
    let padded_name_len = padded_len(name.len());
    let properties = metrics.properties.len();
    let char_infos = if with_char_infos {
        metrics.query_font_char_infos()
    } else {
        0
    };
    let mut out = vec![0; 60 + padded_name_len + properties * 8 + char_infos * 12];
    let units = 7 + (padded_name_len / 4) + properties * 2 + char_infos * 3;
    write_reply_header(
        byte_order,
        &mut out[..X_CLIENT_OUTPUT_RECORD_LEN],
        sequence,
        u32::try_from(units).unwrap_or(7),
    );
    out[1] = u8::try_from(name.len()).unwrap_or(0);
    put_char_info(byte_order, &mut out[8..20], &metrics.min_bounds);
    put_char_info(byte_order, &mut out[24..36], &metrics.max_bounds);
    put_u16(byte_order, &mut out[40..42], metrics.min_char_or_byte2);
    put_u16(byte_order, &mut out[42..44], metrics.max_char_or_byte2);
    put_u16(byte_order, &mut out[44..46], metrics.default_char);
    put_u16(byte_order, &mut out[46..48], u16::try_from(properties).unwrap_or(0));
    out[48] = metrics.draw_direction;
    out[49] = metrics.min_byte1;
    out[50] = metrics.max_byte1;
    out[51] = u8::from(metrics.all_chars_exist);
    put_i16(byte_order, &mut out[52..54], metrics.font_ascent);
    put_i16(byte_order, &mut out[54..56], metrics.font_descent);
    put_u32(byte_order, &mut out[56..60], u32::try_from(char_infos).unwrap_or(0));
    out[60..60 + name.len()].copy_from_slice(name);
    let mut at = 60 + padded_name_len;
    for (_, value, _) in &metrics.properties {
        // The atom is resolved by the caller's table; a property whose name
        // has no atom is written as none rather than invented.
        put_u32(byte_order, &mut out[at..at + 4], 0);
        put_u32(byte_order, &mut out[at + 4..at + 8], *value);
        at += 8;
    }
    for index in 0..char_infos {
        let info = metrics.char_infos.get(index).copied().unwrap_or_default();
        put_char_info(byte_order, &mut out[at..at + 12], &info);
        at += 12;
    }
    out
}

/// One 12-byte CHARINFO.
fn put_char_info(byte_order: XByteOrder, out: &mut [u8], info: &crate::XCharInfo) {
    put_i16(byte_order, &mut out[0..2], info.left_side_bearing);
    put_i16(byte_order, &mut out[2..4], info.right_side_bearing);
    put_i16(byte_order, &mut out[4..6], info.character_width);
    put_i16(byte_order, &mut out[6..8], info.ascent);
    put_i16(byte_order, &mut out[8..10], info.descent);
    put_u16(byte_order, &mut out[10..12], info.attributes);
}

fn write_event_header(
    byte_order: XByteOrder,
    out: &mut [u8],
    event_type: u8,
    detail: u8,
    sequence: u16,
) {
    out[0] = event_type;
    out[1] = detail;
    put_u16(byte_order, &mut out[2..4], sequence);
}

fn write_reply_header(byte_order: XByteOrder, out: &mut [u8], sequence: u16, length_units: u32) {
    out[0] = 1;
    put_u16(byte_order, &mut out[2..4], sequence);
    put_u32(byte_order, &mut out[4..8], length_units);
}

fn put_resource(byte_order: XByteOrder, out: &mut [u8], resource: XResourceId) {
    put_u32(byte_order, out, raw_xid(resource));
}

fn raw_xid(resource: XResourceId) -> u32 {
    u32::try_from(resource.local.raw()).unwrap_or(0)
}

fn put_u16(byte_order: XByteOrder, out: &mut [u8], value: u16) {
    match byte_order {
        XByteOrder::LittleEndian => out.copy_from_slice(&value.to_le_bytes()),
        XByteOrder::BigEndian => out.copy_from_slice(&value.to_be_bytes()),
    }
}

fn put_i32(byte_order: XByteOrder, out: &mut [u8], value: i32) {
    put_u32(byte_order, out, value as u32);
}

fn put_i16(byte_order: XByteOrder, out: &mut [u8], value: i16) {
    match byte_order {
        XByteOrder::LittleEndian => out.copy_from_slice(&value.to_le_bytes()),
        XByteOrder::BigEndian => out.copy_from_slice(&value.to_be_bytes()),
    }
}

fn push_u32(byte_order: XByteOrder, out: &mut Vec<u8>, value: u32) {
    let mut bytes = [0; 4];
    put_u32(byte_order, &mut bytes, value);
    out.extend_from_slice(&bytes);
}

fn push_u16(byte_order: XByteOrder, out: &mut Vec<u8>, value: u16) {
    let mut bytes = [0; 2];
    put_u16(byte_order, &mut bytes, value);
    out.extend_from_slice(&bytes);
}

// XI2 FP3232 orders its two 32-bit fields independently of client byte order.
pub(crate) fn push_xi_fp3232(byte_order: XByteOrder, out: &mut Vec<u8>, value: i64) {
    push_u32(byte_order, out, (value >> 32) as u32);
    push_u32(byte_order, out, value as u32);
}

fn put_u32(byte_order: XByteOrder, out: &mut [u8], value: u32) {
    match byte_order {
        XByteOrder::LittleEndian => out.copy_from_slice(&value.to_le_bytes()),
        XByteOrder::BigEndian => out.copy_from_slice(&value.to_be_bytes()),
    }
}

fn put_u64(byte_order: XByteOrder, out: &mut [u8], value: u64) {
    let bytes = match byte_order {
        XByteOrder::LittleEndian => value.to_le_bytes(),
        XByteOrder::BigEndian => value.to_be_bytes(),
    };
    out.copy_from_slice(&bytes);
}
