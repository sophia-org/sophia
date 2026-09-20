use sophia_protocol::{NamespaceId, Rect, TransactionId};
use sophia_x_authority::*;

const PIXMAP: XResourceId = XResourceId::new(0x100010, 1);
const GC: XResourceId = XResourceId::new(0x100011, 1);
const COLOR: u32 = 0x00654321;

struct Client {
    runtime: XAuthorityRuntime,
    atoms: XAtomTable,
    properties: XPropertyTable,
    sequence: u16,
    depth: u8,
}

impl Client {
    fn new() -> Self {
        Self::with_depth(24)
    }

    fn with_depth(depth: u8) -> Self {
        let mut client = Self {
            runtime: XAuthorityRuntime::new(),
            atoms: XAtomTable::new(),
            properties: XPropertyTable::new(),
            sequence: 0,
            depth,
        };
        client.accept(
            53,
            XWireRequest::CreatePixmap {
                depth,
                pixmap: PIXMAP,
                drawable: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                width: 3,
                height: 3,
            },
        );
        client.accept(
            55,
            XWireRequest::CreateGraphicsContext {
                gc: GC,
                drawable: PIXMAP,
                values: XGraphicsContextValues {
                    foreground: COLOR,
                    ..Default::default()
                },
            },
        );
        client
    }

    fn send(&mut self, opcode: u8, request: XWireRequest) -> XDispatchResult {
        self.sequence += 1;
        dispatch_x11_wire_request(
            XDispatchContext {
                byte_order: XByteOrder::LittleEndian,
                namespace: NamespaceId::from_raw(71),
                transaction: TransactionId::from_raw(u64::from(self.sequence)),
                sequence: self.sequence,
                major_opcode: opcode,
                client_id: 1,
                injection: XTestAdmission::Absent,
            },
            request,
            &mut self.runtime,
            &mut self.atoms,
            &mut self.properties,
        )
    }

    fn accept(&mut self, opcode: u8, request: XWireRequest) {
        let result = self.send(opcode, request);
        assert!(result.outputs.is_empty(), "{result:?}");
        if let Some(response) = result.response {
            assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
            assert!(
                response.transactions.is_empty(),
                "pixmap drawing must not present a window"
            );
        }
    }

    fn pixels(&mut self) -> Vec<u32> {
        let result = self.send(
            73,
            XWireRequest::GetImage {
                format: 2,
                drawable: PIXMAP,
                x: 0,
                y: 0,
                width: 3,
                height: 3,
                plane_mask: u32::MAX,
            },
        );
        match result.outputs.as_slice() {
            [XClientOutput::Reply(XClientReply::GetImage { depth, data, .. })] => {
                assert_eq!(*depth, self.depth);
                assert_eq!(data.len(), 36);
                data.chunks_exact(4)
                    .map(|pixel| u32::from_le_bytes(pixel.try_into().unwrap()))
                    .collect()
            }
            other => panic!("unexpected GetImage result: {other:?}"),
        }
    }
}

fn fill(gc: XResourceId, rect: Rect) -> XWireRequest {
    XWireRequest::PolyFillRectangle {
        drawable: PIXMAP,
        gc,
        rectangles: vec![rect],
    }
}

#[test]
fn a_pixmap_fill_changes_only_the_requested_pixels() {
    let mut client = Client::new();
    assert_eq!(client.pixels(), vec![0; 9]);
    client.accept(
        70,
        fill(
            GC,
            Rect {
                x: 0,
                y: 0,
                width: 3,
                height: 3,
            },
        ),
    );
    assert_eq!(client.pixels(), vec![COLOR; 9]);
    client.accept(
        56,
        XWireRequest::ChangeGraphicsContext {
            gc: GC,
            value_mask: 1 << 2,
            values: XGraphicsContextValues {
                foreground: 0x00abcdef,
                ..Default::default()
            },
        },
    );
    client.accept(
        70,
        fill(
            GC,
            Rect {
                x: 1,
                y: 1,
                width: 1,
                height: 1,
            },
        ),
    );
    let mut expected = vec![COLOR; 9];
    expected[4] = 0x00abcdef;
    assert_eq!(client.pixels(), expected);
}

#[test]
fn pixmap_lines_and_rectangle_outlines_reach_the_cpu_store() {
    let mut client = Client::new();
    client.accept(
        67,
        XWireRequest::PolyRectangle {
            drawable: PIXMAP,
            gc: GC,
            rectangles: vec![Rect {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            }],
        },
    );
    assert_eq!(
        client.pixels(),
        [COLOR, COLOR, COLOR, COLOR, 0, COLOR, COLOR, COLOR, COLOR]
    );
    client.accept(
        65,
        XWireRequest::PolyLine {
            drawable: PIXMAP,
            gc: GC,
            points: vec![XPoint { x: 0, y: 1 }, XPoint { x: 2, y: 1 }],
        },
    );
    assert_eq!(client.pixels(), vec![COLOR; 9]);
}

#[test]
fn invalid_graphics_contexts_cannot_change_pixmap_pixels() {
    let mut client = Client::new();
    let missing_gc = XResourceId::new(0x100099, 1);
    let result = client.send(
        70,
        fill(
            missing_gc,
            Rect {
                x: 0,
                y: 0,
                width: 3,
                height: 3,
            },
        ),
    );
    assert!(
        matches!(result.outputs.as_slice(), [XClientOutput::Error(error)] if error.code == XErrorCode::BadGraphicsContext)
    );
    assert_eq!(client.pixels(), vec![0; 9]);
    let foreign_pixmap = XResourceId::new(0x100020, 1);
    let other_gc = XResourceId::new(0x100021, 1);
    client.accept(
        53,
        XWireRequest::CreatePixmap {
            depth: 32,
            pixmap: foreign_pixmap,
            drawable: PIXMAP,
            width: 3,
            height: 3,
        },
    );
    client.accept(
        55,
        XWireRequest::CreateGraphicsContext {
            gc: other_gc,
            drawable: foreign_pixmap,
            values: Default::default(),
        },
    );
    let result = client.send(
        70,
        fill(
            other_gc,
            Rect {
                x: 0,
                y: 0,
                width: 3,
                height: 3,
            },
        ),
    );
    assert!(
        matches!(result.outputs.as_slice(), [XClientOutput::Error(error)] if error.code == XErrorCode::BadMatch)
    );
    assert_eq!(client.pixels(), vec![0; 9]);
}

#[test]
fn depth_32_fill_preserves_alpha_and_applies_masked_inversion() {
    let mut client = Client::with_depth(32);
    client.accept(
        56,
        XWireRequest::ChangeGraphicsContext {
            gc: GC,
            value_mask: 1 << 2,
            values: XGraphicsContextValues {
                foreground: 0x87654321,
                ..Default::default()
            },
        },
    );
    client.accept(
        70,
        fill(
            GC,
            Rect {
                x: 0,
                y: 0,
                width: 3,
                height: 3,
            },
        ),
    );
    assert_eq!(client.pixels(), vec![0x87654321; 9]);
    client.accept(
        56,
        XWireRequest::ChangeGraphicsContext {
            gc: GC,
            value_mask: (1 << 0) | (1 << 1),
            values: XGraphicsContextValues {
                function: 10,
                plane_mask: 0xff000000,
                ..Default::default()
            },
        },
    );
    client.accept(
        70,
        fill(
            GC,
            Rect {
                x: 1,
                y: 1,
                width: 1,
                height: 1,
            },
        ),
    );
    let mut expected = vec![0x87654321; 9];
    expected[4] = 0x78654321;
    assert_eq!(client.pixels(), expected);
}

#[test]
fn depth_24_raster_inversion_changes_only_admitted_planes() {
    let mut client = Client::new();
    client.accept(
        70,
        fill(
            GC,
            Rect {
                x: 0,
                y: 0,
                width: 3,
                height: 3,
            },
        ),
    );
    client.accept(
        56,
        XWireRequest::ChangeGraphicsContext {
            gc: GC,
            value_mask: (1 << 0) | (1 << 1),
            values: XGraphicsContextValues {
                function: 10,
                plane_mask: 0xffff0000,
                ..Default::default()
            },
        },
    );
    assert_eq!(
        client
            .runtime
            .graphics_context_values(NamespaceId::from_raw(71), GC)
            .unwrap()
            .plane_mask,
        0x00ff0000
    );
    client.accept(
        70,
        fill(
            GC,
            Rect {
                x: 1,
                y: 1,
                width: 1,
                height: 1,
            },
        ),
    );
    let mut expected = vec![COLOR; 9];
    expected[4] = COLOR ^ 0x00ff0000;
    assert_eq!(client.pixels(), expected);
}
