#![cfg(test)]

use super::*;
use crate::prelude::*;

use crate::drm::framebuffer_test_device::Device;

fn submit(
    device: &Device,
    exporter: &mut Exporter,
) -> LiveRenderedPrimaryPlaneScanoutSubmitResult<NativeGbmRenderedScanoutOwner> {
    let selection = LibdrmNativePrimaryPlaneSelectionResult {
        status: LibdrmNativePrimaryPlaneSelectionStatus::Selected,
        selection: Some(LibdrmNativePrimaryPlaneSelection::new(
            drm::control::from_u32(11).unwrap(),
            drm::control::from_u32(12).unwrap(),
            drm::control::from_u32(13).unwrap(),
            SIZE,
            None,
        )),
    };
    submit_rendered_primary_plane_scanout_from_scanout_target_and_selection_with(
        LiveKmsScanoutTargetStatus::Ready,
        Some(LiveGbmEglFrameTargetRecord::new(SIZE)),
        selection,
        None,
        None,
        device,
        exporter,
    )
}

#[test]
fn framebuffer_capture_comes_from_the_actual_submit_failure_and_owns_the_source() {
    for deferred_cleanup in [false, true] {
        let device = Device::new();
        device.close_failure.set(deferred_cleanup);
        let mut exporter = exporter();
        let (frame, mut peer) = frame();
        let original = LiveRendererFrameCorrelation {
            native: None,
            request: None,
            trace: frame.trace,
            direct_scanout: Some(frame.direct_scanout),
        };
        let descriptor = frame.direct_scanout_buffer(SIZE).unwrap().descriptor;
        device.rejected_descriptor.set(descriptor);
        exporter.set_pending_mixed_frame(frame);

        let mut result = submit(&device, &mut exporter);
        assert_eq!(
            result.status,
            LiveRenderedPrimaryPlaneScanoutSubmitStatus::ScanoutExportPending
        );
        assert_eq!(
            result.resources,
            Some(LibdrmNativePrimaryPlaneResourceCreateStatus::FramebufferCreateFailed)
        );
        assert!(matches!(
            result.framebuffer,
            Some(
                LibdrmNativePrimaryPlaneFramebufferCreateDetail::AddFb2ModifiersFailed {
                    error_kind: io::ErrorKind::InvalidInput,
                    raw_os_error: Some(22),
                }
            )
        ));
        assert_eq!(device.imports.get(), 1);
        assert_eq!(device.addfb_calls.get(), 1);
        assert!(device.tests.borrow().is_empty());
        assert!(result.atomic_test.is_none());
        assert!(result.layout_witness.is_none());
        assert!(result.submission.is_none());
        assert_eq!(exporter.direct_scanout_tests(), 0);
        assert_eq!(exporter.direct_scanout_fallbacks(), 1);
        assert_eq!(exporter.discovery().opens.get(), 0);
        assert!(exporter.pending_mixed_frame());

        assert_eq!(result.cleanup.is_some(), deferred_cleanup);
        if let Some(cleanup) = result.cleanup.take() {
            assert!(device.closed.borrow().is_empty());
            device.close_failure.set(false);
            assert!(
                retry_rendered_primary_plane_scanout_cleanup(&device, cleanup)
                    .cleanup
                    .is_none()
            );
        }
        assert_eq!(*device.closed.borrow(), [200]);
        assert!(device.destroyed.borrow().is_empty());
        drop(result);

        let (completed, bound) = bind_owned_fallback(&mut exporter);
        assert!(
            bound,
            "the real submission hook must capture before fallback"
        );
        assert_eq!(completed.trace, original.trace);
        assert_retained(&mut peer);
        let source = exporter
            .take_layout_probe_source(Some(completed), alternative())
            .expect("the supported composed owner receives the exact original");
        assert_eq!(source.export.correlation, Some(original));
        assert_eq!(source.export.descriptor, Some(descriptor));
        assert_eq!(source.image, LiveRendererImageId::from_raw(11));
        let NativeGbmRenderedScanoutOwner::Direct(buffer) = source.export.owner.as_ref().unwrap()
        else {
            panic!("the retained source must own the client's allocation");
        };
        assert_eq!(buffer.image_id, source.image);
        assert_retained(&mut peer);
        assert!(
            exporter
                .take_layout_probe_source(Some(completed), alternative())
                .is_none()
        );
        drop(source);
        assert_released(&mut peer);
    }
}

#[test]
fn framebuffer_capture_excludes_prime_and_non_einval_failures_without_losing_fallback() {
    for (prime_failure, errno) in [(true, 22), (false, 5), (false, 11), (false, 16)] {
        let device = Device::new();
        device.errno.set(Some(errno));
        device.import_failure.set(prime_failure.then_some(1));
        let mut exporter = exporter();
        let (frame, mut peer) = frame();
        device
            .rejected_descriptor
            .set(frame.direct_scanout_buffer(SIZE).unwrap().descriptor);
        exporter.set_pending_mixed_frame(frame);

        let result = submit(&device, &mut exporter);
        assert_eq!(
            result.status,
            LiveRenderedPrimaryPlaneScanoutSubmitStatus::ScanoutExportPending
        );
        assert_eq!(
            result.resources,
            Some(if prime_failure {
                LibdrmNativePrimaryPlaneResourceCreateStatus::BufferImportFailed
            } else {
                LibdrmNativePrimaryPlaneResourceCreateStatus::FramebufferCreateFailed
            })
        );
        assert_eq!(device.imports.get(), 1);
        assert_eq!(device.addfb_calls.get(), usize::from(!prime_failure));
        assert!(device.tests.borrow().is_empty());
        assert!(result.atomic_test.is_none());
        assert!(result.layout_witness.is_none());
        assert!(result.submission.is_none());
        assert!(result.cleanup.is_none());
        assert_eq!(device.closed.borrow().len(), usize::from(!prime_failure));
        assert_eq!(exporter.direct_scanout_fallbacks(), 1);
        assert!(exporter.pending_mixed_frame());
        drop(result);

        let (completed, bound) = bind_owned_fallback(&mut exporter);
        assert!(!bound, "prime_failure={prime_failure} errno={errno}");
        assert!(
            exporter
                .take_layout_probe_source(Some(completed), alternative())
                .is_none()
        );
        assert_released(&mut peer);
    }
}
