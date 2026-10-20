//! Real allocation-owner checks before capture credit is issued.
use super::*;

fn capture_metadata() -> sophia_protocol::shell_files::ShellImageMetadata {
    let mut metadata = image_metadata();
    metadata.source_kind = 2;
    metadata.source_format = DRM_FORMAT_XRGB8888;
    metadata.stride = 0;
    metadata
}

#[test]
fn capture_reservation_requires_current_allocation_and_exact_supported_metadata() {
    for case in 0..9 {
        let mut registry = image_registry();
        let mut m = capture_metadata();
        match case {
            0 => m.admission_epoch += 1,
            1 => m.topology_generation += 1,
            2 => m.output_id += 1,
            3 => m.allocation_id += 1,
            4 => m.stride = 800,
            5 => m.source_format = DRM_FORMAT_ARGB8888,
            6 => m.source_kind = 1,
            7 => {
                m.plane_count = 2;
                m.descriptor_count = 2
            }
            _ => m.declared_bytes -= 1,
        }
        assert!(registry.reserve_capture_image(grant(), &m, 0).is_err());
        assert_eq!(
            registry.resources(grant()).unwrap().usage(),
            ContentMemoryUsage::default()
        );
        assert!(
            registry
                .resources_mut(grant())
                .unwrap()
                .take_event()
                .is_none()
        );
        assert!(
            registry
                .reserve_capture_image(grant(), &capture_metadata(), 0)
                .is_ok()
        );
    }
}

#[test]
fn disconnected_registry_retains_credit_and_routes_exact_failure_to_retired_owner() {
    let mut registry = image_registry();
    let m = capture_metadata();
    let mut r = Some(registry.reserve_capture_image(grant(), &m, 10).unwrap());
    assert_eq!(
        registry.resources(grant()).unwrap().usage().backing,
        m.declared_bytes
    );
    assert!(registry.disconnect(grant()));
    assert!(registry.retired_bytes() > 0);
    assert!(registry.reserve_capture_image(grant(), &m, 10).is_err());
    assert_eq!(
        registry
            .capture_stop_reason(r.as_ref().unwrap(), 10)
            .unwrap(),
        Some(ContentReason::Revoked)
    );
    assert_eq!(
        registry.fail_capture(&mut r, ContentCaptureFailure::Renderer, 9),
        Err(ContentStoreError::ClockRegression)
    );
    assert!(r.is_some());
    assert!(registry.retired_bytes() > 0);
    registry
        .fail_capture(&mut r, ContentCaptureFailure::Renderer, 10)
        .unwrap();
    assert!(r.is_none());
    assert_eq!(registry.retired_bytes(), 0);
}

#[test]
fn registry_cannot_settle_a_ticket_from_a_different_identical_owner() {
    let mut a = image_registry();
    let mut b = image_registry();
    let m = capture_metadata();
    let mut r = Some(a.reserve_capture_image(grant(), &m, 0).unwrap());
    let _s = b.reserve_capture_image(grant(), &m, 0).unwrap();
    assert_eq!(
        b.fail_capture(&mut r, ContentCaptureFailure::Renderer, 0),
        Err(ContentStoreError::Stale)
    );
    assert!(r.is_some());
    assert_eq!(
        b.resources(grant()).unwrap().usage().backing,
        m.declared_bytes
    );
    a.fail_capture(&mut r, ContentCaptureFailure::Renderer, 0)
        .unwrap();
}
