// A set value-mask bit the protocol does not define is BadValue.
//
// WHAT THESE PROVE. CreateWindow, ChangeWindowAttributes, ConfigureWindow
// and CreateGC each take a value mask and a list; the protocol names which
// bits exist and says a set unused bit is a Value error. Before t167 the
// window requests counted the bit for the length and ignored it, so a
// request that carried the extra value was accepted in silence, and
// CreateGC refused it as a length fault. XTS5: CreateWindow 6,
// ChangeWindowAttributes 4, ConfigureWindow 4, CreateGC 6.

fn with_values(byte_order: XByteOrder, mut head: Vec<u8>, count: usize) -> Vec<u8> {
    for _ in 0..count {
        push_u32(&mut head, byte_order, 0);
    }
    let units = u16::try_from(head.len() / 4).unwrap();
    let length = match byte_order {
        XByteOrder::LittleEndian => units.to_le_bytes(),
        XByteOrder::BigEndian => units.to_be_bytes(),
    };
    head[2..4].copy_from_slice(&length);
    head
}

fn create_window_with_mask(byte_order: XByteOrder, mask: u32) -> Vec<u8> {
    let mut out = vec![1, 24, 0, 0];
    push_u32(&mut out, byte_order, X_SETUP_DEFAULT_RESOURCE_ID_BASE + 1);
    push_u32(&mut out, byte_order, X_SETUP_DEFAULT_ROOT);
    for value in [0u16, 0, 16, 16, 0, 1] {
        push_u16(&mut out, byte_order, value);
    }
    push_u32(&mut out, byte_order, 0);
    push_u32(&mut out, byte_order, mask);
    with_values(byte_order, out, mask.count_ones() as usize)
}

fn change_attributes_with_mask(byte_order: XByteOrder, mask: u32) -> Vec<u8> {
    let mut out = vec![2, 0, 0, 0];
    push_u32(&mut out, byte_order, X_SETUP_DEFAULT_ROOT);
    push_u32(&mut out, byte_order, mask);
    with_values(byte_order, out, mask.count_ones() as usize)
}

fn configure_with_mask(byte_order: XByteOrder, mask: u16) -> Vec<u8> {
    let mut out = vec![12, 0, 0, 0];
    push_u32(&mut out, byte_order, X_SETUP_DEFAULT_ROOT);
    push_u16(&mut out, byte_order, mask);
    push_u16(&mut out, byte_order, 0);
    with_values(byte_order, out, mask.count_ones() as usize)
}

fn create_gc_with_mask(byte_order: XByteOrder, mask: u32) -> Vec<u8> {
    let mut out = vec![55, 0, 0, 0];
    push_u32(&mut out, byte_order, X_SETUP_DEFAULT_RESOURCE_ID_BASE + 2);
    push_u32(&mut out, byte_order, X_SETUP_DEFAULT_ROOT);
    push_u32(&mut out, byte_order, mask);
    with_values(byte_order, out, mask.count_ones() as usize)
}

#[test]
fn a_set_unused_value_mask_bit_is_a_value_error_carrying_the_mask() {
    let namespace = NamespaceId::from_raw(1167);
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        // Each: the highest defined bit set alongside the first unused one,
        // with the values the mask announces, so only the bit is wrong.
        let cases: [(&str, Vec<u8>, u32); 4] = [
            ("CreateWindow", create_window_with_mask(byte_order, (1 << 14) | (1 << 15)), (1 << 14) | (1 << 15)),
            ("ChangeWindowAttributes", change_attributes_with_mask(byte_order, (1 << 11) | (1 << 15)), (1 << 11) | (1 << 15)),
            ("ConfigureWindow", configure_with_mask(byte_order, (1 << 6) | (1 << 7)), (1 << 6) | (1 << 7)),
            ("CreateGC", create_gc_with_mask(byte_order, (1 << 22) | (1 << 23)), (1 << 22) | (1 << 23)),
        ];
        for (name, request, mask) in cases {
            let decoded = decode_x11_core_request(context(namespace, 1, byte_order), &request);
            assert_eq!(
                decoded.err(),
                Some(XWireParseError::InvalidValue(mask)),
                "{byte_order:?}: {name} with an unused mask bit is BadValue carrying the mask"
            );
        }
        // The defined bits alone, with their values, still decode.
        for (name, request) in [
            ("CreateWindow", create_window_with_mask(byte_order, 1 << 14)),
            ("ChangeWindowAttributes", change_attributes_with_mask(byte_order, 1 << 11)),
            ("ConfigureWindow", configure_with_mask(byte_order, 1 << 6)),
            ("CreateGC", create_gc_with_mask(byte_order, 1 << 22)),
        ] {
            assert!(
                decode_x11_core_request(context(namespace, 1, byte_order), &request).is_ok(),
                "{byte_order:?}: {name} with its highest defined bit decodes"
            );
        }
    }
}
