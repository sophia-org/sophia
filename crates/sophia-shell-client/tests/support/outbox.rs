use super::*;

#[test]
fn bulk_cannot_spend_action_pair_capacity_and_pairs_enqueue_atomically() {
    let mut outbox = ClientOutbox::default();
    for _ in 0..32 {
        outbox.enqueue(vec![vec![1; 100]], false).unwrap();
    }
    assert_eq!(
        outbox.enqueue(vec![vec![2]], false),
        Err(ShellClientError::QueueSaturated)
    );
    for _ in 0..15 {
        outbox
            .enqueue(vec![vec![3; 100], vec![4; 100]], true)
            .unwrap();
    }
    outbox.enqueue(vec![vec![5]], true).unwrap();
    let records = outbox.frames.len();
    assert_eq!(
        outbox.enqueue(vec![vec![6], vec![7]], true),
        Err(ShellClientError::QueueSaturated)
    );
    assert_eq!(outbox.frames.len(), records);
    assert_eq!(outbox.front().unwrap(), [1; 100]);
    outbox.written(99);
    assert_eq!(outbox.frames.len(), records);
    outbox.written(1);
    outbox.enqueue(vec![vec![6], vec![7]], true).unwrap();
}
