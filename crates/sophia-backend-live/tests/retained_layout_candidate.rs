#![cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]

#[path = "../src/scanout/rendered_scanout/exporter/correlation.rs"]
mod correlation;
#[path = "../src/scanout/rendered_scanout/exporter/layout_candidate.rs"]
mod layout_candidate;

use correlation::{LiveRendererFrameCorrelation, LiveRendererWorkerRequestId};
use layout_candidate::RetainedLayoutCandidate;
#[path = "../src/scanout/rendered_scanout/exporter/native_identity.rs"]
mod native_identity;
use native_identity::{LiveNativeFrameIdentity, NativeFrameOwner};
use sophia_engine::{DirectScanoutVerdict, RenderHeadId};
use sophia_protocol::{DRM_FORMAT_ARGB8888, DRM_FORMAT_XRGB8888, OutputId, Size};
use sophia_renderer_live::{
    LiveCompositionTrace, LiveDirectScanoutBuffer, LiveOwnedDmaBufPlane, LiveRendererImageId,
    LiveRendererScanoutBufferDescriptor,
};
use std::io::Read;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

const LIMIT: Duration = Duration::from_secs(1);
const SIZE: Size = Size {
    width: 16,
    height: 16,
};

fn original() -> LiveRendererFrameCorrelation {
    LiveRendererFrameCorrelation {
        native: None,
        request: None,
        trace: Some(LiveCompositionTrace {
            output: OutputId::from_raw(1),
            head: RenderHeadId::from_raw(2),
            scene_generation: 3,
        }),
        direct_scanout: Some(DirectScanoutVerdict::Eligible),
    }
}

fn fallback(request: Option<u64>) -> LiveRendererFrameCorrelation {
    LiveRendererFrameCorrelation {
        native: None,
        request: request.map(LiveRendererWorkerRequestId),
        direct_scanout: Some(DirectScanoutVerdict::CompositionRequired("refused")),
        ..original()
    }
}

fn descriptor(format: u32) -> LiveRendererScanoutBufferDescriptor {
    LiveRendererScanoutBufferDescriptor::new(SIZE, 64, format, 1)
}

// A socket endpoint tests descriptor ownership only; no GPU import is attempted.
fn source() -> (LiveDirectScanoutBuffer, UnixStream) {
    let (owned, peer) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    (
        LiveDirectScanoutBuffer {
            descriptor: descriptor(DRM_FORMAT_ARGB8888),
            planes: [
                Some(LiveOwnedDmaBufPlane {
                    fd: owned.into(),
                    offset: 0,
                    stride: 64,
                }),
                None,
                None,
                None,
            ],
            image_id: LiveRendererImageId::from_raw(4),
        },
        peer,
    )
}

fn assert_retained(peer: &mut UnixStream) {
    assert_eq!(
        peer.read(&mut [0]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

fn assert_released(peer: &mut UnixStream) {
    assert_eq!(peer.read(&mut [0]).unwrap(), 0);
}

#[test]
fn an_exact_worker_completion_transfers_the_original_source_once() {
    let now = Instant::now();
    let (buffer, mut peer) = source();
    let mut candidate = RetainedLayoutCandidate::default();
    assert!(candidate.capture(buffer, original(), now, now + LIMIT));
    assert!(candidate.bind(fallback(Some(10)), now));
    assert_retained(&mut peer);
    let source = candidate
        .take(fallback(Some(10)), descriptor(DRM_FORMAT_ARGB8888), now)
        .unwrap();
    assert_eq!(source.original, original());
    assert_eq!(source.buffer.image_id, LiveRendererImageId::from_raw(4));
    assert_eq!(source.buffer.descriptor, descriptor(DRM_FORMAT_ARGB8888));
    assert!(
        candidate
            .take(fallback(Some(10)), descriptor(DRM_FORMAT_ARGB8888), now)
            .is_none()
    );
    assert_retained(&mut peer);
    drop(source);
    assert_released(&mut peer);
}

#[test]
fn unknown_or_ineligible_originals_never_retain_an_allocation() {
    let now = Instant::now();
    for invalid in [
        LiveRendererFrameCorrelation {
            trace: None,
            ..original()
        },
        LiveRendererFrameCorrelation {
            direct_scanout: None,
            ..original()
        },
        LiveRendererFrameCorrelation {
            direct_scanout: Some(DirectScanoutVerdict::LayerNotActive),
            ..original()
        },
        LiveRendererFrameCorrelation {
            native: None,
            request: Some(LiveRendererWorkerRequestId(1)),
            ..original()
        },
    ] {
        let (buffer, mut peer) = source();
        let mut candidate = RetainedLayoutCandidate::default();
        assert!(!candidate.capture(buffer, invalid, now, now + LIMIT));
        assert!(!candidate.bind(fallback(Some(1)), now));
        assert_released(&mut peer);
    }
}

#[test]
fn missing_or_changed_fallback_identity_cannot_take_or_rebind_the_source() {
    let now = Instant::now();
    let (buffer, mut peer) = source();
    let mut candidate = RetainedLayoutCandidate::default();
    assert!(candidate.capture(buffer, original(), now, now + LIMIT));
    assert!(!candidate.bind(
        LiveRendererFrameCorrelation {
            trace: None,
            ..fallback(Some(10))
        },
        now
    ));
    assert!(!candidate.bind(original(), now));
    assert!(
        candidate
            .take(fallback(Some(10)), descriptor(DRM_FORMAT_ARGB8888), now)
            .is_none()
    );
    assert!(candidate.bind(fallback(Some(10)), now));
    for wrong in [
        fallback(Some(11)),
        fallback(None),
        LiveRendererFrameCorrelation {
            trace: None,
            ..fallback(Some(10))
        },
        LiveRendererFrameCorrelation {
            direct_scanout: Some(DirectScanoutVerdict::Eligible),
            ..fallback(Some(10))
        },
        LiveRendererFrameCorrelation {
            trace: Some(LiveCompositionTrace {
                scene_generation: 99,
                ..original().trace.unwrap()
            }),
            ..fallback(Some(10))
        },
    ] {
        assert!(!candidate.bind(wrong, now));
        assert!(!candidate.unbind_deferred(wrong, now));
        assert!(
            candidate
                .take(wrong, descriptor(DRM_FORMAT_ARGB8888), now)
                .is_none()
        );
        assert_retained(&mut peer);
    }
    assert!(
        candidate
            .take(fallback(Some(10)), descriptor(DRM_FORMAT_ARGB8888), now)
            .is_some()
    );
    assert_released(&mut peer);
}

#[test]
fn inline_rendering_requires_an_explicit_bind_and_external_offers_invalidate_even_equal_traces() {
    let now = Instant::now();
    let (buffer, mut peer) = source();
    let mut candidate = RetainedLayoutCandidate::default();
    assert!(candidate.capture(buffer, original(), now, now + LIMIT));
    assert!(
        candidate
            .take(fallback(None), descriptor(DRM_FORMAT_ARGB8888), now)
            .is_none()
    );
    assert!(candidate.bind(fallback(None), now));
    candidate.invalidate();
    assert_released(&mut peer);
    assert!(!candidate.bind(fallback(None), now));
    assert!(
        candidate
            .take(fallback(None), descriptor(DRM_FORMAT_ARGB8888), now)
            .is_none()
    );
    let (buffer, _) = source();
    assert!(candidate.capture(buffer, original(), now, now + LIMIT));
    assert!(candidate.bind(fallback(None), now));
    assert!(
        candidate
            .take(fallback(None), descriptor(DRM_FORMAT_ARGB8888), now)
            .is_some()
    );
}

#[test]
fn deferral_rebinds_a_new_request_without_extending_the_deadline() {
    let now = Instant::now();
    let (buffer, mut peer) = source();
    let mut candidate = RetainedLayoutCandidate::default();
    assert!(candidate.capture(buffer, original(), now, now + LIMIT));
    assert!(candidate.bind(fallback(Some(10)), now));
    assert!(candidate.unbind_deferred(fallback(Some(10)), now + LIMIT / 2));
    assert!(
        candidate
            .take(
                fallback(Some(10)),
                descriptor(DRM_FORMAT_ARGB8888),
                now + LIMIT / 2
            )
            .is_none()
    );
    assert!(candidate.bind(fallback(Some(11)), now + LIMIT / 2));
    assert!(
        candidate
            .take(
                fallback(Some(10)),
                descriptor(DRM_FORMAT_ARGB8888),
                now + LIMIT / 2
            )
            .is_none()
    );
    assert_retained(&mut peer);
    assert!(
        candidate
            .take(
                fallback(Some(11)),
                descriptor(DRM_FORMAT_ARGB8888),
                now + LIMIT
            )
            .is_none()
    );
    assert_released(&mut peer);
    assert!(!candidate.expire(now + LIMIT));
}

#[test]
fn format_size_and_validity_must_agree_at_the_matching_completion() {
    let now = Instant::now();
    let mut wrong_size = descriptor(DRM_FORMAT_ARGB8888);
    wrong_size.size.width = 15;
    let invalid = LiveRendererScanoutBufferDescriptor::new(SIZE, 0, DRM_FORMAT_ARGB8888, 1);
    for output in [descriptor(DRM_FORMAT_XRGB8888), wrong_size, invalid] {
        let (buffer, mut peer) = source();
        let mut candidate = RetainedLayoutCandidate::default();
        assert!(candidate.capture(buffer, original(), now, now + LIMIT));
        assert!(candidate.bind(fallback(Some(10)), now));
        assert!(candidate.take(fallback(Some(10)), output, now).is_none());
        assert_released(&mut peer);
        assert!(
            candidate
                .take(fallback(Some(10)), descriptor(DRM_FORMAT_ARGB8888), now)
                .is_none()
        );
    }
}

#[test]
fn replacement_and_expiration_release_the_single_retained_source() {
    let now = Instant::now();
    let (first, mut old_peer) = source();
    let (second, mut new_peer) = source();
    let mut candidate = RetainedLayoutCandidate::default();
    assert!(candidate.capture(first, original(), now, now + LIMIT));
    assert!(candidate.capture(second, original(), now, now + LIMIT));
    assert_released(&mut old_peer);
    assert_retained(&mut new_peer);
    assert!(!candidate.expire(now + LIMIT / 2));
    assert!(candidate.expire(now + LIMIT));
    assert_released(&mut new_peer);
    let (expired, mut peer) = source();
    assert!(!candidate.capture(expired, original(), now, now));
    assert_released(&mut peer);
}

#[test]
fn output_format_preference_belongs_to_the_unbound_fallback_and_expires() {
    let now = Instant::now();
    let (buffer, mut peer) = source();
    let mut candidate = RetainedLayoutCandidate::default();
    assert!(candidate.capture(buffer, original(), now, now + LIMIT));
    assert_eq!(candidate.output_format(original(), now), None);
    let mut unrelated = fallback(None);
    unrelated.trace.as_mut().unwrap().scene_generation += 1;
    assert_eq!(candidate.output_format(unrelated, now), None);
    assert_eq!(
        candidate.output_format(fallback(None), now),
        Some(DRM_FORMAT_ARGB8888)
    );
    assert!(candidate.bind(fallback(Some(10)), now));
    assert_eq!(candidate.output_format(fallback(None), now), None);
    assert!(candidate.unbind_deferred(fallback(Some(10)), now + LIMIT / 2));
    assert_eq!(
        candidate.output_format(fallback(None), now + LIMIT / 2),
        Some(DRM_FORMAT_ARGB8888)
    );
    assert_eq!(candidate.output_format(fallback(None), now + LIMIT), None);
    assert_released(&mut peer);
}

#[test]
fn equal_scene_trace_does_not_authorize_another_native_frames_fallback() {
    let now = Instant::now();
    let owner = NativeFrameOwner::new();
    let output = OutputId::from_raw(1);
    let head = RenderHeadId::from_raw(2);
    let exact = owner.frame(output, head, 3, 4);
    assert_ne!(exact.owner(), 0);
    assert_eq!(exact.owner(), owner.frame(output, head, 3, 5).owner());
    assert_ne!(
        exact.owner(),
        NativeFrameOwner::new().frame(output, head, 3, 4).owner()
    );
    assert_eq!(
        (
            exact.output(),
            exact.head(),
            exact.target_generation(),
            exact.frame()
        ),
        (output, head, 3, 4)
    );
    let (buffer, mut peer) = source();
    let mut candidate = RetainedLayoutCandidate::default();
    assert!(candidate.capture(
        buffer,
        LiveRendererFrameCorrelation {
            native: Some(exact),
            ..original()
        },
        now,
        now + LIMIT
    ));
    for native in [
        None,
        Some(NativeFrameOwner::new().frame(output, head, 3, 4)),
        Some(owner.frame(OutputId::from_raw(9), head, 3, 4)),
        Some(owner.frame(output, RenderHeadId::from_raw(9), 3, 4)),
        Some(owner.frame(output, head, 9, 4)),
        Some(owner.frame(output, head, 3, 9)),
    ] {
        let wrong = LiveRendererFrameCorrelation {
            native,
            ..fallback(Some(10))
        };
        assert_eq!(candidate.output_format(wrong, now), None);
        assert!(!candidate.bind(wrong, now));
        assert_retained(&mut peer);
    }
    let matching = LiveRendererFrameCorrelation {
        native: Some(exact),
        ..fallback(Some(10))
    };
    assert_eq!(
        candidate.output_format(matching, now),
        Some(DRM_FORMAT_ARGB8888)
    );
    assert!(candidate.bind(matching, now));
    let retained = candidate
        .take(matching, descriptor(DRM_FORMAT_ARGB8888), now)
        .unwrap();
    assert_eq!(retained.original.native, Some(exact));
    assert_retained(&mut peer);
    drop(retained);
    assert_released(&mut peer);
}
