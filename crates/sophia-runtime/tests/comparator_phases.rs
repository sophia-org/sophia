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
    assert_eq!(rows.lines().count(), 5);
    for phase in ["owner", "wire"] {
        let row = rows
            .lines()
            .find(|r| r.contains(&format!("phase={phase} ")))
            .unwrap();
        assert!(row.contains("completed_scopes=100 "));
        assert!(row.contains("process_allocations_during_scopes=0"));
    }
}
