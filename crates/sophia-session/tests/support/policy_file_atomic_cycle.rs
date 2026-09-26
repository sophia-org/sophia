//! Actual custody owner, supplied semantic command. No compositor settlement.
#![cfg(test)]
use super::super::journal::reservation_tests::{exhaust_sequence, exhaust_tail, position};
use super::super::typed_codec::TypedFileCodec;
use super::*;
use sophia_protocol::*;

use super::super::startup::tests::array_fixture as fixture;

fn values() -> (WmFileSnapshot, WmFileCycle) {
    let snapshot = WmFileSnapshot {
        transaction: TransactionId::from_raw(10),
        snapshot: PolicyDecodedSnapshot {
            scene: fixture::scene(),
            actions: fixture::actions(),
            classifications: fixture::classifications(),
            launch_origins: fixture::origins(),
        },
    };
    let cycle = WmFileCycle {
        snapshot_transaction: snapshot.transaction,
        request_transaction: TransactionId::from_raw(11),
        request: PolicyProjectionRequest {
            connection_epoch: 2,
            request_id: 12,
            scene_generation: snapshot.snapshot.scene.generation,
            policy_generation: 3,
            affected_outputs: vec![snapshot.snapshot.scene.active_output],
            cause: PolicyRequestCause::SceneChanged,
        },
    };
    (snapshot, cycle)
}
fn owner() -> WmFiles<TypedFileCodec> {
    let limits = WmFileLimits {
        capability_ceiling: u64::MAX,
        profile_required: false,
    };
    let mut owner =
        WmFiles::awaiting_negotiation(2, limits, WmQids::new(), TypedFileCodec).unwrap();
    owner.bind_selected(u64::MAX).unwrap();
    let (snapshot, cycle) = values();
    owner
        .publish_cycle(
            &snapshot,
            &cycle,
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
    owner
}
fn state(owner: &WmFiles<TypedFileCodec>) -> (u64, Vec<u8>, u64, (u64, u64, usize, usize)) {
    let snapshot = owner.snapshot.as_ref().unwrap();
    (
        snapshot.qid,
        snapshot.bytes.clone(),
        *owner.qids.0.lock().unwrap(),
        position(&owner.journal),
    )
}
fn refused(
    owner: &mut WmFiles<TypedFileCodec>,
    snapshot: &WmFileSnapshot,
    cycle: &WmFileCycle,
    expected: Errno,
) {
    let before = state(owner);
    assert_eq!(
        owner.publish_cycle(
            snapshot,
            cycle,
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(1)
        ),
        Err(expected)
    );
    assert_eq!(state(owner), before);
}

#[test]
fn cycle_refusals_and_identity_exhaustion_leave_both_publications_unchanged() {
    let (snapshot, cycle) = values();
    let mut owner = owner();
    let mut wrong = cycle.clone();
    wrong.request.connection_epoch += 1;
    refused(&mut owner, &snapshot, &wrong, Errno::ESTALE);
    wrong = cycle.clone();
    wrong.snapshot_transaction = TransactionId::from_raw(99);
    refused(&mut owner, &snapshot, &wrong, Errno::EINVAL);
    wrong = cycle.clone();
    wrong.request.scene_generation += 1;
    refused(&mut owner, &snapshot, &wrong, Errno::EINVAL);
    wrong = cycle.clone();
    wrong.request_transaction = TransactionId::from_raw(0);
    refused(&mut owner, &snapshot, &wrong, Errno::EINVAL);
    let mut invalid = snapshot.clone();
    invalid.snapshot.scene.outputs.clear();
    refused(&mut owner, &invalid, &cycle, Errno::EINVAL);
    *owner.qids.0.lock().unwrap() = u64::MAX;
    refused(&mut owner, &snapshot, &cycle, Errno::ENOSPC);
    let mut owner = self::owner();
    exhaust_sequence(&mut owner.journal);
    refused(&mut owner, &snapshot, &cycle, Errno::ENOSPC);
    let mut owner = self::owner();
    exhaust_tail(&mut owner.journal);
    refused(&mut owner, &snapshot, &cycle, Errno::ENOSPC);
}

#[test]
fn credit_precedes_snapshot_encoding_and_release_publishes_one_matching_pair() {
    let (mut snapshot, cycle) = values();
    let mut owner = owner();
    while position(&owner.journal).2 < usize::from(WM_FILE_MAX_JOURNAL_RECORDS) {
        owner.append_event(WmFileKind::Negotiated, &[0; 8]).unwrap();
    }
    let valid = snapshot.clone();
    snapshot.snapshot.scene.outputs.clear();
    refused(&mut owner, &snapshot, &cycle, Errno::EAGAIN); // Invalid snapshot was not encoded.
    let old = owner.snapshot.as_ref().unwrap().clone();
    let mut pinned = owner.open(&Node::Snapshot, OpenFlags(0)).unwrap();
    owner
        .journal
        .ack(WmFileAck {
            connection_epoch: 2,
            sequence: 1,
        })
        .unwrap();
    let before = position(&owner.journal);
    let qid = owner
        .publish_cycle(
            &valid,
            &cycle,
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
    assert_eq!(position(&owner.journal).0, before.0 + 1);
    assert_ne!(qid, old.qid);
    assert_eq!(
        owner.describe(&Node::Snapshot, Some(&pinned)).qid_path,
        old.qid
    );
    let ReadOutcome::Ready(bytes) = owner
        .read(&Node::Snapshot, &mut pinned, 0, WM_FILE_MAX_BYTES as u32)
        .unwrap()
    else {
        panic!("old pin")
    };
    assert_eq!(bytes, old.bytes);
    assert_eq!(
        decode_wm_file_snapshot(&owner.snapshot.as_ref().unwrap().bytes, u64::MAX).unwrap(),
        valid
    );
    let ReadOutcome::Ready(event) = owner.journal.read(before.1, 4096).unwrap() else {
        panic!("cycle")
    };
    assert_eq!(decode_wm_file_cycle(&event, u64::MAX).unwrap(), cycle);
}

#[test]
fn byte_credit_stop_and_expiry_spend_no_snapshot_qid_or_event() {
    let (snapshot, cycle) = values();
    let mut owner = owner();
    let remaining = WM_FILE_MAX_BYTES - position(&owner.journal).3;
    owner
        .append_event(
            WmFileKind::Negotiated,
            &vec![0; remaining - WM_FILE_HEADER_BYTES],
        )
        .unwrap();
    refused(&mut owner, &snapshot, &cycle, Errno::EAGAIN);
    let mut owner = self::owner();
    let before = state(&owner);
    assert_eq!(
        owner.publish_cycle(&snapshot, &cycle, &AtomicBool::new(false), Instant::now()),
        Err(Errno(110))
    );
    assert_eq!(state(&owner), before);
    assert_eq!(
        owner.publish_cycle(
            &snapshot,
            &cycle,
            &AtomicBool::new(true),
            Instant::now() + Duration::from_secs(1)
        ),
        Err(Errno(125))
    );
    assert_eq!(state(&owner), before);
    assert!(owner.revoked);
}

#[test]
fn stop_during_encoding_drops_the_reservation_without_an_event() {
    let mut owner = owner();
    let before = state(&owner);
    let stopped = AtomicBool::new(false);
    assert_eq!(
        owner.append_encoded_event_checked(
            WmFileKind::Negotiated,
            |header| {
                let bytes = encode_wm_file_negotiated(header, 0).unwrap();
                stopped.store(true, Ordering::SeqCst);
                Ok(bytes)
            },
            &stopped,
            Instant::now() + Duration::from_secs(1)
        ),
        Err(Errno(125))
    );
    assert_eq!(state(&owner), before);
    assert!(owner.revoked);
}
