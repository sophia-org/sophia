use sophia_protocol::{NamespaceId, Rect, Region, SurfaceConstraints, SurfaceId, TransactionId};
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

#[test]
fn executed_present_regions_are_partitioned_without_changing_source_damage() {
    let mut runtime = fixture(8, 8, 0);
    runtime
        .create_dri3_pixmap(NS, PIXMAP, 1, 8 * 8 * 4, 8, 8, 32, 24, 32)
        .unwrap();
    let part = Rect {
        x: 1,
        y: 1,
        width: 3,
        height: 4,
    };
    let cases = [
        None,
        Some(Region::single(Rect {
            x: -1,
            y: -1,
            width: 10,
            height: 10,
        })),
        Some(Region::single(part)),
        Some(Region {
            rects: vec![part, part],
        }),
        Some(Region::empty()),
        Some(Region::single(Rect { x: 9, ..part })),
    ];
    for (index, update) in cases.into_iter().enumerate() {
        runtime.begin_dispatch();
        let response = runtime.present_standard_pixmap(
            TransactionId::from_raw(500 + index as u64),
            NS,
            PARENT,
            PIXMAP,
            0,
            0,
            None,
            update,
        );
        assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    }
    let stats = runtime.present_timing_statistics().damage;
    assert_eq!(
        (
            stats.absent,
            stats.explicit_full_rect,
            stats.explicit_regions,
            stats.effective_empty
        ),
        (1, 1, 2, 2)
    );
    assert_eq!(stats.source_pixels, 6 * 64);
    assert_eq!(stats.rect_pixels, 164); // Clipped sums deliberately count overlap.
    assert_eq!(stats.rects, 5);
    let rejected = runtime.present_standard_pixmap(
        TransactionId::from_raw(600),
        NS,
        PARENT,
        XResourceId::new(0x400999, 1),
        0,
        0,
        None,
        None,
    );
    assert!(matches!(
        rejected.outcome,
        XAuthorityResponseOutcome::Rejected(_)
    ));
    assert_eq!(runtime.present_timing_statistics().damage, stats);
}

#[test]
fn prepared_or_cancelled_damage_is_not_counted_as_executed() {
    let mut runtime = fixture(8, 8, 0);
    runtime
        .create_dri3_pixmap(NS, PIXMAP, 1, 8 * 8 * 4, 8, 8, 32, 24, 32)
        .unwrap();
    for ticket in [700, 701] {
        runtime
            .prepare_standard_pixmap(
                1,
                TransactionId::from_raw(ticket),
                NS,
                PARENT,
                PIXMAP,
                (0, 0),
                None,
                None,
                XPresentFenceResources::default(),
            )
            .unwrap();
    }
    assert_eq!(
        runtime.present_timing_statistics().damage,
        Default::default()
    );
    assert!(runtime.cancel_prepared_standard_pixmap(TransactionId::from_raw(700)));
    let response = runtime
        .execute_prepared_standard_pixmap(
            TransactionId::from_raw(701),
            TransactionId::from_raw(702),
        )
        .unwrap()
        .unwrap()
        .response;
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    assert_eq!(runtime.present_timing_statistics().damage.absent, 1);
    assert_eq!(runtime.present_timing_statistics().damage.source_pixels, 64);
}
