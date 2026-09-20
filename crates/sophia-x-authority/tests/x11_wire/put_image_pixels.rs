// Test through the wire boundary: the runtime accepts canonical pixels, not
// unvalidated client encodings. Readback checks bytes, not just transactions.
struct ImageUploadFixture {
    runtime: XAuthorityRuntime,
    atoms: XAtomTable,
    properties: XPropertyTable,
    order: XByteOrder,
    sequence: u16,
}

impl ImageUploadFixture {
    const NS: NamespaceId = NamespaceId::from_raw(991);
    const WINDOW: u32 = 0x220001;
    const PIXMAP: u32 = 0x220002;
    const GC: u32 = 0x220003;

    fn new(order: XByteOrder, depth: u8) -> Self {
        let mut fixture = Self {
            runtime: XAuthorityRuntime::new(),
            atoms: XAtomTable::new(),
            properties: XPropertyTable::new(),
            order,
            sequence: 0,
        };
        fixture.send(&create_window_request(order, Self::WINDOW, 0, 0, 3, 2));
        fixture.send(&create_pixmap_request(
            order,
            depth,
            Self::PIXMAP,
            Self::WINDOW,
            3,
            2,
        ));
        fixture.send(&create_gc_values_request(
            order,
            Self::GC,
            Self::PIXMAP,
            3,
            u32::MAX,
            0x123456,
            0x654321,
            0,
            0,
        ));
        fixture
    }

    fn send(&mut self, bytes: &[u8]) -> XDispatchResult {
        self.sequence += 1;
        self.runtime.begin_dispatch();
        let request = decode_x11_core_request(
            context(Self::NS, u64::from(self.sequence), self.order),
            bytes,
        )
        .unwrap();
        dispatch_x11_wire_request(
            dispatch_context(Self::NS, self.sequence, self.order, bytes[0]),
            request,
            &mut self.runtime,
            &mut self.atoms,
            &mut self.properties,
        )
    }

    fn upload(
        &mut self,
        format: u8,
        depth: u8,
        pad: u8,
        x: i16,
        y: i16,
        bytes: &[u8],
    ) -> XDispatchResult {
        let mut request = put_image_request_with_format(
            self.order,
            format,
            Self::PIXMAP,
            Self::GC,
            PutImageGeometry {
                width: 3,
                height: 2,
                dst_x: x,
                dst_y: y,
            },
            bytes,
        );
        request[20] = pad;
        request[21] = depth;
        self.send(&request)
    }

    fn pixels(&self) -> Vec<u32> {
        self.runtime
            .drawable_image_region(
                Self::NS,
                XResourceId::new(u64::from(Self::PIXMAP), 1),
                Rect {
                    x: 0,
                    y: 0,
                    width: 3,
                    height: 2,
                },
            )
            .unwrap()
            .chunks_exact(4)
            .map(|p| u32::from_le_bytes(p.try_into().unwrap()) & 0x00ffffff)
            .collect()
    }
}

#[test]
fn put_image_zpixmap_decodes_both_orders_and_padded_depths() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        for depth in [1, 4, 8, 16, 24, 32] {
            let mut fixture = ImageUploadFixture::new(order, depth);
            let values: [u32; 6] = [1u32, 0, 1, 0, 1, 0].map(|bit| {
                if bit == 0 {
                    0
                } else {
                    match depth {
                        1 => 1,
                        4 => 13,
                        8 => 0xa5,
                        16 => 0x1234,
                        _ => 0x123456,
                    }
                }
            });
            let bpp = match depth {
                24 => 32,
                _ => depth,
            };
            let stride = (3 * usize::from(bpp)).div_ceil(32) * 4;
            let mut data = vec![0; stride * 2];
            for (i, value) in values.iter().enumerate() {
                let row = (i / 3) * stride;
                let x = i % 3;
                match bpp {
                    32 => {
                        let bytes = match order {
                            XByteOrder::LittleEndian => value.to_le_bytes(),
                            XByteOrder::BigEndian => value.to_be_bytes(),
                        };
                        data[row + x * 4..row + x * 4 + 4].copy_from_slice(&bytes);
                    }
                    16 => {
                        let bytes = match order {
                            XByteOrder::LittleEndian => (*value as u16).to_le_bytes(),
                            XByteOrder::BigEndian => (*value as u16).to_be_bytes(),
                        };
                        data[row + x * 2..row + x * 2 + 2].copy_from_slice(&bytes);
                    }
                    8 => data[row + x] = *value as u8,
                    bits => {
                        let bits = usize::from(bits);
                        let shift = match order {
                            XByteOrder::LittleEndian => (x % (8 / bits)) * bits,
                            XByteOrder::BigEndian => (8 / bits - 1 - x % (8 / bits)) * bits,
                        };
                        data[row + x * bits / 8] |= (*value as u8) << shift;
                    }
                }
            }
            assert!(
                fixture.upload(2, depth, 0, 0, 0, &data).outputs.is_empty(),
                "depth={depth} order={order:?}"
            );
            assert_eq!(fixture.pixels(), values, "depth={depth} order={order:?}");
        }
    }
}

#[test]
fn put_image_xy_planes_and_bitmap_use_left_padding_and_gc_colors() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        for format in [0, 1] {
            let mut fixture = ImageUploadFixture::new(order, 24);
            let planes = if format == 0 { 1 } else { 24 };
            let mut data = vec![0; planes * 2 * 4];
            let values = [0x123456u32, 0, 0x654321, 0, 0xabcdef, 0];
            for plane in 0..planes {
                for (i, value) in values.iter().enumerate() {
                    let bit = if format == 0 {
                        u8::from(*value != 0)
                    } else {
                        ((value >> (23 - plane)) & 1) as u8
                    };
                    let x = 7 + i % 3;
                    let shift = match order {
                        XByteOrder::LittleEndian => x % 8,
                        XByteOrder::BigEndian => 7 - x % 8,
                    };
                    data[(plane * 2 + i / 3) * 4 + x / 8] |= bit << shift;
                }
            }
            assert!(
                fixture
                    .upload(format, if format == 0 { 1 } else { 24 }, 7, 0, 0, &data)
                    .outputs
                    .is_empty()
            );
            let expected = values.map(|v| {
                if format == 1 {
                    v
                } else if v == 0 {
                    0x654321
                } else {
                    0x123456
                }
            });
            assert_eq!(fixture.pixels(), expected);
        }
    }
}

#[test]
fn put_image_negative_destination_and_gc_mask_preserve_other_pixels() {
    let order = XByteOrder::LittleEndian;
    let mut fixture = ImageUploadFixture::new(order, 24);
    let values = [
        0x123456u32,
        0xabcdef,
        0x112233,
        0x445566,
        0x778899,
        0xaabbcc,
    ];
    let bytes: Vec<_> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    assert!(fixture.upload(2, 24, 0, -1, -1, &bytes).outputs.is_empty());
    assert_eq!(fixture.pixels(), [0x778899, 0xaabbcc, 0, 0, 0, 0]);
    // GXxor and a green-only mask must preserve all other channels.
    let gc = ImageUploadFixture::GC + 1;
    fixture.send(&create_gc_values_request(
        order,
        gc,
        ImageUploadFixture::PIXMAP,
        6,
        0x00ff00,
        0,
        0,
        0,
        0,
    ));
    let mut request = put_image_request(
        order,
        ImageUploadFixture::PIXMAP,
        gc,
        PutImageGeometry {
            width: 3,
            height: 2,
            dst_x: 0,
            dst_y: 0,
        },
        &bytes,
    );
    assert!(fixture.send(&request).outputs.is_empty());
    let expected = [0x778899u32, 0xaabbcc, 0, 0, 0, 0]
        .into_iter()
        .zip(values)
        .map(|(old, v)| old ^ (v & 0xff00))
        .collect::<Vec<_>>();
    assert_eq!(fixture.pixels(), expected);
    // Reject the complete malformed request before touching backing storage.
    request.truncate(request.len() - 4);
    let length = (request.len() / 4) as u16;
    request[2..4].copy_from_slice(&length.to_le_bytes());
    let rejected = fixture.send(&request);
    assert_eq!(
        rejected.encoded_outputs(order)[0][1],
        XErrorCode::BadLength.wire_code()
    );
    assert_eq!(fixture.pixels(), expected);
    assert!(fixture.runtime.take_cpu_buffer_updates().is_empty());
    // A left-pad on a ZPixmap upload is a Match error, as the protocol has
    // it, not a Value error.
    assert_eq!(
        fixture
            .upload(2, 24, 1, 0, 0, &bytes)
            .encoded_outputs(order)[0][1],
        XErrorCode::BadMatch.wire_code()
    );
    assert_eq!(
        fixture
            .upload(2, 16, 0, 0, 0, &bytes)
            .encoded_outputs(order)[0][1],
        XErrorCode::BadMatch.wire_code()
    );
    assert_eq!(fixture.pixels(), expected);
}
