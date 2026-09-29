use super::*;
use crate::custody::{Admission, Ledger, Ticket};

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
