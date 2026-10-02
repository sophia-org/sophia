use sophia_protocol::{
    BufferSource, NamespaceId, Rect, Region, Size, SurfaceConstraints, SurfaceId, TransactionId,
};
use sophia_x_authority::*;

const NS: NamespaceId = NamespaceId::from_raw(46);
const PARENT: XResourceId = XResourceId::new(0x400003, 1);
const CHILD: XResourceId = XResourceId::new(0x40000b, 1);
const PIXMAP: XResourceId = XResourceId::new(0x400010, 1);
const SURFACE: SurfaceId = SurfaceId::new(46, 1);

fn fixture(width: i32, height: i32, offset: i32) -> XAuthorityRuntime {
    let mut runtime = XAuthorityRuntime::new();
    for (window, surface, geometry) in [
        (
            PARENT,
            SURFACE,
            Rect {
                x: 100,
                y: 200,
                width: width + offset,
                height: height + offset,
            },
        ),
        (
            CHILD,
            SurfaceId::new(47, 1),
            Rect {
                x: offset,
                y: offset,
                width,
                height,
            },
        ),
    ] {
        let response = runtime.apply(XAuthorityRequestPacket {
            transaction: TransactionId::from_raw(u64::from(surface.index())),
            namespace: NS,
            kind: XAuthorityRequestKind::CreateWindow {
                window,
                surface,
                geometry,
                constraints: SurfaceConstraints {
                    min_size: None,
                    max_size: None,
                },
                generation: 1,
            },
        });
        assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    }
    runtime.set_window_parent(NS, CHILD, PARENT).unwrap();
    runtime
}

fn upload(runtime: &mut XAuthorityRuntime, drawable: XResourceId, rect: Rect, data: &[u8]) {
    runtime.begin_dispatch();
    let response = runtime.apply_put_image(
        TransactionId::from_raw(100),
        NS,
        drawable,
        Region::single(rect),
        Some(data),
        Some(&XPutImageSemantics {
            format: 2,
            depth: 24,
            left_pad: 0,
            byte_order: XByteOrder::LittleEndian,
            gc: XGraphicsContextValues::default(),
        }),
    );
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
}

#[test]
fn dri3_present_replaces_a_previous_cpu_publication() {
    // Firefox paints its startup background through core X, then switches to
    // DRI3 on a child. Retaining that CPU background must not select it again.
    let mut runtime = fixture(40, 20, 0);
    upload(
        &mut runtime,
        PARENT,
        Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 20,
        },
        &[0; 40 * 20 * 4],
    );
    let old = runtime.take_cpu_buffer_update().unwrap().handle();
    let descriptor = runtime
        .create_dri3_pixmap(NS, PIXMAP, 1, 40 * 20 * 4, 40, 20, 160, 24, 32)
        .unwrap();
    runtime.begin_dispatch();
    let response = runtime.present_standard_pixmap(
        TransactionId::from_raw(101),
        NS,
        CHILD,
        PIXMAP,
        0,
        0,
        None,
        None,
    );
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    assert_eq!(response.transactions[0].surface, SURFACE);
    assert_eq!(
        response.transactions[0].target_buffer(),
        BufferSource::DmaBuf {
            handle: descriptor.handle.raw()
        },
        "the retained CPU background {old} must not replace the requested DRI3 source"
    );
    assert!(runtime.take_cpu_buffer_updates().is_empty());
}

#[test]
fn firefox_strip_uploads_present_on_the_parent_and_preserve_later_patches() {
    let (width, height, offset) = (1266, 1408, 3);
    let mut runtime = fixture(width, height, offset);
    runtime
        .create_pixmap(NS, PIXMAP, Size { width, height }, 24, 1)
        .unwrap();
    let mut expected = Vec::new();
    for y in 0..height {
        for x in 0..width {
            expected.extend_from_slice(&[x as u8, y as u8, 0x7f, 0]);
        }
    }
    for y in (0..height).step_by(51) {
        let rows = (height - y).min(51);
        let start = (y * width * 4) as usize;
        let end = ((y + rows) * width * 4) as usize;
        upload(
            &mut runtime,
            PIXMAP,
            Rect {
                x: 0,
                y,
                width,
                height: rows,
            },
            &expected[start..end],
        );
    }
    runtime.begin_dispatch();
    let response = runtime.present_standard_pixmap(
        TransactionId::from_raw(102),
        NS,
        CHILD,
        PIXMAP,
        0,
        0,
        None,
        None,
    );
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    assert_eq!(response.transactions[0].surface, SURFACE);
    let XAuthorityCpuBufferUpdate::Replace(snapshot) = runtime.take_cpu_buffer_update().unwrap()
    else {
        panic!("first publication must replace")
    };
    assert_eq!(snapshot.drawable, PARENT);
    assert_eq!(response.transactions[0].raster_extent(), snapshot.size);
    for y in 0..height {
        let start = ((y + offset) * snapshot.size.width * 4 + offset * 4) as usize;
        assert_eq!(
            &snapshot.bytes[start..start + width as usize * 4],
            &expected[(y * width * 4) as usize..((y + 1) * width * 4) as usize]
        );
    }
    let rect = Rect {
        x: 12,
        y: 52,
        width: 1,
        height: 1,
    };
    upload(&mut runtime, PIXMAP, rect, &[0x44, 0x55, 0x66, 0]);
    runtime.begin_dispatch();
    let response = runtime.present_standard_pixmap(
        TransactionId::from_raw(103),
        NS,
        CHILD,
        PIXMAP,
        0,
        0,
        None,
        Some(Region::single(rect)),
    );
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    assert_eq!(response.transactions[0].surface, SURFACE);
    let XAuthorityCpuBufferUpdate::PatchBatch(batch) = runtime.take_cpu_buffer_update().unwrap()
    else {
        panic!("later publication must patch")
    };
    assert_eq!(batch.handle, snapshot.handle);
    assert_eq!(
        response.transactions[0].damage,
        Region::single(Rect {
            x: rect.x + offset,
            y: rect.y + offset,
            ..rect
        })
    );
}

#[test]
fn only_untransformed_top_level_present_supplies_source_pixel_damage() {
    let small = Rect {
        x: 2,
        y: 3,
        width: 4,
        height: 5,
    };
    let full = Region::single(Rect {
        x: 0,
        y: 0,
        width: 40,
        height: 20,
    });
    // Coordinate ambiguity must not turn into undersized output repaint.
    for (label, window, child_offset, x_offset, valid, damage, precise) in [
        ("top-level", PARENT, 0, 0, None, Region::single(small), true),
        ("child", CHILD, 0, 0, None, Region::single(small), false),
        ("offset", PARENT, 0, 1, None, Region::single(small), false),
        (
            "size mismatch",
            PARENT,
            1,
            0,
            None,
            Region::single(small),
            false,
        ),
        (
            "valid region",
            PARENT,
            0,
            0,
            Some(Region::single(small)),
            Region::single(small),
            false,
        ),
        (
            "complex",
            PARENT,
            0,
            0,
            None,
            Region {
                rects: vec![small; 33],
            },
            false,
        ),
    ] {
        let mut runtime = fixture(40, 20, child_offset);
        runtime
            .create_dri3_pixmap(NS, PIXMAP, 1, 40 * 20 * 4, 40, 20, 160, 24, 32)
            .unwrap();
        runtime.begin_dispatch();
        let response = runtime.present_standard_pixmap(
            TransactionId::from_raw(201),
            NS,
            window,
            PIXMAP,
            x_offset,
            0,
            valid,
            Some(damage.clone()),
        );
        assert_eq!(
            response.outcome,
            XAuthorityResponseOutcome::Accepted,
            "{label}"
        );
        assert_eq!(
            response.transactions[0].content.canonical_variant().damage,
            if precise { damage } else { full.clone() },
            "{label}"
        );
    }
}
