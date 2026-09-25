// Request builders for RENDER, XFIXES regions and SHAPE, the second half
// of the extension support. Included from x11_wire.rs beside
// support_extensions.rs (t026).

fn render_query_version_request(byte_order: XByteOrder, major: u32, minor: u32) -> Vec<u8> {
    let mut out = vec![X_RENDER_MAJOR_OPCODE, X_RENDER_QUERY_VERSION_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 3);
    push_u32(&mut out, byte_order, major);
    push_u32(&mut out, byte_order, minor);
    out
}

fn render_query_pict_formats_request(byte_order: XByteOrder) -> Vec<u8> {
    let mut out = vec![X_RENDER_MAJOR_OPCODE, X_RENDER_QUERY_PICT_FORMATS_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 1);
    out
}

/// A bare header-only request for any RENDER minor, for probing refusals.
fn render_minor_request(byte_order: XByteOrder, minor_opcode: u8) -> Vec<u8> {
    let mut out = vec![X_RENDER_MAJOR_OPCODE, minor_opcode];
    push_u16(&mut out, byte_order, 1);
    out
}

fn render_create_picture_request(
    byte_order: XByteOrder,
    picture: u32,
    drawable: u32,
    format: u32,
    values: &[(u32, u32)],
) -> Vec<u8> {
    let mut out = vec![X_RENDER_MAJOR_OPCODE, X_RENDER_CREATE_PICTURE_MINOR_OPCODE];
    push_u16(&mut out, byte_order, (5 + values.len()) as u16);
    push_u32(&mut out, byte_order, picture);
    push_u32(&mut out, byte_order, drawable);
    push_u32(&mut out, byte_order, format);
    let mask = values.iter().fold(0u32, |mask, (bit, _)| mask | (1 << bit));
    push_u32(&mut out, byte_order, mask);
    let mut sorted = values.to_vec();
    sorted.sort_by_key(|(bit, _)| *bit);
    for (_, value) in sorted {
        push_u32(&mut out, byte_order, value);
    }
    out
}

fn render_free_picture_request(byte_order: XByteOrder, picture: u32) -> Vec<u8> {
    let mut out = vec![X_RENDER_MAJOR_OPCODE, X_RENDER_FREE_PICTURE_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 2);
    push_u32(&mut out, byte_order, picture);
    out
}

fn render_fill_rectangles_request(
    byte_order: XByteOrder,
    op: u8,
    picture: u32,
    color: [u16; 4],
    rectangles: &[Rect],
) -> Vec<u8> {
    let mut out = vec![X_RENDER_MAJOR_OPCODE, X_RENDER_FILL_RECTANGLES_MINOR_OPCODE];
    push_u16(&mut out, byte_order, (5 + rectangles.len() * 2) as u16);
    out.push(op);
    out.extend_from_slice(&[0, 0, 0]);
    push_u32(&mut out, byte_order, picture);
    for channel in color {
        push_u16(&mut out, byte_order, channel);
    }
    for rectangle in rectangles {
        push_i16(&mut out, byte_order, rectangle.x as i16);
        push_i16(&mut out, byte_order, rectangle.y as i16);
        push_u16(&mut out, byte_order, rectangle.width as u16);
        push_u16(&mut out, byte_order, rectangle.height as u16);
    }
    out
}

fn render_set_picture_clip_rectangles_request(
    byte_order: XByteOrder,
    picture: u32,
    clip_x_origin: i16,
    clip_y_origin: i16,
    rectangles: &[Rect],
) -> Vec<u8> {
    let mut out = vec![
        X_RENDER_MAJOR_OPCODE,
        X_RENDER_SET_PICTURE_CLIP_RECTANGLES_MINOR_OPCODE,
    ];
    push_u16(&mut out, byte_order, (3 + rectangles.len() * 2) as u16);
    push_u32(&mut out, byte_order, picture);
    push_i16(&mut out, byte_order, clip_x_origin);
    push_i16(&mut out, byte_order, clip_y_origin);
    for rectangle in rectangles {
        push_i16(&mut out, byte_order, rectangle.x as i16);
        push_i16(&mut out, byte_order, rectangle.y as i16);
        push_u16(&mut out, byte_order, rectangle.width as u16);
        push_u16(&mut out, byte_order, rectangle.height as u16);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn render_composite_request(
    byte_order: XByteOrder,
    op: u8,
    source: u32,
    mask: u32,
    destination: u32,
    source_x: i16,
    source_y: i16,
    mask_x: i16,
    mask_y: i16,
    destination_x: i16,
    destination_y: i16,
    width: u16,
    height: u16,
) -> Vec<u8> {
    let mut out = vec![X_RENDER_MAJOR_OPCODE, X_RENDER_COMPOSITE_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 9);
    out.push(op);
    out.extend_from_slice(&[0, 0, 0]);
    push_u32(&mut out, byte_order, source);
    push_u32(&mut out, byte_order, mask);
    push_u32(&mut out, byte_order, destination);
    push_i16(&mut out, byte_order, source_x);
    push_i16(&mut out, byte_order, source_y);
    push_i16(&mut out, byte_order, mask_x);
    push_i16(&mut out, byte_order, mask_y);
    push_i16(&mut out, byte_order, destination_x);
    push_i16(&mut out, byte_order, destination_y);
    push_u16(&mut out, byte_order, width);
    push_u16(&mut out, byte_order, height);
    out
}

fn render_create_glyph_set_request(byte_order: XByteOrder, glyphset: u32, format: u32) -> Vec<u8> {
    let mut out = vec![
        X_RENDER_MAJOR_OPCODE,
        X_RENDER_CREATE_GLYPH_SET_MINOR_OPCODE,
    ];
    push_u16(&mut out, byte_order, 3);
    push_u32(&mut out, byte_order, glyphset);
    push_u32(&mut out, byte_order, format);
    out
}

fn render_reference_glyph_set_request(
    byte_order: XByteOrder,
    glyphset: u32,
    existing: u32,
) -> Vec<u8> {
    let mut out = vec![
        X_RENDER_MAJOR_OPCODE,
        X_RENDER_REFERENCE_GLYPH_SET_MINOR_OPCODE,
    ];
    push_u16(&mut out, byte_order, 3);
    push_u32(&mut out, byte_order, glyphset);
    push_u32(&mut out, byte_order, existing);
    out
}

fn render_free_glyph_set_request(byte_order: XByteOrder, glyphset: u32) -> Vec<u8> {
    let mut out = vec![X_RENDER_MAJOR_OPCODE, X_RENDER_FREE_GLYPH_SET_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 2);
    push_u32(&mut out, byte_order, glyphset);
    out
}

/// One glyph for `render_add_glyphs_request`: identifier, `[width, height]`,
/// `[x, y, off_x, off_y]`, and already-padded image bytes.
type TestGlyph = (u32, [u16; 2], [i16; 4], Vec<u8>);

/// `AddGlyphs` for glyphs whose image bytes are supplied already padded.
fn render_add_glyphs_request(
    byte_order: XByteOrder,
    glyphset: u32,
    glyphs: &[TestGlyph],
) -> Vec<u8> {
    let mut out = vec![X_RENDER_MAJOR_OPCODE, X_RENDER_ADD_GLYPHS_MINOR_OPCODE];
    let data_len: usize = glyphs.iter().map(|(_, _, _, data)| data.len()).sum();
    let len_units = (12 + glyphs.len() * 16 + data_len).div_ceil(4);
    push_u16(&mut out, byte_order, len_units as u16);
    push_u32(&mut out, byte_order, glyphset);
    push_u32(&mut out, byte_order, glyphs.len() as u32);
    for (id, _, _, _) in glyphs {
        push_u32(&mut out, byte_order, *id);
    }
    for (_, size, offsets, _) in glyphs {
        push_u16(&mut out, byte_order, size[0]);
        push_u16(&mut out, byte_order, size[1]);
        for offset in offsets {
            push_i16(&mut out, byte_order, *offset);
        }
    }
    for (_, _, _, data) in glyphs {
        out.extend_from_slice(data);
    }
    out
}

/// `CompositeGlyphs8` with one element: a delta and a run of glyph ids.
#[allow(clippy::too_many_arguments)]
fn render_composite_glyphs8_request(
    byte_order: XByteOrder,
    op: u8,
    source: u32,
    destination: u32,
    mask_format: u32,
    glyphset: u32,
    source_x: i16,
    source_y: i16,
    delta: (i16, i16),
    ids: &[u8],
) -> Vec<u8> {
    let mut out = vec![
        X_RENDER_MAJOR_OPCODE,
        X_RENDER_COMPOSITE_GLYPHS_8_MINOR_OPCODE,
    ];
    let padded = ids.len().next_multiple_of(4);
    push_u16(&mut out, byte_order, ((28 + 8 + padded) / 4) as u16);
    out.push(op);
    out.extend_from_slice(&[0, 0, 0]);
    push_u32(&mut out, byte_order, source);
    push_u32(&mut out, byte_order, destination);
    push_u32(&mut out, byte_order, mask_format);
    push_u32(&mut out, byte_order, glyphset);
    push_i16(&mut out, byte_order, source_x);
    push_i16(&mut out, byte_order, source_y);
    out.push(ids.len() as u8);
    out.extend_from_slice(&[0, 0, 0]);
    push_i16(&mut out, byte_order, delta.0);
    push_i16(&mut out, byte_order, delta.1);
    out.extend_from_slice(ids);
    out.resize(out.len() + (padded - ids.len()), 0);
    out
}

fn render_create_cursor_request(
    byte_order: XByteOrder,
    cursor: u32,
    source: u32,
    hotspot_x: u16,
    hotspot_y: u16,
) -> Vec<u8> {
    let mut out = vec![X_RENDER_MAJOR_OPCODE, X_RENDER_CREATE_CURSOR_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 4);
    push_u32(&mut out, byte_order, cursor);
    push_u32(&mut out, byte_order, source);
    push_u16(&mut out, byte_order, hotspot_x);
    push_u16(&mut out, byte_order, hotspot_y);
    out
}

fn free_cursor_request(byte_order: XByteOrder, cursor: u32) -> Vec<u8> {
    let mut out = vec![95, 0];
    push_u16(&mut out, byte_order, 2);
    push_u32(&mut out, byte_order, cursor);
    out
}

fn xfixes_combine_region_request(
    byte_order: XByteOrder,
    minor_opcode: u8,
    source: u32,
    other: u32,
    destination: u32,
) -> Vec<u8> {
    let mut out = vec![X_XFIXES_MAJOR_OPCODE, minor_opcode];
    if minor_opcode == X_XFIXES_COPY_REGION_MINOR_OPCODE {
        push_u16(&mut out, byte_order, 3);
        push_u32(&mut out, byte_order, source);
        push_u32(&mut out, byte_order, destination);
    } else {
        push_u16(&mut out, byte_order, 4);
        push_u32(&mut out, byte_order, source);
        push_u32(&mut out, byte_order, other);
        push_u32(&mut out, byte_order, destination);
    }
    out
}

fn xfixes_invert_region_request(
    byte_order: XByteOrder,
    source: u32,
    bounds: Rect,
    destination: u32,
) -> Vec<u8> {
    let mut out = vec![X_XFIXES_MAJOR_OPCODE, X_XFIXES_INVERT_REGION_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 5);
    push_u32(&mut out, byte_order, source);
    push_i16(&mut out, byte_order, bounds.x as i16);
    push_i16(&mut out, byte_order, bounds.y as i16);
    push_u16(&mut out, byte_order, bounds.width as u16);
    push_u16(&mut out, byte_order, bounds.height as u16);
    push_u32(&mut out, byte_order, destination);
    out
}

fn xfixes_translate_region_request(
    byte_order: XByteOrder,
    region: u32,
    dx: i16,
    dy: i16,
) -> Vec<u8> {
    let mut out = vec![
        X_XFIXES_MAJOR_OPCODE,
        X_XFIXES_TRANSLATE_REGION_MINOR_OPCODE,
    ];
    push_u16(&mut out, byte_order, 3);
    push_u32(&mut out, byte_order, region);
    push_i16(&mut out, byte_order, dx);
    push_i16(&mut out, byte_order, dy);
    out
}

fn xfixes_region_extents_request(byte_order: XByteOrder, source: u32, destination: u32) -> Vec<u8> {
    let mut out = vec![X_XFIXES_MAJOR_OPCODE, X_XFIXES_REGION_EXTENTS_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 3);
    push_u32(&mut out, byte_order, source);
    push_u32(&mut out, byte_order, destination);
    out
}

fn xfixes_fetch_region_request(byte_order: XByteOrder, region: u32) -> Vec<u8> {
    let mut out = vec![X_XFIXES_MAJOR_OPCODE, X_XFIXES_FETCH_REGION_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 2);
    push_u32(&mut out, byte_order, region);
    out
}

fn xfixes_minor_request(byte_order: XByteOrder, minor_opcode: u8) -> Vec<u8> {
    let mut out = vec![X_XFIXES_MAJOR_OPCODE, minor_opcode];
    push_u16(&mut out, byte_order, 1);
    out
}

#[allow(clippy::too_many_arguments)]
fn shape_rectangles_request(
    byte_order: XByteOrder,
    op: u8,
    kind: u8,
    ordering: u8,
    destination: u32,
    x_offset: i16,
    y_offset: i16,
    rects: &[Rect],
) -> Vec<u8> {
    let mut out = vec![X_SHAPE_MAJOR_OPCODE, X_SHAPE_RECTANGLES_MINOR_OPCODE];
    push_u16(&mut out, byte_order, (4 + rects.len() * 2) as u16);
    out.push(op);
    out.push(kind);
    out.push(ordering);
    out.push(0);
    push_u32(&mut out, byte_order, destination);
    push_i16(&mut out, byte_order, x_offset);
    push_i16(&mut out, byte_order, y_offset);
    for rect in rects {
        push_i16(&mut out, byte_order, rect.x as i16);
        push_i16(&mut out, byte_order, rect.y as i16);
        push_u16(&mut out, byte_order, rect.width as u16);
        push_u16(&mut out, byte_order, rect.height as u16);
    }
    out
}

fn shape_mask_request(
    byte_order: XByteOrder,
    op: u8,
    kind: u8,
    destination: u32,
    x_offset: i16,
    y_offset: i16,
    source: u32,
) -> Vec<u8> {
    let mut out = vec![X_SHAPE_MAJOR_OPCODE, X_SHAPE_MASK_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 5);
    out.push(op);
    out.push(kind);
    push_u16(&mut out, byte_order, 0);
    push_u32(&mut out, byte_order, destination);
    push_i16(&mut out, byte_order, x_offset);
    push_i16(&mut out, byte_order, y_offset);
    push_u32(&mut out, byte_order, source);
    out
}

#[allow(clippy::too_many_arguments)]
fn shape_combine_request(
    byte_order: XByteOrder,
    op: u8,
    kind: u8,
    source_kind: u8,
    destination: u32,
    x_offset: i16,
    y_offset: i16,
    source: u32,
) -> Vec<u8> {
    let mut out = vec![X_SHAPE_MAJOR_OPCODE, X_SHAPE_COMBINE_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 5);
    out.push(op);
    out.push(kind);
    out.push(source_kind);
    out.push(0);
    push_u32(&mut out, byte_order, destination);
    push_i16(&mut out, byte_order, x_offset);
    push_i16(&mut out, byte_order, y_offset);
    push_u32(&mut out, byte_order, source);
    out
}

fn shape_offset_request(
    byte_order: XByteOrder,
    kind: u8,
    destination: u32,
    x_offset: i16,
    y_offset: i16,
) -> Vec<u8> {
    let mut out = vec![X_SHAPE_MAJOR_OPCODE, X_SHAPE_OFFSET_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 4);
    out.push(kind);
    out.extend_from_slice(&[0, 0, 0]);
    push_u32(&mut out, byte_order, destination);
    push_i16(&mut out, byte_order, x_offset);
    push_i16(&mut out, byte_order, y_offset);
    out
}

fn shape_query_extents_request(byte_order: XByteOrder, window: u32) -> Vec<u8> {
    let mut out = vec![X_SHAPE_MAJOR_OPCODE, X_SHAPE_QUERY_EXTENTS_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 2);
    push_u32(&mut out, byte_order, window);
    out
}

fn shape_select_input_request(byte_order: XByteOrder, window: u32, enable: bool) -> Vec<u8> {
    let mut out = vec![X_SHAPE_MAJOR_OPCODE, X_SHAPE_SELECT_INPUT_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 3);
    push_u32(&mut out, byte_order, window);
    out.push(u8::from(enable));
    out.extend_from_slice(&[0, 0, 0]);
    out
}

fn shape_input_selected_request(byte_order: XByteOrder, window: u32) -> Vec<u8> {
    let mut out = vec![X_SHAPE_MAJOR_OPCODE, X_SHAPE_INPUT_SELECTED_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 2);
    push_u32(&mut out, byte_order, window);
    out
}

fn shape_get_rectangles_request(byte_order: XByteOrder, window: u32, kind: u8) -> Vec<u8> {
    let mut out = vec![X_SHAPE_MAJOR_OPCODE, X_SHAPE_GET_RECTANGLES_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 3);
    push_u32(&mut out, byte_order, window);
    out.push(kind);
    out.extend_from_slice(&[0, 0, 0]);
    out
}

fn shape_minor_request(byte_order: XByteOrder, minor_opcode: u8) -> Vec<u8> {
    let mut out = vec![X_SHAPE_MAJOR_OPCODE, minor_opcode];
    push_u16(&mut out, byte_order, 1);
    out
}

/// `PutImage` at an explicit depth, for uploading the depth-1 bitmap a
/// SHAPE mask is read from.
fn put_image_request_at_depth(
    byte_order: XByteOrder,
    depth: u8,
    drawable: u32,
    gc: u32,
    width: u16,
    height: u16,
    data: &[u8],
) -> Vec<u8> {
    let mut out = vec![72, 2];
    let len_units = (24 + padded_len_for_test(data.len())) / 4;
    push_u16(&mut out, byte_order, len_units as u16);
    push_u32(&mut out, byte_order, drawable);
    push_u32(&mut out, byte_order, gc);
    push_u16(&mut out, byte_order, width);
    push_u16(&mut out, byte_order, height);
    push_i16(&mut out, byte_order, 0);
    push_i16(&mut out, byte_order, 0);
    out.push(0);
    out.push(depth);
    push_u16(&mut out, byte_order, 0);
    out.extend_from_slice(data);
    pad_to_four(&mut out);
    out
}

fn render_set_picture_transform_request(
    byte_order: XByteOrder,
    picture: u32,
    matrix: [i32; 9],
) -> Vec<u8> {
    let mut out = vec![
        X_RENDER_MAJOR_OPCODE,
        X_RENDER_SET_PICTURE_TRANSFORM_MINOR_OPCODE,
    ];
    push_u16(&mut out, byte_order, 11);
    push_u32(&mut out, byte_order, picture);
    for entry in matrix {
        push_u32(&mut out, byte_order, entry as u32);
    }
    out
}

fn render_query_filters_request(byte_order: XByteOrder, drawable: u32) -> Vec<u8> {
    let mut out = vec![X_RENDER_MAJOR_OPCODE, X_RENDER_QUERY_FILTERS_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 2);
    push_u32(&mut out, byte_order, drawable);
    out
}

fn render_set_picture_filter_request(
    byte_order: XByteOrder,
    picture: u32,
    name: &str,
    params: &[i32],
) -> Vec<u8> {
    let mut out = vec![
        X_RENDER_MAJOR_OPCODE,
        X_RENDER_SET_PICTURE_FILTER_MINOR_OPCODE,
    ];
    let padded_name = (12 + name.len()).next_multiple_of(4);
    let len_units = (padded_name + params.len() * 4) / 4;
    push_u16(&mut out, byte_order, len_units as u16);
    push_u32(&mut out, byte_order, picture);
    push_u16(&mut out, byte_order, name.len() as u16);
    push_u16(&mut out, byte_order, 0);
    out.extend_from_slice(name.as_bytes());
    while out.len() % 4 != 0 {
        out.push(0);
    }
    for param in params {
        push_u32(&mut out, byte_order, *param as u32);
    }
    out
}

/// A trapezoid in the 16.16 fixed point the wire carries.
type TestTrapezoid = (i32, i32, (i32, i32), (i32, i32), (i32, i32), (i32, i32));

fn fixed(value: i32) -> i32 {
    value * 65536
}

#[allow(clippy::too_many_arguments)]
fn render_trapezoids_request(
    byte_order: XByteOrder,
    op: u8,
    source: u32,
    destination: u32,
    mask_format: u32,
    source_x: i16,
    source_y: i16,
    traps: &[TestTrapezoid],
) -> Vec<u8> {
    let mut out = vec![X_RENDER_MAJOR_OPCODE, X_RENDER_TRAPEZOIDS_MINOR_OPCODE];
    push_u16(&mut out, byte_order, (6 + traps.len() * 10) as u16);
    out.push(op);
    out.extend_from_slice(&[0, 0, 0]);
    push_u32(&mut out, byte_order, source);
    push_u32(&mut out, byte_order, destination);
    push_u32(&mut out, byte_order, mask_format);
    push_i16(&mut out, byte_order, source_x);
    push_i16(&mut out, byte_order, source_y);
    for (top, bottom, l1, l2, r1, r2) in traps {
        for value in [
            *top, *bottom, l1.0, l1.1, l2.0, l2.1, r1.0, r1.1, r2.0, r2.1,
        ] {
            push_u32(&mut out, byte_order, value as u32);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn render_triangles_request(
    byte_order: XByteOrder,
    minor_opcode: u8,
    op: u8,
    source: u32,
    destination: u32,
    points: &[(i32, i32)],
) -> Vec<u8> {
    let mut out = vec![X_RENDER_MAJOR_OPCODE, minor_opcode];
    let words = if minor_opcode == X_RENDER_TRIANGLES_MINOR_OPCODE {
        6 + points.len() / 3 * 6
    } else {
        6 + points.len() * 2
    };
    push_u16(&mut out, byte_order, words as u16);
    out.push(op);
    out.extend_from_slice(&[0, 0, 0]);
    push_u32(&mut out, byte_order, source);
    push_u32(&mut out, byte_order, destination);
    push_u32(&mut out, byte_order, 0);
    push_i16(&mut out, byte_order, 0);
    push_i16(&mut out, byte_order, 0);
    for (x, y) in points {
        push_u32(&mut out, byte_order, *x as u32);
        push_u32(&mut out, byte_order, *y as u32);
    }
    out
}

fn render_create_solid_fill_request(
    byte_order: XByteOrder,
    picture: u32,
    color: [u16; 4],
) -> Vec<u8> {
    let mut out = vec![
        X_RENDER_MAJOR_OPCODE,
        X_RENDER_CREATE_SOLID_FILL_MINOR_OPCODE,
    ];
    push_u16(&mut out, byte_order, 4);
    push_u32(&mut out, byte_order, picture);
    for channel in color {
        push_u16(&mut out, byte_order, channel);
    }
    out
}

/// A linear gradient from `p1` to `p2` with `(position, colour)` stops.
fn render_create_linear_gradient_request(
    byte_order: XByteOrder,
    picture: u32,
    p1: (i32, i32),
    p2: (i32, i32),
    stops: &[(i32, [u16; 4])],
) -> Vec<u8> {
    let mut out = vec![
        X_RENDER_MAJOR_OPCODE,
        X_RENDER_CREATE_LINEAR_GRADIENT_MINOR_OPCODE,
    ];
    push_u16(&mut out, byte_order, (7 + stops.len() * 3) as u16);
    push_u32(&mut out, byte_order, picture);
    for value in [p1.0, p1.1, p2.0, p2.1] {
        push_u32(&mut out, byte_order, value as u32);
    }
    push_u32(&mut out, byte_order, stops.len() as u32);
    for (position, _) in stops {
        push_u32(&mut out, byte_order, *position as u32);
    }
    for (_, color) in stops {
        for channel in color {
            push_u16(&mut out, byte_order, *channel);
        }
    }
    out
}

fn render_change_picture_request(
    byte_order: XByteOrder,
    picture: u32,
    values: &[(u32, u32)],
) -> Vec<u8> {
    let mut out = vec![X_RENDER_MAJOR_OPCODE, X_RENDER_CHANGE_PICTURE_MINOR_OPCODE];
    push_u16(&mut out, byte_order, (3 + values.len()) as u16);
    push_u32(&mut out, byte_order, picture);
    let mask = values.iter().fold(0u32, |mask, (bit, _)| mask | (1 << bit));
    push_u32(&mut out, byte_order, mask);
    let mut sorted = values.to_vec();
    sorted.sort_by_key(|(bit, _)| *bit);
    for (_, value) in sorted {
        push_u32(&mut out, byte_order, value);
    }
    out
}

fn xfixes_create_region_from_request(
    byte_order: XByteOrder,
    minor_opcode: u8,
    region: u32,
    source: u32,
    kind: u8,
) -> Vec<u8> {
    let mut out = vec![X_XFIXES_MAJOR_OPCODE, minor_opcode];
    let from_window = minor_opcode == X_XFIXES_CREATE_REGION_FROM_WINDOW_MINOR_OPCODE;
    push_u16(&mut out, byte_order, if from_window { 4 } else { 3 });
    push_u32(&mut out, byte_order, region);
    push_u32(&mut out, byte_order, source);
    if from_window {
        out.push(kind);
        out.extend_from_slice(&[0, 0, 0]);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn xfixes_expand_region_request(
    byte_order: XByteOrder,
    source: u32,
    destination: u32,
    left: u16,
    right: u16,
    top: u16,
    bottom: u16,
) -> Vec<u8> {
    let mut out = vec![X_XFIXES_MAJOR_OPCODE, X_XFIXES_EXPAND_REGION_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 5);
    push_u32(&mut out, byte_order, source);
    push_u32(&mut out, byte_order, destination);
    for value in [left, right, top, bottom] {
        push_u16(&mut out, byte_order, value);
    }
    out
}
