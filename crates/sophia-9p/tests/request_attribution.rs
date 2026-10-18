#![cfg(feature = "comparator-request-attribution")]
use sophia_9p::request_attribution::{Site, enter, snapshot};

#[global_allocator]
static ALLOCATOR: &stats_alloc::StatsAlloc<std::alloc::System> = &stats_alloc::INSTRUMENTED_SYSTEM;

#[test]
fn scopes_count_real_allocation_and_reallocation_without_allocating_themselves() {
    let before = snapshot();
    let idle = enter(Site::Other);
    drop(idle);
    let after = snapshot();
    assert_eq!(
        after[Site::Other as usize].calls - before[Site::Other as usize].calls,
        1
    );
    assert_eq!(
        after[Site::Other as usize].process_allocations
            - before[Site::Other as usize].process_allocations,
        0
    );

    let outer = enter(Site::Walk);
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(b"12345678");
    bytes.reserve_exact(1024);
    let nested = enter(Site::EncodeReply);
    let other = vec![19u8; 1024];
    std::hint::black_box(&other);
    drop(nested);
    std::hint::black_box(&bytes);
    drop(outer);
    let end = snapshot();
    let walk = &end[Site::Walk as usize];
    let encode = &end[Site::EncodeReply as usize];
    assert_eq!(walk.process_allocations, 2);
    assert_eq!(walk.process_reallocations, 1);
    assert_eq!(encode.process_allocations, 1);
    assert!(walk.process_bytes >= 2048);
    assert!(walk.thread_cpu_ns >= encode.thread_cpu_ns);
    // The nested allocation is already in Walk, so adding these totals lies.
    assert_eq!(walk.calls, 1);
    assert_eq!(encode.calls, 1);
}
