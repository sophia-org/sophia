//! Record-file custody: the acknowledged journal's reservation, reads and
//! release, and staging's append, exact repeat, length and deadline rules.

use std::time::{Duration, Instant};

use sophia_9p::journal::{Journal, JournalBounds, JournalPosition, Staging, StagingBounds};
use sophia_9p::{Errno, ReadOutcome};

const WIDE: JournalBounds = JournalBounds {
    records: 4,
    bytes: 64,
};

/// A length-prefixed record of `size` bytes filled with `fill`.
fn record(size: usize, fill: u8) -> Vec<u8> {
    let mut bytes = vec![fill; size];
    bytes[..4].copy_from_slice(&(size as u32).to_le_bytes());
    bytes
}

fn ready(outcome: ReadOutcome) -> Vec<u8> {
    match outcome {
        ReadOutcome::Ready(bytes) => bytes,
        ReadOutcome::Pending => panic!("expected bytes"),
    }
}

#[test]
fn a_dropped_reservation_spends_nothing_and_a_commit_takes_the_next_sequence() {
    let mut journal = Journal::new(7);
    let before = journal.position();
    drop(journal.prepare(record(8, 1), WIDE).unwrap());
    assert_eq!(journal.position(), before);
    assert_eq!(journal.next_sequence(), 1);
    assert_eq!(journal.prepare(record(8, 1), WIDE).unwrap().commit(), 1);
    assert_eq!(journal.prepare(record(12, 2), WIDE).unwrap().commit(), 2);
    assert_eq!(
        journal.position(),
        JournalPosition {
            next_sequence: 3,
            tail: 20,
            records: 2,
            bytes: 20,
        }
    );
    assert_eq!(journal.size(), 20);
}

#[test]
fn bounds_refuse_with_eagain_and_exhausted_identities_with_enospc() {
    let mut journal = Journal::new(7);
    let tight = JournalBounds {
        records: 1,
        bytes: 16,
    };
    assert_eq!(
        journal.prepare(record(20, 1), tight).err(),
        Some(Errno::EAGAIN)
    );
    journal.prepare(record(8, 1), tight).unwrap().commit();
    assert!(!journal.fits(4, tight));
    assert_eq!(
        journal.prepare(record(4, 1), tight).err(),
        Some(Errno::EAGAIN)
    );
    assert!(journal.fits(8, WIDE));

    let before = Journal::starting_at(7, u64::MAX, 0);
    let mut sequence = Journal::starting_at(7, u64::MAX, 0);
    assert_eq!(
        sequence.prepare(record(8, 1), WIDE).err(),
        Some(Errno::ENOSPC)
    );
    assert_eq!(sequence.position(), before.position());
    let mut offset = Journal::starting_at(7, 1, u64::MAX);
    assert_eq!(
        offset.prepare(record(8, 1), WIDE).err(),
        Some(Errno::ENOSPC)
    );
}

#[test]
fn reads_span_records_wait_at_the_end_and_refuse_released_offsets() {
    let mut journal = Journal::new(7);
    assert_eq!(journal.read(0, 16), Ok(ReadOutcome::Pending));
    let first = record(8, 1);
    let second = record(8, 2);
    journal.prepare(first.clone(), WIDE).unwrap().commit();
    journal.prepare(second.clone(), WIDE).unwrap().commit();
    let all = [first.clone(), second.clone()].concat();
    assert_eq!(ready(journal.read(0, 64).unwrap()), all);
    assert_eq!(ready(journal.read(6, 4).unwrap()), all[6..10]);
    assert_eq!(ready(journal.read(3, 0).unwrap()), Vec::<u8>::new());
    assert_eq!(journal.read(16, 8), Ok(ReadOutcome::Pending));
    assert_eq!(journal.read(17, 8), Err(Errno::EINVAL));
    assert_eq!(journal.ack(7, 1), Ok(true));
    assert_eq!(journal.read(7, 8), Err(Errno::ESTALE));
    assert_eq!(ready(journal.read(8, 8).unwrap()), second);
}

#[test]
fn acknowledgement_releases_through_a_retained_sequence_once() {
    let mut journal = Journal::new(7);
    for fill in 1..=3 {
        journal.prepare(record(8, fill), WIDE).unwrap().commit();
    }
    assert_eq!(journal.ack(8, 1), Err(Errno::ESTALE));
    assert_eq!(journal.ack(7, 0), Err(Errno::EINVAL));
    assert_eq!(journal.ack(7, 4), Err(Errno::EINVAL));
    assert_eq!(journal.ack(7, 2), Ok(true));
    assert_eq!(journal.position().records, 1);
    assert_eq!(journal.position().bytes, 8);
    assert_eq!(journal.ack(7, 2), Ok(false));
    assert_eq!(journal.ack(7, 1), Err(Errno::EINVAL));
    assert_eq!(journal.ack(7, 3), Ok(true));
    assert_eq!(journal.position().records, 0);
    assert_eq!(journal.size(), 24);
}

#[test]
fn a_journal_started_at_a_position_treats_earlier_offsets_as_released() {
    let mut journal = Journal::starting_at(3, 10, 100);
    assert_eq!(journal.read(99, 8), Err(Errno::ESTALE));
    assert_eq!(journal.read(100, 8), Ok(ReadOutcome::Pending));
    assert_eq!(journal.prepare(record(8, 1), WIDE).unwrap().commit(), 10);
    assert_eq!(journal.ack(3, 9), Ok(false));
    assert_eq!(journal.ack(3, 10), Ok(true));
    assert_eq!(journal.size(), 108);
}

const STAGING: StagingBounds = StagingBounds {
    header_bytes: 8,
    max_bytes: 32,
    assembly: Duration::from_millis(100),
};

#[test]
fn staging_appends_accepts_exact_repeats_and_refuses_gaps_and_changes() {
    let now = Instant::now();
    let mut staging = Staging::new(4, STAGING);
    let whole = record(12, 5);
    assert_eq!(staging.write(0, &whole[..2], now), Ok(2));
    assert_eq!(staging.write(4, &whole[4..6], now), Err(Errno::EINVAL));
    assert_eq!(staging.write(2, &whole[2..8], now), Ok(6));
    assert_eq!(staging.write(0, &whole[..8], now), Ok(8));
    assert_eq!(staging.write(4, &[9; 2], now), Err(Errno::EINVAL));
    assert_eq!(staging.write(6, &whole[6..10], now), Err(Errno::EINVAL));
    assert_eq!(staging.write(8, &whole[8..], now), Ok(4));
    assert_eq!(staging.write(12, &[0], now), Err(Errno::EINVAL));
    assert_eq!(staging.handle, 4);
    assert_eq!(staging.bytes, whole);
}

#[test]
fn staging_refuses_declared_lengths_outside_the_bounds_before_retaining() {
    let now = Instant::now();
    for declared in [7u32, 33] {
        let mut staging = Staging::new(1, STAGING);
        assert_eq!(
            staging.write(0, &declared.to_le_bytes(), now),
            Err(Errno::EINVAL)
        );
        assert!(staging.bytes.is_empty());
    }
    let mut staging = Staging::new(1, STAGING);
    assert_eq!(staging.write(0, &33u32.to_le_bytes()[..3], now), Ok(3));
    assert_eq!(staging.write(3, &[0], now), Err(Errno::EINVAL));
    assert_eq!(staging.bytes.len(), 3);
    assert_eq!(
        Staging::new(1, STAGING).write(0, &[0; 33], now),
        Err(Errno::EINVAL)
    );
}

#[test]
fn staging_expires_from_its_first_byte_and_bounds_waits() {
    let now = Instant::now();
    let mut staging = Staging::new(1, STAGING);
    assert_eq!(staging.write(0, &[], now), Ok(0));
    assert!(!staging.expired(now + Duration::from_secs(9)));
    assert_eq!(
        staging.wait(now, Duration::from_secs(1)),
        Duration::from_secs(1)
    );
    staging.write(0, &record(12, 1)[..4], now).unwrap();
    assert_eq!(staging.wait(now, Duration::from_secs(1)), STAGING.assembly);
    assert_eq!(
        staging.wait(now + Duration::from_millis(60), Duration::from_millis(10)),
        Duration::from_millis(10)
    );
    let later = now + STAGING.assembly;
    assert!(staging.expired(later));
    assert_eq!(staging.write(4, &[1], later), Err(Errno::ESTALE));
}
