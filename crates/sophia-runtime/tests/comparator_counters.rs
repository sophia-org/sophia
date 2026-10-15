#![cfg(feature = "comparator-allocation-counting")]
use sophia_runtime::comparator_counters::{self as counters, Allocator};

#[global_allocator]
static ALLOCATOR: Allocator = counters::ALLOCATOR;

#[test]
fn instrumented_counts_include_real_allocations_with_ordered_clock_bounds() {
    let before = counters::snapshot();
    let bytes = std::hint::black_box(vec![7u8; 4096]);
    let after = counters::snapshot();
    assert!(before.before_usec <= before.after_usec);
    assert!(before.after_usec <= after.before_usec);
    assert!(after.before_usec <= after.after_usec);
    assert!(after.cpu_ns >= before.cpu_ns);
    assert!(after.allocations > before.allocations);
    assert!(after.bytes_allocated >= before.bytes_allocated + bytes.len());
    drop(bytes);
    assert!(counters::snapshot().deallocations > after.deallocations);
}

#[test]
fn sampler_stops_and_joins_without_waiting_for_its_sample_budget() {
    let sampler = counters::Sampler::start().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(8));
    sampler.finish().unwrap();
}
