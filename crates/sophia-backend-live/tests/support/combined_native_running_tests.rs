//! G3-5d d-1: the running fixture's renderer reads the validated pixels the
//! worker bound into every occurrence, in final layer order, and the pixels
//! come home whole once it has dropped them. A real owned-ring job with its
//! admitted description goes through the validator, the worker's route,
//! slot, bind and the fixture's seam; nothing here is injected.

use super::*;
use sophia_protocol::{
    ContentGrant, ContentResourceBegin, ContentResourceId, DRM_FORMAT_ARGB8888, Rect, Size,
};
use sophia_renderer_live::{LiveCompositionPlacement, LiveOwnedMixedCompositionFrame};
use sophia_runtime::{
    ContentEpochRegistry, IsbValidationWorker, OwnedRing, OwnedRingGeometry, OwnedSlotIdentity,
};
use std::os::unix::fs::FileExt;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(10);
const GRANT: ContentGrant = ContentGrant {
    connection_epoch: 1,
    content_grant_epoch: 1,
};
/// Eight valid premultiplied pixels (no colour above its alpha), as the G3-2
/// bind controls use.
const BYTES: [u8; 32] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32,
];

fn placement(x: i32) -> LiveCompositionPlacement {
    LiveCompositionPlacement {
        target: Rect {
            x,
            y: 0,
            width: 2,
            height: 4,
        },
        clip: None,
        transform: sophia_protocol::Transform::IDENTITY,
        alpha: 1.0,
        sampling: sophia_engine::HeadSamplingClass::Exact,
    }
}

/// A placeholder for the admitted 2x4 ARGB resource 77, at a final index.
fn occurrence(index: usize, x: i32) -> crate::CombinedPlaceholder {
    crate::CombinedPlaceholder {
        index,
        node: sophia_engine::CompositorNodeId::ShellContent {
            grant: GRANT,
            output: OutputId::from_raw(1),
            candidate: 1,
            surface: 0,
            placement: u16::try_from(index).unwrap(),
        },
        placement: placement(x),
        size: Size {
            width: 2,
            height: 4,
        },
        stride: 8,
        format: DRM_FORMAT_ARGB8888,
        handle: 77,
        generation: 9,
    }
}

fn description() -> ContentResourceBegin {
    ContentResourceBegin {
        grant: GRANT,
        resource: ContentResourceId {
            id: 77,
            generation: 9,
        },
        width_px: 2,
        height_px: 4,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: 1,
        total_bytes: 32,
    }
}

/// A retained Cpu layer the owner lowered from its own resident image.
fn retained(handle: u64) -> LiveOwnedMixedCompositionLayer {
    LiveOwnedMixedCompositionLayer::Cpu {
        buffer: LiveSharedCpuBufferSource {
            pixel_version: std::num::NonZeroU64::new(handle),
            handle,
            size: Size {
                width: 1,
                height: 1,
            },
            stride: 4,
            format: DRM_FORMAT_ARGB8888,
            generation: 1,
            bytes: LiveCpuPixelStorage::Shared(Arc::new(vec![0, 0, 0, 255])),
        },
        placement: placement(100),
    }
}

#[test]
fn a_running_render_reads_each_bound_occurrence_in_order_and_returns_the_pixels() {
    let output = sophia_engine::HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 64,
            height: 32,
        },
        scale: 1,
    };
    let mut fixture = CombinedNativeFixture::running(&[output]);
    let (head, inbox) = fixture
        .reserve_combined_frame(output.id)
        .expect("the serving facade reserves");

    // The peer's frame in a real owned ring, read back through its offer.
    let mut registry = ContentEpochRegistry::new(64 << 20).unwrap();
    registry
        .admit(sophia_protocol::ContentLimits::prototype(GRANT))
        .unwrap();
    let geometry = OwnedRingGeometry::new(2, 32).unwrap();
    let source = registry.reserve_owned_ring_source(GRANT, geometry).unwrap();
    let file = std::fs::File::from(
        rustix::fs::memfd_create("combined-running", rustix::fs::MemfdFlags::CLOEXEC).unwrap(),
    );
    file.set_len(geometry.source_bytes() as u64).unwrap();
    let mut ring = OwnedRing::admit(file.try_clone().unwrap(), source).unwrap();
    let identity = OwnedSlotIdentity {
        offer: 1,
        slot: 0,
        generation: 2,
    };
    file.write_all_at(&BYTES, 4096).unwrap();
    file.write_all_at(&identity.generation.to_le_bytes(), 0)
        .unwrap();
    let read = ring.offer(identity, 32).unwrap();

    // Final layers: 77 bound, 100 retained, 77 bound.
    let mut validator =
        IsbValidationWorker::<CombinedRenderPlan>::for_combined_plans(1, 64).unwrap();
    let reservation = validator
        .reserve_combined(32, Instant::now() + WAIT)
        .unwrap();
    let plan = CombinedRenderPlan::new(
        Instant::now() + WAIT,
        crate::api::LiveGbmEglFrameTargetRecord::new(output.size),
        crate::PendingRenderedFrame::Mixed(
            LiveOwnedMixedCompositionFrame {
                layers: vec![retained(100)],
                ..Default::default()
            },
            None,
        ),
        Vec::new(),
        None,
        Box::new([occurrence(0, 0), occurrence(2, 4)]),
    );
    let _owner_plan = validator
        .submit_ring_combined(&reservation, read, description(), plan, inbox)
        .ok()
        .expect("ring submission");
    let end = Instant::now() + WAIT;
    let result = loop {
        if let Some(result) = fixture
            .take_combined_frame_result(&head)
            .expect("the issuer answers")
        {
            break result;
        }
        assert!(
            Instant::now() < end,
            "the running worker's frame did not come home"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
    let pixels = fixture
        .settle_combined_frame(&head, &mut validator, reservation, result)
        .ok()
        .expect("the frame's own reservation settles");
    assert_eq!(
        pixels.bytes(),
        BYTES.as_slice(),
        "the validated pixels came home whole"
    );
    assert_eq!(validator.pending(), 0, "settlement cleared the credit");
    let seen = |index| CombinedSeen {
        index,
        handle: 77,
        checksum: checksum(&BYTES),
        strong: 3,
    };
    assert_eq!(
        fixture.seen(),
        [seen(0), seen(2)],
        "one read per bound occurrence, in final layer order, while the bound frame held two clones"
    );
    let slots = fixture
        .slot_metrics(output.id)
        .expect("the head has a worker");
    assert_eq!(
        (slots.acquisitions, slots.leased),
        (1, 1),
        "one render in one slot, its lease held"
    );
}
