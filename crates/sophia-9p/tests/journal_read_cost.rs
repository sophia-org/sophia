//! Separate completed-read payload allocation from encoding into retained storage.
//! Counts are diagnostics, not a contract that today's overhead must remain.
use sophia_9p::journal::{Journal, JournalBounds};
use sophia_9p::{ReadOutcome, Reply, Tag, wire};

#[global_allocator]
static ALLOCATOR: &stats_alloc::StatsAlloc<std::alloc::System> = &stats_alloc::INSTRUMENTED_SYSTEM;

#[test]
fn completed_journal_read_cost_preserves_bytes_and_retention() {
    for (records, offset, count) in [(1, 0, 256), (4, 0, 1024), (4, 17, 700)] {
        let mut journal = Journal::new(7);
        let mut expected = Vec::new();
        for index in 0..records {
            let mut record = vec![index as u8; 256];
            record[..4].copy_from_slice(&256u32.to_le_bytes());
            expected.extend_from_slice(&record);
            journal
                .prepare(
                    record,
                    JournalBounds {
                        records: 4,
                        bytes: 1024,
                    },
                )
                .unwrap()
                .commit();
        }
        let retained = journal.position();
        let mut output = Vec::with_capacity(2048);
        let (mut read_allocs, mut read_reallocs, mut encode_allocs) = (0, 0, 0);
        for _ in 0..1000 {
            let before = ALLOCATOR.stats();
            let ReadOutcome::Ready(bytes) = journal.read(offset, count).unwrap() else {
                panic!("published bytes must be readable");
            };
            let after = ALLOCATOR.stats();
            read_allocs += after.allocations - before.allocations;
            read_reallocs += after.reallocations - before.reallocations;
            assert_eq!(
                bytes,
                expected[offset as usize..offset as usize + count as usize]
            );
            output.clear();
            let reply = Reply::Read(bytes);
            let before = ALLOCATOR.stats();
            wire::encode(Tag(1), &reply, &mut output).unwrap();
            let after = ALLOCATOR.stats();
            encode_allocs +=
                after.allocations - before.allocations + after.reallocations - before.reallocations;
            assert_eq!(
                &output[11..],
                &expected[offset as usize..offset as usize + count as usize]
            );
            assert_eq!(journal.position(), retained);
        }
        let before = ALLOCATOR.stats();
        for _ in 0..1000 {
            assert_eq!(
                journal.read(journal.size(), count),
                Ok(ReadOutcome::Pending)
            );
        }
        let after = ALLOCATOR.stats();
        let pending_allocs =
            after.allocations - before.allocations + after.reallocations - before.reallocations;
        eprintln!(
            "journal_read_cost records={records} offset={offset} count={count} reads=1000 read_allocations={read_allocs} read_reallocations={read_reallocs} encode_allocation_operations={encode_allocs} pending_allocation_operations={pending_allocs}"
        );
        journal.ack(7, records as u64).unwrap();
        assert_eq!(journal.position().records, 0);
    }
}
