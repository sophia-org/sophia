use sophia_backend_live::{
    LibdrmNativeOutputCapability, LibdrmNativeOutputTiming, LibdrmNativeVrrPropertyDiscoveryStatus,
    project_live_output_authority_snapshot,
};
use sophia_engine::{HeadlessOutput, RenderHeadId};
use sophia_protocol::{OutputId, Size};
use sophia_session::live_output_authority::LiveOutputAuthorityOwner;

#[global_allocator]
static ALLOCATOR: &stats_alloc::StatsAlloc<std::alloc::System> = &stats_alloc::INSTRUMENTED_SYSTEM;

#[test]
fn cadence_reads_current_primary_without_copying_output_inventory() {
    let timing = LibdrmNativeOutputTiming::new(1920, 1080, 60_000);
    let capabilities: Vec<_> = (1..=2)
        .map(|id| {
            LibdrmNativeOutputCapability::new(
                OutputId::from_raw(id),
                id as u32,
                format!("DP-{id}"),
                [timing, LibdrmNativeOutputTiming::new(1280, 720, 60_000)],
                Some(timing),
                timing,
                LibdrmNativeVrrPropertyDiscoveryStatus::Discovered,
            )
            .unwrap()
            .bind_head(RenderHeadId::from_raw(id))
            .unwrap()
        })
        .collect();
    let outputs: Vec<_> = (1..=2)
        .map(|id| HeadlessOutput {
            id: OutputId::from_raw(id),
            size: Size {
                width: 1920,
                height: 1080,
            },
            scale: 1,
        })
        .collect();
    let snapshot = project_live_output_authority_snapshot(&capabilities, &outputs, 7).unwrap();
    let mut owner = LiveOutputAuthorityOwner::new(3, snapshot).unwrap();
    let before = ALLOCATOR.stats();
    for _ in 0..1000 {
        std::hint::black_box(owner.published().clone());
    }
    let owned = ALLOCATOR.stats() - before;
    let before = ALLOCATOR.stats();
    for _ in 0..1000 {
        assert_eq!(
            std::hint::black_box(owner.published()).primary_output,
            OutputId::from_raw(1)
        );
    }
    let borrowed = ALLOCATOR.stats() - before;
    assert!(owned.allocations > 0);
    assert_eq!(borrowed.allocations, 0);
    assert_eq!(borrowed.reallocations, 0);
    eprintln!(
        "1000 topology clones: {} allocations; primary reads: {}",
        owned.allocations, borrowed.allocations
    );

    let mut replacement = owner.published().clone();
    replacement.topology_epoch += 1;
    replacement.primary_output = OutputId::from_raw(2);
    owner.replace_published_snapshot(replacement).unwrap();
    assert_eq!(owner.published().primary_output, OutputId::from_raw(2));
}
