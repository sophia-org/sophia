#![cfg(test)]

use super::*;
use std::sync::Arc;

#[test]
fn topology_replacement_cannot_orphan_a_shell_retirement_claim() {
    let first = HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 640,
            height: 480,
        },
        scale: 1,
    };
    let mut second = first;
    second.id = OutputId::from_raw(2);
    let mut runtime = LiveProductionVisualRuntime::new(&[first, second], None).expect("runtime");
    let old_grant = sophia_protocol::ContentGrant {
        connection_epoch: 7,
        content_grant_epoch: 9,
    };
    let replacement_grant = sophia_protocol::ContentGrant {
        connection_epoch: 8,
        content_grant_epoch: 10,
    };
    runtime
        .retained_projection_retirements
        .insert((second.id, LiveShellContentLayer::Shell), old_grant);
    let retained = BTreeSet::from([first.id]);

    assert_eq!(
        runtime
            .retain_shell_content_outputs(&retained)
            .expect_err("a removed output must not silently lose its accepted obligation")
            .to_string(),
        "native topology replacement would orphan a shell content retirement claim"
    );
    assert_eq!(
        runtime.revoke_shell_content_retirement_claims(replacement_grant),
        0,
        "a fresh connection cannot revoke an older connection's exact debt"
    );
    assert_eq!(runtime.revoke_shell_content_retirement_claims(old_grant), 1);
    assert_eq!(
        runtime
            .retain_shell_content_outputs(&retained)
            .expect("revoked connection has no remaining retirement obligation"),
        0
    );
}

#[test]
fn in_flight_renderer_source_keeps_cpu_content_variants() {
    let surface = SurfaceId::new(7, 1);
    let cpu_handle = 4;
    let cpu_layers = [LiveCpuPresentationLayer {
        surface,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 64,
            height: 48,
        },
        buffer: sophia_renderer_live::LiveCpuBufferSource {
            handle: cpu_handle,
            size: Size {
                width: 64,
                height: 48,
            },
            stride: 64 * 4,
            format: sophia_renderer_live::LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            generation: 1,
            bytes: Arc::new(vec![0x7f; 64 * 48 * 4]),
        },
    }];
    let dma_source = BufferSource::DmaBuf { handle: 3 };
    let displayed = crate::LiveRetainedRendererImageLayer {
        image_id: sophia_renderer_live::LiveRendererImageId::from_raw(9),
        size: Size {
            width: 64,
            height: 48,
        },
        format: sophia_renderer_live::LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        placement: sophia_renderer_live::LiveCompositionPlacement {
            target: cpu_layers[0].geometry,
            clip: None,
            transform: Transform::IDENTITY,
            alpha: 1.0,
            sampling: sophia_engine::HeadSamplingClass::Exact,
        },
    };

    let sources = retained_surface_sources(
        surface,
        dma_source,
        &cpu_layers,
        Some(&displayed),
        None,
        None,
        None,
    )
    .expect("mixed retained source set");

    assert_eq!(sources.len(), 2);
    assert!(sources.iter().any(|source| {
        source.source == dma_source
            && matches!(
                source.kind,
                sophia_renderer_live::LiveOwnedHeadCompositionSourceKind::RendererImage { .. }
            )
    }));
    assert!(sources.iter().any(|source| {
        source.source == BufferSource::CpuBuffer { handle: cpu_handle }
            && matches!(
                source.kind,
                sophia_renderer_live::LiveOwnedHeadCompositionSourceKind::Cpu(_)
            )
    }));
}

fn direct_test_layer(image_raw: u64) -> crate::LiveRetainedRendererImageLayer {
    crate::LiveRetainedRendererImageLayer {
        image_id: sophia_renderer_live::LiveRendererImageId::from_raw(image_raw),
        size: Size {
            width: 2560,
            height: 1440,
        },
        format: sophia_renderer_live::LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888,
        placement: sophia_renderer_live::LiveCompositionPlacement {
            target: Rect {
                x: 0,
                y: 0,
                width: 2560,
                height: 1440,
            },
            clip: None,
            transform: Transform::IDENTITY,
            alpha: 1.0,
            sampling: sophia_engine::HeadSamplingClass::Exact,
        },
    }
}

fn direct_test_frame() -> sophia_renderer_live::LiveOwnedMultiPlaneDmaBufFrame {
    let fd: std::os::fd::OwnedFd = std::fs::File::open("/dev/null").unwrap().into();
    sophia_renderer_live::LiveOwnedMultiPlaneDmaBufFrame {
        width: 2560,
        height: 1440,
        format: sophia_renderer_live::LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888,
        modifier: 0,
        plane_count: 1,
        planes: [
            Some(sophia_renderer_live::LiveOwnedDmaBufPlane {
                fd,
                offset: 0,
                stride: 2560 * 4,
            }),
            None,
            None,
            None,
        ],
    }
}

/// A directly displayed surface composes from the client's still-held planes,
/// never from a renderer image nobody imported.
///
/// The renderer's store holds snapshots of frames it composed; a direct frame
/// was deliberately never composed -- that copy is what direct scanout skips.
/// Emitting `RendererImage` for one asked the renderer for a picture nobody
/// took: the first overlay ever opened over a direct frame failed its compose
/// with `InvalidLayer` (reported as `InvalidTarget`) and the session died with
/// the client on glass.
#[test]
fn a_displayed_direct_frame_sources_the_clients_planes() {
    let surface = SurfaceId::new(7, 1);
    let dma_source = BufferSource::DmaBuf { handle: 3 };
    let displayed = direct_test_layer(437);

    let sources = retained_surface_sources(
        surface,
        dma_source,
        &[],
        None,
        None,
        Some(&displayed),
        Some(direct_test_frame()),
    )
    .expect("direct retained source set");

    let [source] = sources.as_slice() else {
        panic!("one displayed layer produces one source")
    };
    let sophia_renderer_live::LiveOwnedHeadCompositionSourceKind::DmaBuf { image_id, frame } =
        &source.kind
    else {
        panic!(
            "a direct frame must not claim a renderer image: {:?}",
            source.kind
        )
    };
    // The identity survives, so the compose captures the snapshot under the
    // same id the layer carries and later retained frames can use it.
    assert_eq!(image_id.raw(), 437);
    assert_eq!(frame.plane_count, 1);
}

/// The same for a direct submission still in flight: an activation can land
/// between its submit and its retirement, and the requeue that follows must
/// not name its never-imported image either.
#[test]
fn an_in_flight_direct_frame_sources_the_clients_planes() {
    let surface = SurfaceId::new(7, 1);
    let dma_source = BufferSource::DmaBuf { handle: 3 };
    let in_flight = direct_test_layer(441);

    let sources = retained_surface_sources(
        surface,
        dma_source,
        &[],
        Some(&in_flight),
        Some(direct_test_frame()),
        None,
        None,
    )
    .expect("in-flight direct source set");

    assert!(sources.iter().any(|source| matches!(
        &source.kind,
        sophia_renderer_live::LiveOwnedHeadCompositionSourceKind::DmaBuf { image_id, .. }
            if image_id.raw() == 441
    )));
}

/// A composed frame's retained image is real -- the flip that displayed it
/// promoted a snapshot -- so without a direct override the arms keep naming
/// the renderer image, and the ordinary retained path is unchanged.
#[test]
fn a_composed_frame_still_sources_its_renderer_image() {
    let surface = SurfaceId::new(7, 1);
    let dma_source = BufferSource::DmaBuf { handle: 3 };
    let displayed = direct_test_layer(299);

    let sources =
        retained_surface_sources(surface, dma_source, &[], None, None, Some(&displayed), None)
            .expect("composed retained source set");

    assert!(matches!(
        sources[0].kind,
        sophia_renderer_live::LiveOwnedHeadCompositionSourceKind::RendererImage { image_id, .. }
            if image_id.raw() == 299
    ));
}

/// The Present/renderer-image identity is one definition, not three guesses.
///
/// Finding a displayed direct frame's client planes is a reverse lookup from
/// the image id its retained layer carries. If that derivation ever drifts,
/// the lookup does not error -- it finds nothing, falls back to naming a
/// renderer image nobody imported, and the compose refuses. That is exactly
/// how the first overlay over a direct frame killed a session, so the
/// round trip is pinned here rather than left as a coincidence three call
/// sites happen to agree on.
#[test]
fn a_present_and_its_renderer_image_name_each_other() {
    use crate::presentation::{present_for_renderer_image, renderer_image_for_present};
    use sophia_protocol::TransactionId;

    for raw in [1u64, 437, 4_294_967_296] {
        let transaction = TransactionId::from_raw(raw);
        let image = renderer_image_for_present(transaction);
        assert_eq!(
            present_for_renderer_image(image),
            transaction,
            "the reverse lookup must find the Present that staged the image"
        );
    }
}

#[path = "../../../tests/support/live_presentation_regressions.rs"]
mod live_presentation_regressions;
