#![cfg(feature = "comparator-progress")]
mod support;
use sophia_9p::{Epoch, progress::service_ledger as ledger};
use support::*;
#[global_allocator]
static ALLOCATOR: &stats_alloc::StatsAlloc<std::alloc::System> = &stats_alloc::INSTRUMENTED_SYSTEM;

#[test]
fn retry_ledger_separates_pending_ready_and_revoked_reads_and_restores_sources() {
    let mut h = Harness::attached();
    h.open(1, &[b"events"], 0);
    assert!(h.send(&tread(9, 1, 0, 64)).unwrap().is_empty());
    let before = ledger::snapshot();
    {
        let _wake = ledger::retry_source(0);
        h.connection.retry_waiting(&mut h.export).unwrap();
        {
            let _write = ledger::retry_source(1);
            h.export.push_event(b"ready");
            h.connection.retry_waiting(&mut h.export).unwrap();
            assert_eq!(h.take()[0].data(), b"ready");
        }
        assert!(h.send(&tread(10, 1, 0, 64)).unwrap().is_empty());
        h.export.revoke(Epoch(1));
        h.connection.retry_waiting(&mut h.export).unwrap();
        assert_eq!(h.take()[0].errno(), Some(ESTALE));
    }
    let after = ledger::snapshot();
    for (name, expected) in [
        ("retry_wake_pending", 1),
        ("retry_write_ready", 1),
        ("retry_wake_error", 1),
    ] {
        let i = ledger::NAMES.iter().position(|n| *n == name).unwrap();
        assert_eq!(after[i] - before[i], expected, "{name}");
    }
    let before = ledger::snapshot();
    let allocations = ALLOCATOR.stats();
    ledger::input();
    {
        let _owner = ledger::owner_input();
        ledger::input();
    }
    ledger::input();
    let bytes = ledger::written_bytes();
    ledger::flush(bytes, false);
    ledger::written(9);
    ledger::flush(bytes, false);
    ledger::flush(bytes, true);
    let completed = ALLOCATOR.stats();
    assert_eq!(completed.allocations, allocations.allocations);
    assert_eq!(completed.reallocations, allocations.reallocations);
    let after = ledger::snapshot();
    assert_eq!(
        &after[..5]
            .iter()
            .zip(&before[..5])
            .map(|(a, b)| a - b)
            .collect::<Vec<_>>(),
        &[1, 2, 1, 1, 1]
    );
}
