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
    assert!(
        publication
            .update(Some(epoch), Some(5), Some(1), None)
            .is_none()
    );
    let record = publication
        .update(Some(epoch), Some(5), Some(1), Some((epoch, 2, 4)))
        .unwrap();
    assert_eq!(
        session_lock_coverage_record(record),
        "sophia_live_session_lock schema=1 status=covered epoch=3 topology_epoch=5 owner=1 outputs=2 heads=4"
    );
    assert!(
        publication
            .update(Some(epoch), Some(5), Some(1), Some((epoch, 2, 4)))
            .is_none()
    );
    assert!(
        publication
            .update(Some(epoch), Some(6), Some(1), None)
            .is_none()
    );
    let record = publication
        .update(Some(epoch), Some(6), Some(1), Some((epoch, 1, 2)))
        .unwrap();
    assert_eq!(
        (record.topology_epoch, record.outputs, record.heads),
        (6, 1, 2)
    );
    assert!(
        publication
            .update(Some(epoch), Some(7), Some(1), None)
            .is_none()
    );
    let record = publication
        .update(Some(epoch), Some(7), Some(1), Some((epoch, 2, 4)))
        .unwrap();
    assert_eq!(
        (record.topology_epoch, record.outputs, record.heads),
        (7, 2, 4)
    );
    assert!(
        publication
            .update(Some(epoch), Some(5), Some(1), Some((epoch, 2, 4)))
            .is_none()
    );
    let next = SessionLockEpoch::from_raw(4).unwrap();
    assert!(
        publication
            .update(Some(next), Some(7), Some(1), Some((next, 2, 4)))
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
        assert!(
            publication
                .update(locked, topology, Some(1), proof)
                .is_none()
        );
    }
    assert!(
        publication
            .update(Some(epoch), Some(1), Some(1), Some((epoch, 2, 2)))
            .is_some()
    );
}

#[test]
fn a_replacement_owner_on_an_unchanged_topology_reports_its_own_cover() {
    // A same-port return keeps the topology epoch, so only the owner shows
    // that new heads, not the retired ones, retired this cover.
    let epoch = SessionLockEpoch::from_raw(1).unwrap();
    let mut publication = SessionLockCoveragePublication::default();
    let first = publication
        .update(Some(epoch), Some(3), Some(3), Some((epoch, 1, 1)))
        .unwrap();
    assert_eq!(first.owner, 3);
    assert!(
        publication
            .update(Some(epoch), Some(3), Some(3), Some((epoch, 1, 1)))
            .is_none()
    );
    // The returned owner has not yet retired the cover on its heads.
    assert!(
        publication
            .update(Some(epoch), Some(3), Some(4), None)
            .is_none()
    );
    let returned = publication
        .update(Some(epoch), Some(3), Some(4), Some((epoch, 1, 1)))
        .unwrap();
    assert_eq!(
        session_lock_coverage_record(returned),
        "sophia_live_session_lock schema=1 status=covered epoch=1 topology_epoch=3 owner=4 outputs=1 heads=1"
    );
    // An older topology never reports, whatever the owner.
    assert!(
        publication
            .update(Some(epoch), Some(2), Some(5), Some((epoch, 1, 1)))
            .is_none()
    );
    // Without an owner there is no head proof to attribute.
    let mut fresh = SessionLockCoveragePublication::default();
    for owner in [None, Some(0)] {
        assert!(
            fresh
                .update(Some(epoch), Some(3), owner, Some((epoch, 1, 1)))
                .is_none()
        );
    }
}
