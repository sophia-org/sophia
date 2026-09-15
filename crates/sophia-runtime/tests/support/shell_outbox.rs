use super::*;

#[test]
fn partial_write_preserves_record_custody_and_fifo_order() {
    let mut outbox = ShellOutbox::default();
    outbox.push(vec![1, 2, 3, 4], true);
    outbox.push(vec![5, 6], true);
    assert_eq!(outbox.len(), 6);
    outbox.written(2);
    assert_eq!(outbox.front(), [3, 4]);
    assert_eq!(outbox.len(), 6, "the original allocation is still retained");
    assert_eq!(outbox.controls(), 2);
    outbox.written(2);
    assert_eq!(outbox.front(), [5, 6]);
    assert_eq!(outbox.len(), 2);
    outbox.written(1);
    assert_eq!(outbox.front(), [6]);
    assert_eq!(outbox.len(), 2);
    outbox.written(1);
    assert!(outbox.is_empty());
    assert_eq!(outbox.len(), 0);
}

#[test]
fn disconnect_clears_the_exact_partially_written_inventory() {
    let mut outbox = ShellOutbox::default();
    outbox.push(vec![1, 2], true);
    outbox.written(1);
    outbox.clear();
    assert!(outbox.is_empty());
    assert_eq!(outbox.len(), 0);
    outbox.push(vec![3], true);
    assert_eq!(outbox.front(), [3]);
}
