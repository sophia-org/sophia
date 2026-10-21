//! Retired IPC resource semantics retained only by the comparator.
use sophia_protocol::*;
use sophia_runtime::*;
#[path = "support/content_isb.rs"]
#[allow(dead_code)]
mod isb;

fn description() -> ContentResourceBegin {
    ContentResourceBegin {
        grant: ContentGrant {
            connection_epoch: 11,
            content_grant_epoch: 3,
        },
        resource: ContentResourceId {
            id: 1,
            generation: 1,
        },
        width_px: 2,
        height_px: 1,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: 1,
        total_bytes: 8,
    }
}

#[test]
fn byte_upload_retains_consumers_and_releases_without_image_journal_confirmation() {
    let d = description();
    let mut store = ContentResourceStore::new(ContentLimits::prototype(d.grant)).unwrap();
    let tx = TransactionId::from_raw(1);
    let bytes = vec![0, 0, 255, 255, 0, 128, 0, 128];
    store.begin(tx, d.clone(), 0).unwrap();
    assert!(store.lease(d.grant, d.resource).is_err());
    store
        .chunk(
            tx,
            &ContentResourceChunk {
                grant: d.grant,
                resource: d.resource,
                ordinal: 0,
                offset: 0,
                bytes: bytes.clone(),
            },
            1,
        )
        .unwrap();
    store
        .end(
            tx,
            &ContentResourceEnd {
                grant: d.grant,
                resource: d.resource,
                chunk_count: 1,
                total_bytes: 8,
            },
            2,
        )
        .unwrap();
    while store.take_event().is_some() {}
    let lease = store.lease(d.grant, d.resource).unwrap();
    let renderer = lease.clone().into_cpu().ok().unwrap();
    store
        .retire(
            tx,
            &ContentResourceRetire {
                grant: d.grant,
                resource: d.resource,
            },
        )
        .unwrap();
    drop(lease);
    store.collect();
    assert!(store.take_event().is_none());
    assert_eq!(renderer.bytes(), bytes);
    assert_eq!(store.usage().backing, 8);
    drop(renderer);
    store.collect();
    assert_eq!(store.usage().backing, 0);
    assert!(matches!(
        store.take_event().unwrap().record,
        ShellContentRecord::ResourceReleased(_)
    ));
    assert!(store.take_event().is_none());
    assert!(store.quiescent());
}

#[test]
fn byte_idle_timeout_does_not_change_production_sealed_timeout() {
    let d = description();
    let mut limits = ContentLimits::prototype(d.grant);
    limits.transfer_idle_timeout_ms = 10;
    limits.transfer_timeout_ms = 100;
    let mut bytes = ContentResourceStore::new(limits.clone()).unwrap();
    let mut sealed = ContentResourceStore::new(limits.clone()).unwrap();
    let mut expired = ContentResourceStore::new(limits).unwrap();
    let tx = TransactionId::from_raw(1);
    bytes.begin(tx, d.clone(), 0).unwrap();
    sealed.begin_isb(tx, d.clone(), 0).unwrap();
    expired.begin_isb(tx, d.clone(), 0).unwrap();
    bytes.expire(10).unwrap();
    sealed.expire(10).unwrap();
    assert_eq!(bytes.usage().staging, 0);
    assert_eq!(sealed.usage().staging, 8);
    sealed
        .complete_isb(
            d.grant,
            d.resource,
            &mut Some(Ok(isb::validated(&[0; 8]))),
            10,
        )
        .unwrap();
    assert!(sealed.lease(d.grant, d.resource).is_ok());
    expired.expire(100).unwrap();
    assert_eq!(expired.usage().backing, 8);
    assert_eq!(
        expired.complete_isb(
            d.grant,
            d.resource,
            &mut Some(Ok(isb::validated(&[0; 8]))),
            100
        ),
        Err(ContentStoreError::Stale)
    );
    assert_eq!(expired.usage().backing, 0);
}
