#![cfg(test)]

use super::*;

#[test]
fn buckets_partition_waits_and_print_joint_names_in_bit_order() {
    let mut causes = WaitCauses::default();
    causes.add(RING);
    causes.add(RING);
    causes.add(JOURNAL | DEADLINE);
    causes.add(WIRE | RING);
    causes.add(0);
    assert_eq!(causes.count(RING), 2);
    assert_eq!(causes.count(RING | WIRE), 1);
    assert_eq!(causes.count(WIRE), 0);
    assert_eq!(causes.waits(), 5);
    assert_eq!(
        causes.to_string(),
        " cause_none=1 cause_ring=2 cause_ring_wire=1 cause_journal_deadline=1"
    );
}

#[test]
fn every_bit_has_a_name_and_counts_saturate() {
    let mut causes = WaitCauses::default();
    causes.add(RING | JOURNAL | WIRE | FD | NATIVE | DEADLINE | SERVICE);
    assert_eq!(
        causes.to_string(),
        " cause_ring_journal_wire_fd_native_deadline_service=1"
    );
    let mut full = WaitCauses([u64::MAX; BUCKETS]);
    full.add(FD);
    assert_eq!(full.count(FD), u64::MAX);
    assert_eq!(full.waits(), u64::MAX);
}
