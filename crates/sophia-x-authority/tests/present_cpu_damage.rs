use sophia_protocol::{
    NamespaceId, Rect, Region, Size, SurfaceConstraints, SurfaceId, TransactionId,
};
use sophia_x_authority::*;

const NS: NamespaceId = NamespaceId::from_raw(63);
const WINDOW: XResourceId = XResourceId::new(0x500003, 1);
const PIXMAP: XResourceId = XResourceId::new(0x500010, 1);
const FULL: Rect = Rect {
    x: 0,
    y: 0,
    width: 64,
    height: 64,
};
const PATCH: Rect = Rect {
    x: 4,
    y: 7,
    width: 8,
    height: 9,
};

fn fixture() -> XAuthorityRuntime {
    let mut runtime = XAuthorityRuntime::new();
    let response = runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(1),
        namespace: NS,
        kind: XAuthorityRequestKind::CreateWindow {
            window: WINDOW,
            surface: SurfaceId::new(63, 1),
            geometry: FULL,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    });
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    runtime
        .create_pixmap(
            NS,
            PIXMAP,
            Size {
                width: 64,
                height: 64,
            },
            24,
            1,
        )
        .unwrap();
    assert_eq!(
        runtime
            .apply_put_image(
                TransactionId::from_raw(2),
                NS,
                PIXMAP,
                Region::single(FULL),
                Some(&[0x55; 64 * 64 * 4]),
                None
            )
            .outcome,
        XAuthorityResponseOutcome::Accepted
    );
    runtime
}

fn present(
    runtime: &mut XAuthorityRuntime,
    ticket: u64,
    pixmap: XResourceId,
    offset: i16,
    valid: Option<Region>,
    damage: Option<Region>,
) -> Region {
    runtime.begin_dispatch();
    let response = runtime.present_standard_pixmap(
        TransactionId::from_raw(ticket),
        NS,
        WINDOW,
        pixmap,
        offset,
        0,
        valid,
        damage,
    );
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    response.transactions[0]
        .content
        .canonical_variant()
        .damage
        .clone()
}

#[test]
fn consecutive_cpu_presents_keep_patch_damage_in_the_published_raster() {
    let mut runtime = fixture();
    assert_eq!(
        present(&mut runtime, 10, PIXMAP, 0, None, None),
        Region::single(FULL)
    );
    assert_eq!(
        present(
            &mut runtime,
            11,
            PIXMAP,
            0,
            None,
            Some(Region::single(PATCH))
        ),
        Region::single(PATCH),
        "a warm CPU Present must retain canonical patch damage"
    );
    assert_eq!(
        present(&mut runtime, 12, PIXMAP, 0, None, Some(Region::empty())),
        Region::empty()
    );
    assert_eq!(
        present(&mut runtime, 13, PIXMAP, 0, None, None),
        Region::single(FULL)
    );
}

#[test]
fn a_first_cpu_patch_and_a_patch_after_dma_buf_switch_damage_the_whole_raster() {
    let mut runtime = fixture();
    assert_eq!(
        present(
            &mut runtime,
            10,
            PIXMAP,
            0,
            None,
            Some(Region::single(PATCH))
        ),
        Region::single(FULL)
    );
    let dma = XResourceId::new(0x500011, 1);
    runtime
        .create_dri3_pixmap(NS, dma, 91, 64 * 64 * 4, 64, 64, 256, 24, 32)
        .unwrap();
    present(&mut runtime, 11, dma, 0, None, None);
    assert_eq!(
        present(
            &mut runtime,
            12,
            PIXMAP,
            0,
            None,
            Some(Region::single(PATCH))
        ),
        Region::single(FULL),
        "a retained CPU buffer is not the preceding DMA-BUF frame"
    );
    assert_eq!(
        present(
            &mut runtime,
            13,
            PIXMAP,
            0,
            None,
            Some(Region::single(PATCH))
        ),
        Region::single(PATCH)
    );
}

#[test]
fn offset_and_valid_region_presents_keep_conservative_raster_damage() {
    let mut runtime = fixture();
    present(&mut runtime, 10, PIXMAP, 0, None, None);
    assert_eq!(
        present(
            &mut runtime,
            11,
            PIXMAP,
            1,
            None,
            Some(Region::single(PATCH))
        ),
        Region::single(FULL)
    );
    assert_eq!(
        present(
            &mut runtime,
            12,
            PIXMAP,
            0,
            Some(Region::single(FULL)),
            Some(Region::single(PATCH))
        ),
        Region::single(FULL)
    );
}

#[test]
fn a_core_draw_breaks_the_cpu_present_predecessor() {
    let mut runtime = fixture();
    present(&mut runtime, 10, PIXMAP, 0, None, None);
    runtime.begin_dispatch();
    let response = runtime.apply_put_image(
        TransactionId::from_raw(11),
        NS,
        WINDOW,
        Region::single(FULL),
        Some(&[0x33; 64 * 64 * 4]),
        None,
    );
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    assert_eq!(
        present(
            &mut runtime,
            12,
            PIXMAP,
            0,
            None,
            Some(Region::single(PATCH))
        ),
        Region::single(FULL),
        "a core draw invalidates the CPU Present predecessor"
    );
    assert_eq!(
        present(
            &mut runtime,
            13,
            PIXMAP,
            0,
            None,
            Some(Region::single(PATCH))
        ),
        Region::single(PATCH)
    );
}

#[test]
fn resize_requires_a_new_full_raster_before_patches_resume() {
    let mut runtime = fixture();
    present(&mut runtime, 10, PIXMAP, 0, None, None);
    runtime
        .configure_window_geometry(
            NS,
            WINDOW,
            XWindowGeometryUpdate {
                width: Some(32),
                height: Some(32),
                generation: 20,
                ..XWindowGeometryUpdate::default()
            },
        )
        .unwrap();
    let resized = Rect {
        width: 32,
        height: 32,
        ..FULL
    };
    let small = XResourceId::new(0x500012, 1);
    runtime
        .create_pixmap(
            NS,
            small,
            Size {
                width: 32,
                height: 32,
            },
            24,
            1,
        )
        .unwrap();
    assert_eq!(
        runtime
            .apply_put_image(
                TransactionId::from_raw(11),
                NS,
                small,
                Region::single(resized),
                Some(&[0x66; 32 * 32 * 4]),
                None
            )
            .outcome,
        XAuthorityResponseOutcome::Accepted
    );
    assert_eq!(
        present(
            &mut runtime,
            12,
            small,
            0,
            None,
            Some(Region::single(PATCH))
        ),
        Region::single(resized),
        "resize replaces the raster"
    );
    assert_eq!(
        present(
            &mut runtime,
            13,
            small,
            0,
            None,
            Some(Region::single(PATCH))
        ),
        Region::single(PATCH)
    );
}

#[test]
fn child_presents_keep_full_parent_raster_damage() {
    let mut runtime = fixture();
    let child = XResourceId::new(0x500020, 1);
    let response = runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(3),
        namespace: NS,
        kind: XAuthorityRequestKind::CreateWindow {
            window: child,
            surface: SurfaceId::new(64, 1),
            geometry: FULL,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    });
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    runtime.set_window_parent(NS, child, WINDOW).unwrap();
    present(&mut runtime, 10, PIXMAP, 0, None, None);
    for ticket in [11, 12] {
        runtime.begin_dispatch();
        let response = runtime.present_standard_pixmap(
            TransactionId::from_raw(ticket),
            NS,
            child,
            PIXMAP,
            0,
            0,
            None,
            Some(Region::single(PATCH)),
        );
        assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
        assert_eq!(
            response.transactions[0].content.canonical_variant().damage,
            Region::single(FULL)
        );
    }
}

#[test]
fn canonical_damage_uses_the_clipped_destination_rectangles() {
    let mut runtime = fixture();
    present(&mut runtime, 10, PIXMAP, 0, None, None);
    let beyond = Rect {
        x: 60,
        y: 62,
        width: 10,
        height: 10,
    };
    assert_eq!(
        present(
            &mut runtime,
            11,
            PIXMAP,
            0,
            None,
            Some(Region::single(beyond))
        ),
        Region::single(Rect {
            width: 4,
            height: 2,
            ..beyond
        })
    );
}
