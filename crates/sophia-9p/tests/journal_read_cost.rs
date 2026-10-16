//! Separate completed-read payload allocation from encoding into retained storage.
//! Guard against buffer growth and allocations on warmed encoding and pending reads.
use sophia_9p::journal::{Journal, JournalBounds};
use sophia_9p::{ReadOutcome, Reply, Tag, wire};

#[global_allocator]
static ALLOCATOR: &stats_alloc::StatsAlloc<std::alloc::System> = &stats_alloc::INSTRUMENTED_SYSTEM;

#[test]
fn completed_journal_read_cost_preserves_bytes_and_retention() {
    for (records, offset, count) in [
        (1, 0, 256),
        (4, 0, 1024),
        (4, 17, 700),
        (1, 253, u32::MAX),
        (4, 17, u32::MAX),
    ] {
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
        let end = offset as usize + (count as usize).min(expected.len() - offset as usize);
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
            assert_eq!(bytes, expected[offset as usize..end]);
            // A large request cannot reserve more than the readable range.
            assert_eq!(after.bytes_allocated - before.bytes_allocated, bytes.len());
            output.clear();
            let reply = Reply::Read(bytes);
            let before = ALLOCATOR.stats();
            wire::encode(Tag(1), &reply, &mut output).unwrap();
            let after = ALLOCATOR.stats();
            encode_allocs +=
                after.allocations - before.allocations + after.reallocations - before.reallocations;
            assert_eq!(&output[11..], &expected[offset as usize..end]);
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
        assert_eq!(read_reallocs, 0);
        assert_eq!(encode_allocs, 0);
        assert_eq!(pending_allocs, 0);
        // Logical payload traffic from the two source-level copy stages;
        // this does not count allocator moves, kernel copies or CPU time.
        let payload_bytes = (end - offset as usize) * 1000;
        eprintln!(
            "journal_read_cost records={records} offset={offset} count={count} reads=1000 read_allocations={read_allocs} read_reallocations={read_reallocs} encode_allocation_operations={encode_allocs} pending_allocation_operations={pending_allocs} gathered_payload_bytes={payload_bytes} encoded_payload_bytes={payload_bytes}"
        );
        journal.ack(7, records as u64).unwrap();
        assert_eq!(journal.position().records, 0);
    }
}
