// The window attributes X refuses (t216): background and border attributes
// on an InputOnly window, or a nonzero border width, are Match errors; a
// background or border pixmap that names no pixmap is a Pixmap error, and
// one of another depth than the window a Match error.

fn input_only_request(order: XByteOrder, window: u32, parent: u32) -> Vec<u8> {
    let mut out = vec![1, 0];
    push_u16(&mut out, order, 8);
    push_u32(&mut out, order, window);
    push_u32(&mut out, order, parent);
    for value in [0u16, 0, 10, 10, 0, 2] {
        push_u16(&mut out, order, value);
    }
    push_u32(&mut out, order, 0);
    push_u32(&mut out, order, 0);
    out
}

fn attribute_request(order: XByteOrder, window: u32, bit: u32, value: u32) -> Vec<u8> {
    let mut out = vec![2, 0];
    push_u16(&mut out, order, 4);
    push_u32(&mut out, order, window);
    push_u32(&mut out, order, 1 << bit);
    push_u32(&mut out, order, value);
    out
}

#[test]
fn window_attributes_are_refused_as_x_refuses_them() {
    let mut fixture = CursorFixture::new();
    let order = CursorFixture::ORDER;
    // 0x7a0001 is an InputOutput window; 0x7a0002 a depth-one pixmap and
    // 0x7a0004 a depth-24 one.
    assert_eq!(fixture.error(1, input_only_request(order, 0x7a0040, 0x7a0001)), None);
    for (bit, value, what) in [
        (0, 0x7a0004, "a background pixmap"),
        (1, 0, "a background pixel"),
        (2, 0x7a0004, "a border pixmap"),
        (3, 0, "a border pixel"),
    ] {
        assert_eq!(
            fixture.error(2, attribute_request(order, 0x7a0040, bit, value)),
            Some(XErrorCode::BadMatch),
            "{what} on an InputOnly window"
        );
    }
    assert_eq!(
        fixture.error(12, configure_window_request(order, 0x7a0040, 1 << 4, &[1])),
        Some(XErrorCode::BadMatch),
        "a border width on an InputOnly window"
    );
    assert_eq!(fixture.error(2, attribute_request(order, 0x7a0001, 0, 0x7a00ff)), Some(XErrorCode::BadPixmap));
    assert_eq!(fixture.error(2, attribute_request(order, 0x7a0001, 0, 0x7a0002)), Some(XErrorCode::BadMatch));
    assert_eq!(fixture.error(2, attribute_request(order, 0x7a0001, 2, 0x7a00ff)), Some(XErrorCode::BadPixmap));
    assert_eq!(fixture.error(2, attribute_request(order, 0x7a0001, 2, 0x7a0002)), Some(XErrorCode::BadMatch));
    // What is valid stays valid.
    assert_eq!(fixture.error(2, attribute_request(order, 0x7a0001, 0, 0x7a0004)), None);
    assert_eq!(fixture.error(2, attribute_request(order, 0x7a0001, 2, 0x7a0004)), None);
    assert_eq!(fixture.error(2, attribute_request(order, 0x7a0001, 3, 0x123456)), None);
    assert_eq!(fixture.error(2, attribute_request(order, 0x7a0001, 0, 1)), None, "ParentRelative from the root");
}

/// CopyFromParent under an InputOnly parent makes an InputOnly window, so a
/// background or border pixel on it is refused as on any InputOnly window
/// (XTS XCreateSimpleWindow-10).
#[test]
fn a_copy_from_parent_window_under_input_only_refuses_its_pixels() {
    let mut fixture = CursorFixture::new();
    let order = CursorFixture::ORDER;
    assert_eq!(fixture.error(1, input_only_request(order, 0x7a0050, 0x7a0001)), None);
    let mut simple = vec![1, 0];
    push_u16(&mut simple, order, 10);
    push_u32(&mut simple, order, 0x7a0051);
    push_u32(&mut simple, order, 0x7a0050);
    for value in [0u16, 0, 5, 5, 0, 0] {
        push_u16(&mut simple, order, value);
    }
    push_u32(&mut simple, order, 0);
    push_u32(&mut simple, order, (1 << 1) | (1 << 3));
    push_u32(&mut simple, order, 0);
    push_u32(&mut simple, order, 1);
    assert_eq!(fixture.error(1, simple), Some(XErrorCode::BadMatch));
}
