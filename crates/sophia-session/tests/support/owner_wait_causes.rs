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

#[test]
fn service_reasons_count_each_source_and_name_waits_without_one() {
    use sophia_backend_live::native_short_service::{
        EXPORTER_WORKER_IN_FLIGHT, FENCE_SIGNALED, HEAD_RENDERING_CONTENT,
    };
    let mut reasons = ServiceReasons::default();
    reasons.add(HEAD_RENDERING_CONTENT | EXPORTER_WORKER_IN_FLIGHT);
    reasons.add(EXPORTER_WORKER_IN_FLIGHT);
    reasons.add(FENCE_SIGNALED);
    reasons.add(0);
    assert_eq!(
        reasons.to_string(),
        " service_reason_fence_signaled=1 service_reason_head_rendering_content=1 service_reason_exporter_worker_in_flight=2 service_reason_none=1"
    );
    let mut full = ServiceReasons([u64::MAX; REASON_NAMES.len() + 1]);
    full.add(FENCE_SIGNALED);
    full.add(0);
    assert_eq!(full.0[6], u64::MAX);
    assert_eq!(full.0[REASON_NAMES.len()], u64::MAX);
}
