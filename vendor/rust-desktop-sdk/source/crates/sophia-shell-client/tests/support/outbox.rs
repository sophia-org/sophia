use super::*;
use crate::custody::{Admission, Ledger, Ticket};

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
