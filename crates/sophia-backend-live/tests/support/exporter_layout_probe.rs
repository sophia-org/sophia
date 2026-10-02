#![cfg(test)]

#[path = "framebuffer_capture.rs"]
mod framebuffer_capture;

use super::*;
use crate::{
    LibdrmNativeAtomicCommitFlagsReport, LibdrmNativeAtomicCommitRequestScope,
    LibdrmNativeAtomicCommitSubmitStatus as TestStatus, LibdrmNativeAtomicTestReport,
    LibdrmNativePlaneFormatSnapshotUnknown,
};
use sophia_engine::{DirectScanoutVerdict, HeadSamplingClass, RenderHeadId};
use sophia_protocol::{DRM_FORMAT_XRGB8888, OutputId, Rect, Size, Transform};
use sophia_renderer_live::{
    LiveCompositionPlacement, LiveCompositionTrace, LiveGbmEglFrameTargetRecord,
    LiveOwnedDmaBufPlane, LiveOwnedMixedCompositionFrame, LiveOwnedMixedCompositionLayer,
    LiveOwnedMultiPlaneDmaBufFrame, LiveRendererImageId, LiveRendererScanoutBufferExportStatus,
};
use std::cell::Cell;
use std::io::{self, Read};
use std::os::unix::net::UnixStream;

const SIZE: Size = Size {
    width: 640,
    height: 480,
};
const TILED: u64 = 0x0200_0000_0040_1b03;

#[derive(Default)]
struct MissingRenderDevice {
    opens: Cell<usize>,
}

impl RenderDeviceDiscoveryBackend for MissingRenderDevice {
    type Device = std::fs::File;

    fn open_render_device(&self) -> io::Result<Self::Device> {
        self.opens.set(self.opens.get() + 1);
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "test render device unavailable",
        ))
    }
}

type Exporter = NativeGbmRenderedScanoutBufferDiscoveryExporter<MissingRenderDevice>;

fn formats(modifiers: &[u64]) -> LibdrmNativePlaneFormatSnapshot {
    let mut bytes = vec![0; 32 + modifiers.len() * 24];
    for (offset, value) in [
        (0, 1),
        (8, 1),
        (12, 24),
        (16, modifiers.len() as u32),
        (20, 32),
        (24, DRM_FORMAT_XRGB8888),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
    }
    for (index, modifier) in modifiers.iter().enumerate() {
        let start = 32 + index * 24;
        bytes[start..start + 8].copy_from_slice(&1u64.to_ne_bytes());
        bytes[start + 16..start + 24].copy_from_slice(&modifier.to_ne_bytes());
    }
    let snapshot = LibdrmNativePlaneFormatSnapshot::parse(13, 77, &bytes);
    assert_eq!(snapshot.unknown_reason(), None);
    snapshot
}

fn exporter() -> Exporter {
    let mut exporter =
        Exporter::new(MissingRenderDevice::default()).with_layout_probe_formats(formats(&[0]));
    exporter.set_direct_scanout_enabled(true);
    exporter.set_layout_probe_available(true);
    exporter
}

// The descriptor owns a socket endpoint so retaining its last duplicate is observable.
// The exporter only checks and duplicates it; no renderer or KMS import is attempted.
fn frame() -> (LiveOwnedMixedCompositionFrame, UnixStream) {
    let (owned, peer) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    (
        LiveOwnedMixedCompositionFrame {
            image_reads: Default::default(),
            trace: Some(LiveCompositionTrace {
                output: OutputId::from_raw(1),
                head: RenderHeadId::from_raw(2),
                scene_generation: 91,
            }),
            layers: vec![LiveOwnedMixedCompositionLayer::DmaBuf {
                image_id: LiveRendererImageId::from_raw(11),
                frame: LiveOwnedMultiPlaneDmaBufFrame {
                    width: SIZE.width as u32,
                    height: SIZE.height as u32,
                    format: DRM_FORMAT_XRGB8888,
                    modifier: TILED,
                    plane_count: 1,
                    planes: [
                        Some(LiveOwnedDmaBufPlane {
                            fd: owned.into(),
                            offset: 0,
                            stride: 2_560,
                        }),
                        None,
                        None,
                        None,
                    ],
                },
                placement: LiveCompositionPlacement {
                    target: Rect {
                        x: 0,
                        y: 0,
                        width: SIZE.width,
                        height: SIZE.height,
                    },
                    clip: None,
                    transform: Transform::IDENTITY,
                    alpha: 1.0,
                    sampling: HeadSamplingClass::Exact,
                },
            }],
            output_damage_snapshot: None,
            direct_scanout: DirectScanoutVerdict::Eligible,
        },
        peer,
    )
}

fn test_result(status: TestStatus) -> LibdrmNativeAtomicTestReport {
    LibdrmNativeAtomicTestReport {
        status,
        request: None,
        error_kind: (status == TestStatus::Rejected).then_some(io::ErrorKind::InvalidInput),
        raw_os_error: (status == TestStatus::Rejected).then_some(22),
        request_scope: LibdrmNativeAtomicCommitRequestScope::PageFlip,
        commit_flags: LibdrmNativeAtomicCommitFlagsReport {
            page_flip_event: false,
            nonblocking: true,
            allow_modeset: false,
            test_only: true,
        },
    }
}

fn offer_and_fall_back(
    exporter: &mut Exporter,
    status: TestStatus,
) -> (
    LiveRendererFrameCorrelation,
    LiveRendererScanoutBufferDescriptor,
    UnixStream,
) {
    let (frame, peer) = frame();
    exporter.set_pending_mixed_frame(frame);
    let direct = exporter.export_rendered_scanout_buffer(LiveGbmEglFrameTargetRecord::new(SIZE));
    assert_eq!(
        direct.status,
        LiveRendererScanoutBufferExportStatus::Exported
    );
    let correlation = direct
        .correlation
        .expect("the owned direct frame has a trace");
    let descriptor = direct.descriptor.unwrap();
    assert_eq!(descriptor.modifier, Some(TILED));
    assert!(direct.owner.as_ref().unwrap().is_direct_client_buffer());
    assert_eq!(exporter.discovery().opens.get(), 0);
    exporter.record_direct_scanout_test_result(test_result(status));
    assert!(LiveRenderedScanoutBufferExporter::fall_back_from_direct(
        exporter
    ));
    drop(direct);
    (correlation, descriptor, peer)
}

fn bind_owned_fallback(exporter: &mut Exporter) -> (LiveRendererFrameCorrelation, bool) {
    let frame = exporter
        .pending_frame
        .take()
        .expect("fallback remains owned");
    let correlation = super::super::worker::frame_correlation(&frame, None);
    assert_eq!(
        correlation.direct_scanout,
        Some(DirectScanoutVerdict::CompositionRequired("refused"))
    );
    let bound = exporter
        .layout_probe
        .candidate
        .bind(correlation, Instant::now());
    drop(frame);
    (correlation, bound)
}

fn alternative() -> LiveRendererScanoutBufferDescriptor {
    let mut descriptor =
        LiveRendererScanoutBufferDescriptor::new(SIZE, 2_560, DRM_FORMAT_XRGB8888, 101);
    descriptor.modifier = Some(0);
    descriptor
}

fn assert_retained(peer: &mut UnixStream) {
    assert_eq!(
        peer.read(&mut [0]).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

fn assert_released(peer: &mut UnixStream) {
    assert_eq!(peer.read(&mut [0]).unwrap(), 0);
}

#[test]
fn a_refused_direct_export_retains_its_allocation_through_the_actual_fallback() {
    let mut exporter = exporter();
    let (original, descriptor, mut peer) = offer_and_fall_back(&mut exporter, TestStatus::Rejected);
    assert_eq!(exporter.direct_scanout_tests(), 1);
    assert_eq!(exporter.direct_scanout_fallbacks(), 1);
    let (completed, bound) = bind_owned_fallback(&mut exporter);
    assert!(bound);
    assert_eq!(completed.trace, original.trace);
    assert_retained(&mut peer);

    let source = exporter
        .take_layout_probe_source(Some(completed), alternative())
        .expect("the supported fallback receives the retained original");
    assert_eq!(source.export.correlation, Some(original));
    assert_eq!(source.export.descriptor, Some(descriptor));
    let NativeGbmRenderedScanoutOwner::Direct(ref buffer) = *source.export.owner.as_ref().unwrap()
    else {
        panic!("the probe must receive the client's allocation");
    };
    assert_eq!(buffer.image_id, LiveRendererImageId::from_raw(11));
    assert_eq!(source.image, buffer.image_id);
    assert_retained(&mut peer);
    assert!(
        exporter
            .take_layout_probe_source(Some(completed), alternative())
            .is_none()
    );
    drop(source);
    assert_released(&mut peer);
}

#[test]
fn capture_requires_a_refusal_known_absence_and_a_supported_alternative() {
    let unknown = LibdrmNativePlaneFormatSnapshot::unavailable(
        13,
        LibdrmNativePlaneFormatSnapshotUnknown::ReadFailed,
    );
    for (snapshot, available, status) in [
        (Some(unknown), true, TestStatus::Rejected),
        (None, true, TestStatus::Rejected),
        (Some(formats(&[])), true, TestStatus::Rejected),
        (Some(formats(&[0, TILED])), true, TestStatus::Rejected),
        (Some(formats(&[0])), false, TestStatus::Rejected),
        (Some(formats(&[0])), true, TestStatus::Submitted),
        (Some(formats(&[0])), true, TestStatus::WouldBlock),
    ] {
        let mut exporter = Exporter::new(MissingRenderDevice::default());
        if let Some(snapshot) = snapshot {
            exporter = exporter.with_layout_probe_formats(snapshot);
        }
        exporter.set_direct_scanout_enabled(true);
        exporter.set_layout_probe_available(available);
        let (_, _, mut peer) = offer_and_fall_back(&mut exporter, status);
        let (completed, bound) = bind_owned_fallback(&mut exporter);
        assert!(!bound);
        assert!(
            exporter
                .take_layout_probe_source(Some(completed), alternative())
                .is_none()
        );
        assert_released(&mut peer);
    }
}

#[test]
fn an_unknown_or_unsupported_completed_layout_cannot_take_the_retained_source() {
    for modifier in [
        None,
        Some(TILED),
        Some(TILED + 1),
        Some(u64::MAX),
        Some(sophia_protocol::DRM_FORMAT_MOD_INVALID),
    ] {
        let mut exporter = exporter();
        let (_, _, mut peer) = offer_and_fall_back(&mut exporter, TestStatus::Rejected);
        let (completed, bound) = bind_owned_fallback(&mut exporter);
        assert!(bound);
        let mut descriptor = alternative();
        descriptor.modifier = modifier;
        assert!(
            exporter
                .take_layout_probe_source(Some(completed), descriptor)
                .is_none()
        );
        assert_released(&mut peer);
    }
}

#[test]
fn an_external_offer_with_the_same_trace_still_releases_the_old_source() {
    let mut exporter = exporter();
    let (original, _, mut old_peer) = offer_and_fall_back(&mut exporter, TestStatus::Rejected);
    let (completed, bound) = bind_owned_fallback(&mut exporter);
    assert!(bound);
    let (new_frame, mut new_peer) = frame();
    assert_eq!(new_frame.trace, original.trace);
    exporter.set_pending_mixed_frame(new_frame);
    assert_released(&mut old_peer);
    assert_retained(&mut new_peer);
    assert!(
        exporter
            .take_layout_probe_source(Some(completed), alternative())
            .is_none()
    );
    assert!(exporter.discard_pending_frame());
    assert_released(&mut new_peer);
}

#[test]
fn owner_invalidation_releases_a_source_after_its_fallback_left_the_queue() {
    for invalidation in 0..5 {
        let mut exporter = exporter();
        let (_, _, mut peer) = offer_and_fall_back(&mut exporter, TestStatus::Rejected);
        let (completed, bound) = bind_owned_fallback(&mut exporter);
        assert!(bound);
        match invalidation {
            0 => exporter.set_direct_scanout_enabled(false),
            1 => assert!(!exporter.discard_pending_frame()),
            2 => exporter.set_layout_probe_available(false),
            3 => exporter = exporter.with_layout_probe_formats(formats(&[0])),
            4 => exporter.expire_layout_probe(Instant::now() + PROBE_INTERVAL),
            _ => unreachable!(),
        }
        assert_released(&mut peer);
        assert!(
            exporter
                .take_layout_probe_source(Some(completed), alternative())
                .is_none()
        );
    }
}

#[test]
fn new_offers_cannot_restart_the_capture_interval() {
    let mut exporter = exporter();
    let (_, _, mut first_peer) = offer_and_fall_back(&mut exporter, TestStatus::Rejected);
    let (completed, bound) = bind_owned_fallback(&mut exporter);
    assert!(bound);
    drop(
        exporter
            .take_layout_probe_source(Some(completed), alternative())
            .unwrap(),
    );
    assert_released(&mut first_peer);

    let last_capture = exporter
        .layout_probe
        .last_capture
        .expect("successful capture is timed");
    let (_, _, mut second_peer) = offer_and_fall_back(&mut exporter, TestStatus::Rejected);
    let (_, bound) = bind_owned_fallback(&mut exporter);
    assert!(!bound);
    assert_eq!(exporter.layout_probe.last_capture, Some(last_capture));
    assert_released(&mut second_peer);

    // Advance the eligibility boundary without sleeping or changing production's clock.
    exporter.layout_probe.last_capture = Some(Instant::now() - PROBE_INTERVAL);
    exporter.layout_probe.last_attempt = Some(Instant::now() - PROBE_INTERVAL);
    let (_, _, mut third_peer) = offer_and_fall_back(&mut exporter, TestStatus::Rejected);
    let (completed, bound) = bind_owned_fallback(&mut exporter);
    assert!(bound);
    drop(
        exporter
            .take_layout_probe_source(Some(completed), alternative())
            .unwrap(),
    );
    assert_released(&mut third_peer);
}

#[test]
fn recent_pair_work_still_blocks_a_source_captured_after_an_older_interval() {
    let mut exporter = exporter();
    let (_, _, mut first_peer) = offer_and_fall_back(&mut exporter, TestStatus::Rejected);
    let (completed, bound) = bind_owned_fallback(&mut exporter);
    assert!(bound);
    drop(
        exporter
            .take_layout_probe_source(Some(completed), alternative())
            .unwrap(),
    );
    assert_released(&mut first_peer);
    let last_attempt = exporter
        .layout_probe
        .last_attempt
        .expect("source transfer is timed");

    exporter.layout_probe.last_capture = Some(Instant::now() - PROBE_INTERVAL);
    let (_, _, mut next_peer) = offer_and_fall_back(&mut exporter, TestStatus::Rejected);
    let (completed, bound) = bind_owned_fallback(&mut exporter);
    assert!(bound);
    assert_retained(&mut next_peer);
    assert_eq!(exporter.layout_probe.last_attempt, Some(last_attempt));
    assert!(
        exporter
            .take_layout_probe_source(Some(completed), alternative())
            .is_none()
    );
    assert_eq!(exporter.layout_probe.last_attempt, Some(last_attempt));
    assert_released(&mut next_peer);
}

#[test]
fn an_unavailable_inline_renderer_drops_the_optional_source_and_keeps_the_frame() {
    let mut exporter = exporter();
    let (_, _, mut peer) = offer_and_fall_back(&mut exporter, TestStatus::Rejected);
    assert!(exporter.layout_probe.last_capture.is_some());
    let result = exporter.export_rendered_scanout_buffer(LiveGbmEglFrameTargetRecord::new(SIZE));
    assert_eq!(
        result.status,
        LiveRendererScanoutBufferExportStatus::Unavailable
    );
    assert!(result.owner.is_none());
    assert_eq!(exporter.discovery().opens.get(), 1);
    assert!(exporter.pending_mixed_frame());
    assert_retained(&mut peer);
    let (completed, bound) = bind_owned_fallback(&mut exporter);
    assert!(!bound);
    assert!(
        exporter
            .take_layout_probe_source(Some(completed), alternative())
            .is_none()
    );
    assert_released(&mut peer);
}
