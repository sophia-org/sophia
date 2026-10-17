use super::*;
use crate::custody::{Admission, Ledger, Ticket};

#[test]
fn configured_headroom_and_inflight_charges_are_preserved() {
    let mut outbox = ClientOutbox::default();
    let mut ledger = Ledger::default();
    let limits = QueueLimits {
        records: 4,
        bytes: 40,
        control_records: 1,
        control_bytes: 10,
    };
    outbox.set_limits(limits).unwrap();
    for _ in 0..3 {
        outbox
            .enqueue(vec![vec![1; 10]], false, &mut ledger)
            .unwrap();
    }
    assert!(outbox.enqueue(vec![vec![1]], false, &mut ledger).is_err());
    outbox
        .enqueue(vec![vec![2; 10]], true, &mut ledger)
        .unwrap();
    assert!(outbox.enqueue(vec![vec![2]], true, &mut ledger).is_err());
    let bytes = outbox.take_front_bytes();
    assert_eq!(bytes.len(), 10);
    assert!(outbox.set_limits(QueueLimits::default()).is_err());
    assert_eq!(outbox.limits(), limits);
    for _ in 0..4 {
        outbox.retire_front();
    }
    let token = outbox.reserve_capacity(3, 30).unwrap();
    assert!(outbox.set_limits(QueueLimits::default()).is_err());
    outbox.cancel_reservation(&token).unwrap();
    outbox.set_limits(QueueLimits::default()).unwrap();
}

#[test]
fn invalid_limits_leave_configuration_unchanged() {
    let mut outbox = ClientOutbox::default();
    for bad in [
        QueueLimits {
            records: 0,
            ..QueueLimits::default()
        },
        QueueLimits {
            records: 65,
            ..QueueLimits::default()
        },
        QueueLimits {
            bytes: 0,
            ..QueueLimits::default()
        },
        QueueLimits {
            bytes: MAX_QUEUED_BYTES + 1,
            ..QueueLimits::default()
        },
        QueueLimits {
            control_records: 65,
            ..QueueLimits::default()
        },
        QueueLimits {
            control_bytes: MAX_QUEUED_BYTES + 1,
            ..QueueLimits::default()
        },
    ] {
        assert!(outbox.set_limits(bad).is_err());
        assert_eq!(outbox.limits(), QueueLimits::default());
    }
    outbox
        .set_limits(QueueLimits {
            records: 1,
            bytes: 1,
            control_records: 0,
            control_bytes: 0,
        })
        .unwrap();
    let mut ledger = Ledger::default();
    outbox.enqueue(vec![vec![1]], false, &mut ledger).unwrap();
    assert!(outbox.enqueue(vec![vec![1]], true, &mut ledger).is_err());
}

#[test]
fn reserved_bytes_and_slots_preserve_bulk_and_control_limits() {
    let mut outbox = ClientOutbox::default();
    let mut ledger = Ledger::default();
    let token = outbox
        .reserve_capacity(1, MAX_QUEUED_BYTES - CONTROL_BYTES)
        .unwrap();
    assert!(matches!(
        outbox.prepare(vec![vec![1]], false, &mut ledger),
        Err(ShellClientError::QueueSaturated)
    ));
    outbox
        .enqueue(vec![vec![2; CONTROL_BYTES]], true, &mut ledger)
        .unwrap();
    assert!(matches!(
        outbox.prepare(vec![vec![3]], true, &mut ledger),
        Err(ShellClientError::QueueSaturated)
    ));
    let prepared = outbox
        .prepare_reserved(&token, vec![vec![4]], false, &mut ledger)
        .unwrap();
    let admission = outbox.commit(prepared, &mut ledger);
    assert_eq!(admission.first, Ticket(2));
    // The unused byte allowance was released on commit.
    outbox.enqueue(vec![vec![5]], false, &mut ledger).unwrap();
    assert_eq!(outbox.bytes, CONTROL_BYTES + 2);
}

#[test]
fn preparation_acquires_capacity_without_queueing_or_spending_tickets() {
    let mut outbox = ClientOutbox::default();
    let mut ledger = Ledger::default();
    let prepared = outbox
        .prepare(vec![vec![1; 10], vec![2; 20]], true, &mut ledger)
        .unwrap();
    let capacity = outbox.frames.capacity();
    let payload = prepared.frames[0].bytes.as_ref().unwrap().as_ptr();
    assert!(capacity >= 2);
    assert!(outbox.front().is_none());
    assert_eq!(ledger.peek(), 1);
    let admission = outbox.commit(prepared, &mut ledger);
    assert_eq!(
        admission,
        Admission {
            first: Ticket(1),
            count: 2
        }
    );
    assert_eq!(outbox.frames.capacity(), capacity);
    assert_eq!(outbox.front().unwrap().as_ptr(), payload);
    assert_eq!(outbox.bytes, 30);
    assert_eq!(ledger.get(Ticket(2)), Some(crate::Custody::Queued));
}

#[test]
fn moving_payload_keeps_its_ticket_and_all_admission_charges_until_retirement() {
    let mut outbox = ClientOutbox::default();
    let mut ledger = Ledger::default();
    for _ in 0..32 {
        outbox
            .enqueue(vec![vec![0x5a; 100]], false, &mut ledger)
            .unwrap();
    }
    let address = outbox.front().unwrap().as_ptr();
    let charged = (outbox.bytes, outbox.bulk_bytes, outbox.bulk_records);
    let bytes = outbox.take_front_bytes();
    assert_eq!(
        bytes.as_ptr(),
        address,
        "moving to the wire does not copy the payload"
    );
    assert_eq!(outbox.front_ticket(), Some(1));
    assert_eq!(outbox.tickets().next(), Some(1));
    assert_eq!(
        (outbox.bytes, outbox.bulk_bytes, outbox.bulk_records),
        charged
    );
    assert_eq!(
        outbox.enqueue(vec![vec![1]], false, &mut ledger),
        Err(ShellClientError::QueueSaturated)
    );
    // Control admission still has its reserved share while bulk is held.
    outbox
        .enqueue(vec![vec![2; 10]], true, &mut ledger)
        .unwrap();
    outbox.retire_front();
    assert_eq!(outbox.front_ticket(), Some(2));
    assert_eq!(outbox.bytes, charged.0 + 10 - 100);
    assert_eq!(outbox.bulk_bytes, charged.1 - 100);
    assert_eq!(outbox.bulk_records, charged.2 - 1);
    outbox
        .enqueue(vec![vec![3; 100]], false, &mut ledger)
        .unwrap();
}

#[test]
fn bulk_cannot_spend_action_pair_capacity_and_pairs_enqueue_atomically() {
    let mut outbox = ClientOutbox::default();
    let mut ledger = Ledger::default();
    for _ in 0..32 {
        outbox
            .enqueue(vec![vec![1; 100]], false, &mut ledger)
            .unwrap();
    }
    assert_eq!(
        outbox.enqueue(vec![vec![2]], false, &mut ledger),
        Err(ShellClientError::QueueSaturated)
    );
    // A refused admission spends no ticket.
    assert_eq!(ledger.peek(), 33);
    for _ in 0..15 {
        outbox
            .enqueue(vec![vec![3; 100], vec![4; 100]], true, &mut ledger)
            .unwrap();
    }
    outbox.enqueue(vec![vec![5]], true, &mut ledger).unwrap();
    let records = outbox.frames.len();
    assert_eq!(
        outbox.enqueue(vec![vec![6], vec![7]], true, &mut ledger),
        Err(ShellClientError::QueueSaturated)
    );
    assert_eq!(outbox.frames.len(), records);
    assert_eq!(outbox.front().unwrap(), [1; 100]);
    assert_eq!(outbox.front_ticket(), Some(1));
    assert_eq!(outbox.frames.len(), records);
    outbox.retire_front();
    assert_eq!(outbox.frames.len(), records - 1);
    let pair = outbox
        .enqueue(vec![vec![6], vec![7]], true, &mut ledger)
        .unwrap();
    assert_eq!(pair.count, 2);
    assert_eq!(
        pair,
        Admission {
            first: Ticket(ledger.peek() - 2),
            count: 2
        }
    );
}
