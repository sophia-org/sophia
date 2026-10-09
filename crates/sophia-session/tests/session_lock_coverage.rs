//! Diagnostic records do not let an absent, stale or partial proof stand for
//! coverage of a new topology. The actual head proof has backend controls.
use sophia_engine::SessionLockEpoch;
use sophia_session::session_lock_coverage::{
    SessionLockCoveragePublication, session_lock_coverage_record,
};

#[test]
fn each_topology_waits_for_its_cover_proof_and_reports_once() {
    let epoch = SessionLockEpoch::from_raw(3).unwrap();
    let mut publication = SessionLockCoveragePublication::default();
    assert!(publication.update(Some(epoch), Some(5), None).is_none());
    let record = publication
        .update(Some(epoch), Some(5), Some((epoch, 2, 4)))
        .unwrap();
    assert_eq!(
        session_lock_coverage_record(record),
        "sophia_live_session_lock schema=1 status=covered epoch=3 topology_epoch=5 outputs=2 heads=4"
    );
    assert!(
        publication
            .update(Some(epoch), Some(5), Some((epoch, 2, 4)))
            .is_none()
    );
    assert!(publication.update(Some(epoch), Some(6), None).is_none());
    let record = publication
        .update(Some(epoch), Some(6), Some((epoch, 1, 2)))
        .unwrap();
    assert_eq!(
        (record.topology_epoch, record.outputs, record.heads),
        (6, 1, 2)
    );
    assert!(publication.update(Some(epoch), Some(7), None).is_none());
    let record = publication
        .update(Some(epoch), Some(7), Some((epoch, 2, 4)))
        .unwrap();
    assert_eq!(
        (record.topology_epoch, record.outputs, record.heads),
        (7, 2, 4)
    );
    assert!(
        publication
            .update(Some(epoch), Some(5), Some((epoch, 2, 4)))
            .is_none()
    );
    let next = SessionLockEpoch::from_raw(4).unwrap();
    assert!(
        publication
            .update(Some(next), Some(7), Some((next, 2, 4)))
            .is_some()
    );
}

#[test]
fn stale_epoch_empty_heads_unpublished_topology_and_unlocked_session_never_report() {
    let epoch = SessionLockEpoch::from_raw(3).unwrap();
    let stale = SessionLockEpoch::from_raw(2).unwrap();
    let mut publication = SessionLockCoveragePublication::default();
    for (locked, topology, proof) in [
        (None, Some(1), Some((epoch, 1, 1))),
        (Some(epoch), None, Some((epoch, 1, 1))),
        (Some(epoch), Some(0), Some((epoch, 1, 1))),
        (Some(epoch), Some(1), Some((stale, 1, 1))),
        (Some(epoch), Some(1), Some((epoch, 0, 0))),
        (Some(epoch), Some(1), Some((epoch, 1, 0))),
        (Some(epoch), Some(1), Some((epoch, 2, 1))),
    ] {
        assert!(publication.update(locked, topology, proof).is_none());
    }
    assert!(
        publication
            .update(Some(epoch), Some(1), Some((epoch, 2, 2)))
            .is_some()
    );
}
