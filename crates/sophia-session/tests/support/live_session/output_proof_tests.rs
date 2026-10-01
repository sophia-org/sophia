use super::super::output_proof::{
    OutputPeerLossObservation, OutputPeerLossProof, validate_output_peer_loss_proof,
};
use sophia_protocol::TransactionId;
use std::time::{Duration, Instant};

#[test]
fn output_peer_loss_proof_requires_an_explicit_bounded_native_role() {
    let runtime = Some(Duration::from_secs(30));
    for (native, normal, peer, bound, armed, other) in [
        (false, true, true, runtime, true, false),
        (true, false, true, runtime, true, false),
        (true, true, false, runtime, true, false),
        (true, true, true, None, true, false),
        (true, true, true, Some(Duration::ZERO), true, false),
        (true, true, true, runtime, false, false),
        (true, true, true, runtime, true, true),
    ] {
        assert!(
            validate_output_peer_loss_proof(true, native, normal, peer, bound, armed, other)
                .is_err()
        );
    }
    assert!(validate_output_peer_loss_proof(true, true, true, true, runtime, true, false).is_ok());
    assert!(validate_output_peer_loss_proof(false, false, false, false, None, false, true).is_ok());
}

#[test]
fn output_peer_loss_hold_never_expires_into_commit() {
    let now = Instant::now();
    let transaction = TransactionId::from_raw(11);
    let mut proof = OutputPeerLossProof::new(true);
    assert!(proof.arm(3, transaction, now));
    assert!(!proof.holds(TransactionId::from_raw(12)));
    assert!(!proof.expire(transaction, now + Duration::from_millis(4999)));
    assert!(proof.expire(transaction, now + Duration::from_secs(5)));
    assert!(proof.holds(transaction));
    assert!(!proof.expire(transaction, now + Duration::from_secs(6)));
    // Late departure does not turn the expired proof into a pass.
    proof.observe(Some(departure(transaction)));
    proof.restored(transaction);
    assert!(proof.finish().is_err());
    assert!(proof.holds(transaction));
    assert!(!proof.arm(4, transaction, now));
}

#[test]
fn output_peer_loss_hold_is_one_shot_and_clears_only_after_restoration() {
    let now = Instant::now();
    let transaction = TransactionId::from_raw(11);
    let other = TransactionId::from_raw(12);
    let mut disabled = OutputPeerLossProof::new(false);
    assert!(!disabled.arm(3, transaction, now));
    let mut proof = OutputPeerLossProof::new(true);
    assert!(proof.arm(3, transaction, now));
    proof.observe(Some(departure(other)));
    assert!(proof.holds(transaction));
    proof.restored(other);
    assert_eq!(proof.finish().unwrap(), None);
    proof.observe(Some(departure(transaction)));
    assert!(!proof.expire(transaction, now + Duration::from_secs(30)));
    assert!(proof.holds(transaction));
    proof.restored(transaction);
    assert_eq!(proof.finish().unwrap(), Some((3, transaction)));
    assert!(!proof.holds(transaction));
    assert!(!proof.arm(4, transaction, now));
}

fn departure(transaction: TransactionId) -> OutputPeerLossObservation {
    OutputPeerLossObservation {
        connection_epoch: 3,
        transaction,
        peer: 123,
        disconnected: true,
        terminated: true,
        failed: false,
    }
}

#[test]
fn output_peer_loss_requires_both_typed_events_in_either_order() {
    let transaction = TransactionId::from_raw(11);
    for disconnect_first in [false, true] {
        let mut proof = OutputPeerLossProof::new(true);
        proof.arm(3, transaction, Instant::now());
        let mut observed = departure(transaction);
        observed.disconnected = disconnect_first;
        observed.terminated = !disconnect_first;
        proof.observe(Some(observed));
        proof.restored(transaction);
        assert_eq!(
            proof.finish().unwrap(),
            None,
            "restoration alone does not prove both events"
        );
        proof.observe(Some(departure(transaction)));
        assert_eq!(proof.finish().unwrap(), Some((3, transaction)));
    }
}

#[test]
fn output_peer_loss_refuses_failure_and_wrong_epoch_even_after_restoration() {
    let transaction = TransactionId::from_raw(11);
    let now = Instant::now();
    let mut proof = OutputPeerLossProof::new(true);
    proof.arm(3, transaction, now);
    let mut wrong = departure(transaction);
    wrong.connection_epoch = 4;
    proof.observe(Some(wrong));
    proof.restored(transaction);
    assert_eq!(proof.finish().unwrap(), None);
    let mut failed = departure(transaction);
    failed.failed = true;
    proof.observe(Some(failed));
    assert!(proof.rollback_required(transaction));
    assert!(
        proof.finish().is_err(),
        "service failure is not successful departure"
    );
    let mut late = OutputPeerLossProof::new(true);
    late.arm(3, transaction, now);
    late.restored(transaction);
    assert!(late.poll(now + Duration::from_secs(5), Some(departure(transaction))));
    assert!(late.finish().is_err());
}

#[test]
fn output_peer_loss_outer_deadline_cannot_qualify_an_incomplete_proof() {
    let transaction = TransactionId::from_raw(11);
    let mut pending = OutputPeerLossProof::new(true);
    assert!(pending.interrupt());
    assert!(!pending.interrupt());
    assert!(!pending.arm(3, transaction, Instant::now()));

    let mut active = OutputPeerLossProof::new(true);
    active.arm(3, transaction, Instant::now());
    active.observe(Some(departure(transaction)));
    assert!(active.interrupt());
    assert!(active.holds(transaction));
    assert!(active.rollback_required(transaction));
    active.restored(transaction);
    assert!(active.finish().is_err());

    let mut completed = OutputPeerLossProof::new(true);
    completed.arm(3, transaction, Instant::now());
    completed.observe(Some(departure(transaction)));
    completed.restored(transaction);
    assert_eq!(completed.finish().unwrap(), Some((3, transaction)));
    assert!(!completed.interrupt());
    assert!(!OutputPeerLossProof::new(false).interrupt());
}
