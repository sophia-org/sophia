//! Resource accounting controls; no renderer/GPU and no source-release proof.
use super::*;
#[path = "shell_capture_acceptance.rs"]
mod acceptance;
use crate::test_content_isb as isb;

fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 2,
    }
}
fn description(id: u64) -> ContentResourceBegin {
    ContentResourceBegin {
        grant: grant(),
        resource: ContentResourceId { id, generation: 1 },
        width_px: 4,
        height_px: 4,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: 1,
        total_bytes: 64,
    }
}
fn metadata(id: u64) -> ShellImageMetadata {
    ShellImageMetadata {
        offer_id: id,
        resource: id,
        resource_generation: 1,
        admission_epoch: 2,
        output_id: 1,
        allocation_id: 1,
        topology_generation: 1,
        source_kind: 2,
        source_format: DRM_FORMAT_XRGB8888,
        width: 4,
        height: 4,
        stride: 0,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 64,
        modifier: 0,
    }
}
fn owner() -> ContentResourceStore {
    ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap()
}
fn reserve(store: &mut ContentResourceStore, now: u64) -> Option<ContentCaptureReservation> {
    Some(
        store
            .begin_capture(description(1), metadata(1), now)
            .unwrap(),
    )
}
fn held(store: &ContentResourceStore) {
    let u = store.usage();
    assert_eq!(
        (
            u.staging,
            u.reserved_resident,
            u.backing,
            u.resident,
            u.retiring
        ),
        (64, 64, 64, 0, 0)
    );
    assert_eq!(store.response_credits, 2);
    assert!(!store.quiescent());
}

#[test]
fn failure_returns_logical_credit_once_without_acceptance_or_release_events() {
    let mut store = owner();
    let mut r = reserve(&mut store, 0);
    assert_eq!(r.as_ref().unwrap().description(), &description(1));
    assert_eq!(r.as_ref().unwrap().metadata(), &metadata(1));
    held(&store);
    store
        .fail_capture(&mut r, ContentCaptureFailure::Renderer, 0)
        .unwrap();
    assert!(r.is_none());
    assert_eq!(store.usage(), ContentMemoryUsage::default());
    assert_eq!(store.response_credits, 0);
    let event = store.take_event().unwrap();
    let ShellContentRecord::ResourceStatus(status) = event.record else {
        panic!("only failure status")
    };
    assert_eq!(status.reason, ContentReason::RendererFailed as u16);
    assert!(store.take_event().is_none());
    assert!(matches!(
        store.lease(grant(), description(1).resource),
        Err(ContentStoreError::Stale)
    ));
    assert_eq!(
        store.fail_capture(&mut r, ContentCaptureFailure::Renderer, 0),
        Err(ContentStoreError::Stale)
    );
    assert!(store.take_event().is_none());
}

#[test]
fn cpu_completion_never_consumes_capture_credit_or_its_result() {
    let mut store = owner();
    let _r = reserve(&mut store, 0);
    for valid in [false, true] {
        let mut result = Some(if valid {
            Ok(isb::validated(&[255; 64]))
        } else {
            Err(ContentStoreError::Malformed)
        });
        assert_eq!(
            store.complete_isb(grant(), description(1).resource, &mut result, 0),
            Err(ContentStoreError::Stale)
        );
        assert!(result.is_some());
        held(&store);
        assert!(store.take_event().is_none());
    }
}

#[test]
fn identical_names_in_another_store_do_not_settle_the_wrong_transfer() {
    let mut original = owner();
    let mut r = reserve(&mut original, 0);
    for capture in [false, true] {
        let mut other = owner();
        let _other_ticket = if capture {
            reserve(&mut other, 0)
        } else {
            other
                .begin_isb(TransactionId::from_raw(1), description(1), 0)
                .unwrap();
            None
        };
        assert_eq!(
            other.fail_capture(&mut r, ContentCaptureFailure::Renderer, 0),
            Err(ContentStoreError::Stale)
        );
        assert!(r.is_some());
        held(&other);
        held(&original);
    }
    original
        .fail_capture(&mut r, ContentCaptureFailure::Renderer, 0)
        .unwrap();
}

#[test]
fn withdrawal_and_expiry_keep_credit_until_settlement_and_keep_first_reason() {
    for reason in [
        ContentReason::Cancelled,
        ContentReason::Timeout,
        ContentReason::Revoked,
    ] {
        let mut store = owner();
        let mut r = reserve(&mut store, 0);
        let now = match reason {
            ContentReason::Cancelled => {
                store
                    .cancel(
                        TransactionId::from_raw(5),
                        &ContentResourceCancel {
                            grant: grant(),
                            resource: description(1).resource,
                        },
                    )
                    .unwrap();
                0
            }
            ContentReason::Timeout => u64::from(store.limits.transfer_timeout_ms),
            _ => {
                store.revoke();
                0
            }
        };
        assert_eq!(
            store.capture_stop_reason(r.as_ref().unwrap(), now).unwrap(),
            Some(reason)
        );
        held(&store);
        assert!(store.take_event().is_none());
        store
            .fail_capture(&mut r, ContentCaptureFailure::Renderer, now)
            .unwrap();
        assert_eq!(store.usage(), ContentMemoryUsage::default());
        let ShellContentRecord::ResourceStatus(status) = store.take_event().unwrap().record else {
            panic!("failure status")
        };
        assert_eq!(status.reason, reason as u16);
    }
}

#[test]
fn clock_regression_preserves_ticket_and_credit_for_retry() {
    let mut store = owner();
    let mut r = reserve(&mut store, 10);
    assert_eq!(
        store.fail_capture(&mut r, ContentCaptureFailure::Renderer, 9),
        Err(ContentStoreError::ClockRegression)
    );
    assert!(r.is_some());
    held(&store);
    assert!(store.take_event().is_none());
    store
        .fail_capture(&mut r, ContentCaptureFailure::Renderer, 10)
        .unwrap();
    assert!(r.is_none());
}

#[test]
fn losing_ticket_cannot_refund_uncertain_custody() {
    let mut store = owner();
    drop(reserve(&mut store, 0));
    store.revoke();
    store.collect();
    held(&store);
}

#[test]
fn cpu_and_capture_share_quota_and_generation_history() {
    let mut limits = ContentLimits::prototype(grant());
    limits.max_open_transfers = 1;
    let mut store = ContentResourceStore::new(limits).unwrap();
    let mut r = reserve(&mut store, 0);
    assert_eq!(
        store.begin_isb(TransactionId::from_raw(2), description(2), 0),
        Err(ContentStoreError::Budget)
    );
    store
        .fail_capture(&mut r, ContentCaptureFailure::Renderer, 0)
        .unwrap();
    assert_eq!(
        store.begin_isb(TransactionId::from_raw(1), description(1), 0),
        Err(ContentStoreError::Stale)
    );
    let mut next = description(1);
    next.resource.generation = 2;
    store
        .begin_isb(TransactionId::from_raw(2), next, 0)
        .unwrap();
    held(&store);
}

#[test]
fn capture_failure_does_not_release_an_existing_cpu_lease() {
    let mut store = owner();
    isb::accept(
        &mut store,
        TransactionId::from_raw(1),
        description(1),
        &[255; 64],
        0,
    );
    let lease = store.lease(grant(), description(1).resource).unwrap();
    let mut r = Some(store.begin_capture(description(2), metadata(2), 0).unwrap());
    assert_eq!(store.usage().backing, 128);
    store
        .fail_capture(&mut r, ContentCaptureFailure::Renderer, 0)
        .unwrap();
    assert_eq!(store.usage().backing, 64);
    assert_eq!(store.usage().resident, 64);
    assert_eq!(lease.cpu_bytes().unwrap(), &[255; 64]);
}
