// The errors the core cursor requests owe: a cursor's bitmaps are checked
// as the protocol and Xorg check them, and a name that is no cursor or no
// font is named as such rather than as a window.

fn bitmap_cursor_request(order: XByteOrder, cursor: u32, source: u32, mask: u32, hotspot: (u16, u16)) -> Vec<u8> {
    let mut out = vec![93, 0];
    push_u16(&mut out, order, 8);
    push_u32(&mut out, order, cursor);
    push_u32(&mut out, order, source);
    push_u32(&mut out, order, mask);
    for _ in 0..6 {
        push_u16(&mut out, order, 0);
    }
    push_u16(&mut out, order, hotspot.0);
    push_u16(&mut out, order, hotspot.1);
    out
}

fn glyph_cursor_request(order: XByteOrder, cursor: u32, font: u32, glyph: u16) -> Vec<u8> {
    let mut out = vec![94, 0];
    push_u16(&mut out, order, 8);
    push_u32(&mut out, order, cursor);
    push_u32(&mut out, order, font);
    push_u32(&mut out, order, 0);
    push_u16(&mut out, order, glyph);
    for _ in 0..7 {
        push_u16(&mut out, order, 0);
    }
    out
}

struct CursorFixture {
    runtime: XAuthorityRuntime,
    atoms: XAtomTable,
    properties: XPropertyTable,
    sequence: u16,
}

impl CursorFixture {
    const NS: NamespaceId = NamespaceId::from_raw(0x5301);
    const ORDER: XByteOrder = XByteOrder::LittleEndian;

    fn new() -> Self {
        let mut fixture = Self {
            runtime: XAuthorityRuntime::new(),
            atoms: XAtomTable::new(),
            properties: XPropertyTable::new(),
            sequence: 0,
        };
        assert_eq!(fixture.error(1, create_window_request(Self::ORDER, 0x7a0001, 0, 0, 20, 20)), None);
        for (pixmap, depth, size) in [(0x7a0002, 1, 16), (0x7a0003, 1, 8), (0x7a0004, 24, 16)] {
            let request = create_pixmap_request(Self::ORDER, depth, pixmap, 0x7a0001, size, size);
            assert_eq!(fixture.error(53, request), None);
        }
        assert_eq!(fixture.error(45, open_font_request(Self::ORDER, 0x7a0005, "cursor")), None);
        fixture
    }

    /// The error code the request answered, if it answered one.
    fn error(&mut self, op: u8, bytes: Vec<u8>) -> Option<XErrorCode> {
        self.sequence += 1;
        let request = decode_x11_core_request(context(Self::NS, u64::from(self.sequence), Self::ORDER), &bytes).unwrap();
        let result = dispatch_x11_wire_request(
            dispatch_context(Self::NS, self.sequence, Self::ORDER, op),
            request,
            &mut self.runtime,
            &mut self.atoms,
            &mut self.properties,
        );
        result.outputs.iter().find_map(|output| match output {
            XClientOutput::Error(error) => Some(error.code),
            _ => None,
        })
    }
}

#[test]
fn a_cursor_takes_depth_one_bitmaps_and_a_hotspot_on_them() {
    let mut fixture = CursorFixture::new();
    let order = CursorFixture::ORDER;
    assert_eq!(fixture.error(93, bitmap_cursor_request(order, 0x7a0010, 0x7a0002, 0, (16, 16))), None,
        "Xorg admits a hotspot one past each edge");
    assert_eq!(fixture.error(93, bitmap_cursor_request(order, 0x7a0011, 0x7a0002, 0, (17, 0))),
        Some(XErrorCode::BadMatch), "a hotspot off the source");
    assert_eq!(fixture.error(93, bitmap_cursor_request(order, 0x7a0012, 0x7a0004, 0, (0, 0))),
        Some(XErrorCode::BadMatch), "a colour pixmap is no bitmap");
    assert_eq!(fixture.error(93, bitmap_cursor_request(order, 0x7a0013, 0x7a0002, 0x7a0003, (0, 0))),
        Some(XErrorCode::BadMatch), "a mask of another size");
    assert_eq!(fixture.error(93, bitmap_cursor_request(order, 0x7a0014, 0x7a00ff, 0, (0, 0))),
        Some(XErrorCode::BadPixmap), "a source that is no pixmap");
}

#[test]
fn a_glyph_cursor_names_its_font_and_its_glyph() {
    let mut fixture = CursorFixture::new();
    let order = CursorFixture::ORDER;
    assert_eq!(fixture.error(94, glyph_cursor_request(order, 0x7a0020, 0x7a0005, 68)), None);
    assert_eq!(fixture.error(94, glyph_cursor_request(order, 0x7a0021, 0x7a00ff, 68)),
        Some(XErrorCode::BadFont), "a font that does not exist");
    assert_eq!(fixture.error(94, glyph_cursor_request(order, 0x7a0022, 0x7a0005, 0xfff0)),
        Some(XErrorCode::BadValue), "a glyph the font does not define");
}

#[test]
fn a_name_that_is_no_cursor_is_a_cursor_error() {
    let mut fixture = CursorFixture::new();
    let order = CursorFixture::ORDER;
    assert_eq!(fixture.error(96, recolor_cursor_request(order, 0x7a00ff)), Some(XErrorCode::BadCursor));
    assert_eq!(fixture.error(95, resource_request(order, 95, 0x7a00ff)), Some(XErrorCode::BadCursor));
    let mut attributes = vec![2, 0];
    push_u16(&mut attributes, order, 4);
    push_u32(&mut attributes, order, 0x7a0001);
    push_u32(&mut attributes, order, 1 << 14);
    push_u32(&mut attributes, order, 0x7a00ff);
    assert_eq!(fixture.error(2, attributes), Some(XErrorCode::BadCursor),
        "a window attribute naming no cursor");
}
