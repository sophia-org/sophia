//! G3-5a (source review R1): a Session-side consumer, built as a dependency
//! with test-support and the combined feature, reaches the combined native
//! fixture and the bridge only through public paths, and reads an accept
//! refusal by its public names. No device and no renderer service run.
#![cfg(all(feature = "test-support", feature = "experimental-combined-frame-job"))]

use sophia_backend_live::{
    CombinedAccept, CombinedAcceptError, CombinedAcceptFailed, CombinedAcceptRefusal,
    CombinedNativeFixture, LiveProductionVisualRuntime, LiveRendererFrameCorrelation,
    LiveShellContentFrame, LiveShellContentLayer, ShellContentNative,
};
use sophia_engine::{
    CompositorContentImage, CompositorDamageList, CompositorNodeId, ContentTargetContinuity,
    HeadlessOutput, OutputFrameDamageSnapshot, PresentedContentTarget,
};
use sophia_protocol::{
    ContentAllocationId, ContentGrant, ContentLimits, ContentLogicalRect, ContentOutputId,
    ContentPixelRect, ContentResourceBegin, ContentResourceId, OutputId, Rect, Size, TransactionId,
};
use sophia_runtime::{ContentResourceStore, CpuPixelValidation};

const GRANT: ContentGrant = ContentGrant {
    connection_epoch: 200,
    content_grant_epoch: 200,
};

/// One resident 4x2 resource, uploaded through the store's public ISB path.
fn lease(store: &mut ContentResourceStore) -> sophia_runtime::ContentResourceLease {
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    store
        .begin_isb(
            TransactionId::from_raw(1),
            ContentResourceBegin {
                grant: GRANT,
                resource,
                width_px: 4,
                height_px: 2,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: 1,
                total_bytes: 32,
            },
            0,
        )
        .unwrap();
    let mut scan = CpuPixelValidation::from_owned(vec![0xff; 32].into()).unwrap();
    while !scan.step().unwrap() {}
    store
        .complete_isb(GRANT, resource, &mut Some(Ok(scan.finish().unwrap())), 0)
        .unwrap();
    while store.take_event().is_some() {}
    store.lease(GRANT, resource).unwrap()
}

/// A Shell candidate for `output` with one real lease.
fn candidate(output: HeadlessOutput, store: &mut ContentResourceStore) -> LiveShellContentFrame {
    let lease = lease(store);
    let content_output = ContentOutputId {
        id: output.id.raw(),
        generation: 1,
    };
    let rect = ContentPixelRect {
        x: 0,
        y: 0,
        width: 4,
        height: 2,
    };
    LiveShellContentFrame {
        output: output.id,
        content_output,
        grant: GRANT,
        candidate_generation: 1,
        interaction_generation: 1,
        popouts: Vec::new(),
        images: vec![CompositorContentImage {
            node: CompositorNodeId::ShellContent {
                grant: GRANT,
                output: output.id,
                candidate: 1,
                surface: 0,
                placement: 0,
            },
            generation: lease.description().resource.generation,
            output_size_px: output.size,
            geometry_px: Rect {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
            },
            size_px: Size {
                width: 4,
                height: 2,
            },
            stride: 16,
            format: u32::from_le_bytes(*b"AR24"),
            resource: lease.into(),
        }],
        targets: vec![PresentedContentTarget {
            continuity: ContentTargetContinuity::mint(),
            scale_generation: 1,
            grant: GRANT,
            output: content_output,
            candidate_generation: 1,
            presentation_epoch: 0,
            interaction_generation: 1,
            allocation: ContentAllocationId {
                id: output.id.raw(),
                generation: 1,
            },
            allocation_logical: ContentLogicalRect {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
            },
            allocation_pixel: rect,
            target_id: 1,
            target_generation: 1,
            action_id: 1,
            bounds_px: rect,
        }],
        allocations: vec![],
    }
}

#[test]
fn a_dependency_reads_a_bridge_accept_refusal_by_its_public_names() {
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        ..HeadlessOutput::deterministic()
    };
    let mut fixture = CombinedNativeFixture::new(&[output]);
    let frame = ShellContentNative::allocate_combined_frame(&mut fixture);
    let (head, _inbox) = ShellContentNative::reserve_combined_frame(&mut fixture, output.id)
        .expect("the serving facade reserves");
    let mut runtime = LiveProductionVisualRuntime::new(&[output], None).unwrap();
    let mut store = ContentResourceStore::new(ContentLimits::prototype(GRANT)).unwrap();
    let shell = candidate(output, &mut store);
    // The facade issued this admission but holds no result for it.
    let accept = CombinedAccept {
        job: 1,
        frame,
        checksum: frame.raw(),
        snapshot: OutputFrameDamageSnapshot {
            damage_history: Default::default(),
            output,
            surfaces: Vec::new(),
            compositor_display_list: CompositorDamageList::empty(output.id),
            software_cursor: None,
        },
        correlation: LiveRendererFrameCorrelation {
            native: None,
            request: None,
            trace: None,
            direct_scanout: None,
        },
        output_format: None,
    };
    let result: Result<(), Box<CombinedAcceptFailed>> =
        ShellContentNative::accept_combined_shell_content(
            &mut fixture,
            &mut runtime,
            shell.clone(),
            LiveShellContentLayer::Shell,
            &head,
            accept,
        );
    let Err(failed) = result else {
        panic!("an admission with no held result was accepted");
    };
    let CombinedAcceptFailed { error, frame } = *failed;
    assert!(
        matches!(
            error,
            CombinedAcceptError::Native(CombinedAcceptRefusal::NotHeld)
        ),
        "the native half's refusal is readable by name"
    );
    assert_eq!(frame, shell, "the refused frame comes back");
}
