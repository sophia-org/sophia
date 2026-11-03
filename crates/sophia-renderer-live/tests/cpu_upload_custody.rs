#![cfg(feature = "gbm-probe")]
use sophia_protocol::{
    ContentGrant, ContentLimits, ContentResourceBegin, ContentResourceId, TransactionId,
};
use sophia_runtime::{ContentResourceStore, IsbPixelValidation, ValidatedIsbPixels};
#[path = "../../sophia-runtime/tests/support/content_isb.rs"]
mod isb;
fn content_resource() -> (
    sophia_runtime::ContentResourceStore,
    sophia_runtime::ContentResourceLease,
) {
    let grant = ContentGrant {
        connection_epoch: 7,
        content_grant_epoch: 9,
    };
    let resource = ContentResourceId {
        id: 11,
        generation: 1,
    };
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant)).unwrap();
    isb::accept(
        &mut store,
        TransactionId::from_raw(1),
        ContentResourceBegin {
            grant,
            resource,
            width_px: 4,
            height_px: 2,
            rendered_scale_numerator: 1,
            rendered_scale_denominator: 1,
            pixel_format: 1,
            chunk_count: 1,
            total_bytes: 32,
        },
        &[0x7f; 32],
        0,
    );
    let lease = store.lease(grant, resource).unwrap();
    (store, lease)
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; render node only"]
fn cached_gl_pixels_do_not_hold_resource_custody_or_alias_a_reopened_store() {
    use sophia_protocol::{ContentResourceRetire, ShellContentRecord};
    use sophia_renderer_native_egl::*;
    let path = std::env::var_os("SOPHIA_TEST_RENDER_NODE").expect("explicit render-node grant");
    let mut renderer = NativeGbmRenderedScanoutContext::from_backend_device_result(
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path),
    )
    .context
    .unwrap();
    let mut previous = None;
    for expected_uploads in 1..=2 {
        // Same protocol identity after a connection/store replacement, fresh accepted bytes.
        let (mut store, lease) = content_resource();
        let description = lease.description().clone();
        let cpu = lease.into_cpu().ok().unwrap();
        let version = cpu.pixel_version();
        assert!(version.is_some());
        assert_ne!(version, previous);
        previous = version;
        let layers = [NativeCompositionLayer::Cpu(NativeCpuCompositionLayer {
            pixel_version: version,
            width: 4,
            height: 2,
            stride: 16,
            format: u32::from_le_bytes(*b"XR24"),
            pixels: cpu.bytes(),
            target: NativeCompositionRect {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
            },
            clip: None,
            alpha: 1.0,
            sampling: NativeCompositionSampling::ExactNearest,
        })];
        let (report, snapshot) = renderer
            .export_composed_with_snapshot(
                NativeCompositionFrame {
                    width: 4,
                    height: 2,
                    layers: &layers,
                    trace: None,
                    repaint: None,
                },
                NativeCompositionOutputRequest::default(),
            )
            .unwrap();
        assert_eq!(snapshot.rgba(), [127, 127, 127, 255].repeat(8));
        drop(report);
        drop(cpu);
        while store.take_event().is_some() {}
        store
            .retire(
                TransactionId::from_raw(100),
                &ContentResourceRetire {
                    grant: description.grant,
                    resource: description.resource,
                },
            )
            .unwrap();
        store.collect();
        assert!(matches!(store.take_event().map(|event| event.record),
            Some(ShellContentRecord::ResourceReleased(record)) if record.resource == description.resource));
        let stats = renderer.persistent_render_stats().cpu_upload;
        assert_eq!(stats.uploads, expected_uploads);
        assert_eq!(
            stats.retained_bytes, 32,
            "GPU pixels remain cached after ResourceReleased"
        );
    }
}
