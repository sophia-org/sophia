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
                server_time: 4_242,
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

const MASK: XResourceId = XResourceId::new(0x100012, 1);
const MASK_GC: XResourceId = XResourceId::new(0x100013, 1);
const SOURCE: XResourceId = XResourceId::new(0x100014, 1);
/// The pixels a clip mask admitting (0, 0) and (1, 1) lets through, in
/// GetImage order.
const DIAGONAL: [usize; 2] = [0, 4];

/// A 3x3 depth-24 pixmap whose graphics context clips to a depth-1 mask
/// holding only (0, 0) and (1, 1).
fn clipped_client() -> Client {
    let mut client = Client::new();
    client.accept(
        53,
        XWireRequest::CreatePixmap {
            depth: 1,
            pixmap: MASK,
            drawable: PIXMAP,
            width: 3,
            height: 3,
        },
    );
    client.accept(
        55,
        XWireRequest::CreateGraphicsContext {
            gc: MASK_GC,
            drawable: MASK,
            values: XGraphicsContextValues {
                foreground: 1,
                ..Default::default()
            },
        },
    );
    for (x, y) in [(0, 0), (1, 1)] {
        client.accept(
            70,
            XWireRequest::PolyFillRectangle {
                drawable: MASK,
                gc: MASK_GC,
                rectangles: vec![Rect {
                    x,
                    y,
                    width: 1,
                    height: 1,
                }],
            },
        );
    }
    client.accept(
        56,
        XWireRequest::ChangeGraphicsContext {
            gc: GC,
            value_mask: 1 << 19,
            values: XGraphicsContextValues {
                clip_mask: Some(MASK),
                ..Default::default()
            },
        },
    );
    client
}

fn only_admitted(color: u32) -> Vec<u32> {
    let mut expected = vec![0; 9];
    for index in DIAGONAL {
        expected[index] = color;
    }
    expected
}

fn every_row() -> Vec<(XPoint, XPoint)> {
    (0..3)
        .map(|y| (XPoint { x: 0, y }, XPoint { x: 2, y }))
        .collect()
}

#[test]
fn a_clip_mask_confines_a_fill() {
    let mut client = clipped_client();
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
    assert_eq!(client.pixels(), only_admitted(COLOR));
}

#[test]
fn a_clip_mask_confines_segments_at_its_origin() {
    let mut client = clipped_client();
    client.accept(
        66,
        XWireRequest::PolySegment {
            drawable: PIXMAP,
            gc: GC,
            segments: every_row(),
        },
    );
    assert_eq!(client.pixels(), only_admitted(COLOR));

    // The clip origin moves the mask, so (1, 0) and (2, 1) are what it
    // admits now; the pixels already drawn stay as they were.
    client.accept(
        56,
        XWireRequest::ChangeGraphicsContext {
            gc: GC,
            value_mask: (1 << 2) | (1 << 17) | (1 << 18),
            values: XGraphicsContextValues {
                foreground: 0x00abcdef,
                clip_x_origin: 1,
                ..Default::default()
            },
        },
    );
    client.accept(
        66,
        XWireRequest::PolySegment {
            drawable: PIXMAP,
            gc: GC,
            segments: every_row(),
        },
    );
    let mut expected = only_admitted(COLOR);
    expected[1] = 0x00abcdef;
    expected[5] = 0x00abcdef;
    assert_eq!(client.pixels(), expected);
}

#[test]
fn a_clip_mask_confines_a_polyline() {
    let mut client = clipped_client();
    client.accept(
        65,
        XWireRequest::PolyLine {
            drawable: PIXMAP,
            gc: GC,
            points: vec![
                XPoint { x: 0, y: 0 },
                XPoint { x: 2, y: 0 },
                XPoint { x: 2, y: 2 },
                XPoint { x: 0, y: 2 },
                XPoint { x: 0, y: 1 },
                XPoint { x: 1, y: 1 },
            ],
        },
    );
    assert_eq!(client.pixels(), only_admitted(COLOR));
}

#[test]
fn a_clip_mask_confines_a_rectangle_outline() {
    let mut client = clipped_client();
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
    // The outline covers eight pixels; of the mask's two, only (0, 0) is on it.
    let mut expected = vec![0; 9];
    expected[0] = COLOR;
    assert_eq!(client.pixels(), expected);
}

#[test]
fn a_clip_mask_confines_a_copy() {
    const SOURCE_GC: XResourceId = XResourceId::new(0x100015, 1);
    let mut client = clipped_client();
    client.accept(
        53,
        XWireRequest::CreatePixmap {
            depth: 24,
            pixmap: SOURCE,
            drawable: PIXMAP,
            width: 3,
            height: 3,
        },
    );
    client.accept(
        55,
        XWireRequest::CreateGraphicsContext {
            gc: SOURCE_GC,
            drawable: SOURCE,
            values: XGraphicsContextValues {
                foreground: COLOR,
                ..Default::default()
            },
        },
    );
    client.accept(
        70,
        XWireRequest::PolyFillRectangle {
            drawable: SOURCE,
            gc: SOURCE_GC,
            rectangles: vec![Rect {
                x: 0,
                y: 0,
                width: 3,
                height: 3,
            }],
        },
    );
    let result = client.send(
        62,
        XWireRequest::CopyArea {
            source: SOURCE,
            destination: PIXMAP,
            gc: GC,
            src_x: 0,
            src_y: 0,
            dst_x: 0,
            dst_y: 0,
            width: 3,
            height: 3,
        },
    );
    assert!(result.response.is_some(), "{result:?}");
    assert_eq!(client.pixels(), only_admitted(COLOR));
}

#[test]
fn a_clip_mask_confines_an_image() {
    let mut client = clipped_client();
    let data: Vec<u8> = std::iter::repeat_n(0x00abcdef_u32.to_le_bytes(), 9)
        .flatten()
        .collect();
    client.accept(
        72,
        XWireRequest::PutImage {
            format: 2,
            drawable: PIXMAP,
            gc: GC,
            width: 3,
            height: 3,
            dst_x: 0,
            dst_y: 0,
            left_pad: 0,
            depth: 24,
            data,
        },
    );
    assert_eq!(client.pixels(), only_admitted(0x00abcdef));
}

fn whole() -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: 3,
        height: 3,
    }
}

#[test]
fn a_clip_mask_outlives_its_pixmap_and_a_reused_xid() {
    // Setting a mask and freeing the pixmap at once is legal, and XTS's
    // clip-origin purposes do exactly that: the GC keeps the mask.
    let mut client = clipped_client();
    client.accept(54, XWireRequest::FreePixmap { pixmap: MASK });
    client.accept(70, fill(GC, whole()));
    assert_eq!(client.pixels(), only_admitted(COLOR));

    // The XID may now name a new pixmap; the GC still holds the old mask,
    // not whatever the client puts there next.
    client.accept(
        53,
        XWireRequest::CreatePixmap {
            depth: 1,
            pixmap: MASK,
            drawable: PIXMAP,
            width: 3,
            height: 3,
        },
    );
    client.accept(
        70,
        XWireRequest::PolyFillRectangle {
            drawable: MASK,
            gc: MASK_GC,
            rectangles: vec![whole()],
        },
    );
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
    client.accept(70, fill(GC, whole()));
    assert_eq!(client.pixels(), only_admitted(0x00abcdef));
}

#[test]
fn a_tile_outlives_its_pixmap() {
    const TILE: XResourceId = XResourceId::new(0x100016, 1);
    const TILE_GC: XResourceId = XResourceId::new(0x100017, 1);
    let mut client = Client::new();
    client.accept(
        53,
        XWireRequest::CreatePixmap {
            depth: 24,
            pixmap: TILE,
            drawable: PIXMAP,
            width: 2,
            height: 1,
        },
    );
    client.accept(
        55,
        XWireRequest::CreateGraphicsContext {
            gc: TILE_GC,
            drawable: TILE,
            values: XGraphicsContextValues {
                foreground: 0x00abcdef,
                ..Default::default()
            },
        },
    );
    client.accept(
        70,
        XWireRequest::PolyFillRectangle {
            drawable: TILE,
            gc: TILE_GC,
            rectangles: vec![Rect {
                x: 1,
                y: 0,
                width: 1,
                height: 1,
            }],
        },
    );
    // A tile of two columns: 0 then 0xabcdef.
    client.accept(
        56,
        XWireRequest::ChangeGraphicsContext {
            gc: GC,
            value_mask: (1 << 8) | (1 << 10),
            values: XGraphicsContextValues {
                fill_style: 1,
                tile: Some(TILE),
                ..Default::default()
            },
        },
    );
    client.accept(54, XWireRequest::FreePixmap { pixmap: TILE });
    client.accept(70, fill(GC, whole()));
    let row = [0, 0x00abcdef, 0];
    assert_eq!(client.pixels(), [row, row, row].concat());
}

#[test]
fn a_freed_pixmap_goes_when_the_last_context_lets_go() {
    const OTHER_GC: XResourceId = XResourceId::new(0x100018, 1);
    let mut client = clipped_client();
    // A second context holding the same mask.
    client.accept(
        55,
        XWireRequest::CreateGraphicsContext {
            gc: OTHER_GC,
            drawable: PIXMAP,
            values: XGraphicsContextValues::default(),
        },
    );
    client.accept(
        57,
        XWireRequest::CopyGraphicsContext {
            source: GC,
            destination: OTHER_GC,
            value_mask: 1 << 19,
        },
    );
    client.accept(54, XWireRequest::FreePixmap { pixmap: MASK });
    assert_eq!(client.runtime.retained_pixmap_count(), 1);
    // One lets go by taking no mask; the other still holds it.
    client.accept(
        56,
        XWireRequest::ChangeGraphicsContext {
            gc: GC,
            value_mask: 1 << 19,
            values: XGraphicsContextValues::default(),
        },
    );
    assert_eq!(client.runtime.retained_pixmap_count(), 1);
    client.accept(60, XWireRequest::FreeGraphicsContext { gc: OTHER_GC });
    assert_eq!(client.runtime.retained_pixmap_count(), 0);
}
