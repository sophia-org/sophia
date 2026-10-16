#![cfg(feature = "comparator-profiling")]
use sophia_runtime::comparator_counters::{
    self as counters,
    phases::{self, Phase},
};
#[global_allocator]
static ALLOCATOR: counters::Allocator = counters::ALLOCATOR;

#[test]
fn bounded_phase_records_allocate_nothing_and_count_completed_scopes() {
    let mut bytes = [0u8; 4096];
    let before = counters::snapshot();
    for sequence in 0..100 {
        let owner = phases::enter(Phase::Owner);
        {
            let _wire = phases::enter(Phase::Wire);
            let _preparation = phases::enter(Phase::Preparation);
            let _after = phases::enter(Phase::AfterComponents);
            let _lifecycle = phases::enter(Phase::Lifecycle);
            let _wm = phases::enter(Phase::Wm);
            let _authority = phases::enter(Phase::Authority);
        }
        drop(owner);
        phases::record(&mut std::io::Cursor::new(&mut bytes[..]), sequence).unwrap();
    }
    let after = counters::snapshot();
    assert_eq!(after.allocations, before.allocations);
    assert_eq!(after.reallocations, before.reallocations);
    let mut output = std::io::Cursor::new(&mut bytes[..]);
    phases::record(&mut output, 100).unwrap();
    let len = output.position() as usize;
    let rows = std::str::from_utf8(&bytes[..len]).unwrap();
    assert_eq!(rows.lines().count(), 10);
    for phase in [
        "owner",
        "wire",
        "preparation",
        "after_components",
        "lifecycle",
        "wm",
        "authority",
    ] {
        let row = rows
            .lines()
            .find(|r| r.contains(&format!("phase={phase} ")))
            .unwrap();
        assert!(row.contains("completed_scopes=100 "));
        assert!(row.contains("process_allocations_during_scopes=0"));
    }

    // Even a run shorter than the phase publication interval retains both
    // endpoints. The allocating sink is outside the overhead measurement.
    #[derive(Clone)]
    struct Sink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let sink = Sink(Default::default());
    drop(counters::Sampler::start_with_output(sink.clone()).unwrap());
    let captured = sink.0.lock().unwrap();
    let captured = std::str::from_utf8(&captured).unwrap();
    let end = captured.lines().last().unwrap();
    assert!(end.contains("stage=end "));
    let sequence = end
        .split_whitespace()
        .find(|s| s.starts_with("sequence="))
        .unwrap();
    for sequence in ["sequence=0", sequence] {
        assert_eq!(
            captured
                .lines()
                .filter(|line| {
                    line.starts_with("sophia_comparator_profile ")
                        && line.split_whitespace().any(|field| field == sequence)
                })
                .count(),
            10
        );
    }
}
