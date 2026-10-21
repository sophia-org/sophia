use super::*;
use crate::owned_ring_protocol::{EVENT_BYTES, Event, EventKind, Identity};
use sophia_protocol::ContentGrant;

fn event(epoch: u64) -> Event {
    Event::ring(
        EventKind::RingReady,
        Identity {
            grant: ContentGrant {
                connection_epoch: epoch,
                content_grant_epoch: 2,
            },
            channel: 3,
            ring: 4,
            generation: 5,
        },
        0,
    )
}

fn journal(epoch: u64) -> Journal {
    Journal::new(
        epoch,
        JournalBounds {
            bytes: 4096,
            reserve_bytes: 2048,
        },
        Instant::now(),
    )
}

#[test]
fn owned_prepare_is_atomic_and_shares_sequence_and_ack() {
    let mut journal = journal(1);
    let mut incoming = event(1);
    incoming.sequence = 999;
    drop(journal.prepare_owned_event(incoming, true).unwrap());
    assert_eq!((journal.records(), journal.size()), (0, 0));
    assert_eq!(
        journal
            .prepare_owned_event(incoming, true)
            .unwrap()
            .commit(),
        1
    );
    let ReadOutcome::Ready(bytes) = journal.read(0, EVENT_BYTES as u32).unwrap() else {
        panic!("record pending")
    };
    incoming.sequence = 1;
    assert_eq!(Event::decode(&bytes), Some(incoming));
    // A normal stable event follows in the very same stream and sequence.
    let mut body = [0; 16];
    body[0] = 1;
    body[8..10].copy_from_slice(&(ShellFileKind::Negotiate as u16).to_le_bytes());
    assert_eq!(journal.append(ShellFileKind::Submitted, &body, true), Ok(2));
    assert_eq!(journal.records(), 2);
    let ack = |epoch, sequence| ShellFileAck {
        connection_epoch: epoch,
        sequence,
    };
    assert!(journal.ack(ack(2, 2), Instant::now()).is_err());
    assert_eq!(journal.records(), 2);
    journal.ack(ack(1, 1), Instant::now()).unwrap();
    assert_eq!(journal.records(), 1);
    assert!(journal.read(0, EVENT_BYTES as u32).is_err());
    journal.ack(ack(1, 2), Instant::now()).unwrap();
    assert_eq!(journal.records(), 0);
}

#[test]
fn owned_invalid_event_does_not_spend_reservation_or_sequence() {
    let mut journal = journal(1);
    let hold = journal.hold_reservation(EVENT_BYTES).unwrap();
    let mut invalid = event(1);
    invalid.reason = 1;
    for refused in [event(2), invalid] {
        assert!(journal.prepare_owned_event(refused, true).is_err());
        assert_eq!(
            journal.append_owned_event(hold, refused),
            Err(Errno::EINVAL)
        );
        assert_eq!((journal.records(), journal.size()), (0, 0));
    }
    assert_eq!(journal.append_owned_event(hold, event(1)), Ok(1));
    assert_eq!(
        journal.append_owned_event(hold, event(1)),
        Err(Errno::EINVAL)
    );
}

#[test]
fn owned_exact_size_and_epoch_holds_cannot_be_reused() {
    let mut first = journal(1);
    let wrong_size = first.hold_reservation(EVENT_BYTES + 8).unwrap();
    assert_eq!(
        first.append_owned_event(wrong_size, event(1)),
        Err(Errno::EINVAL)
    );
    assert!(
        first
            .held
            .iter()
            .flatten()
            .any(|(held, _)| *held == wrong_size)
    );
    first.release_reservation(wrong_size);
    let released = first.hold_reservation(EVENT_BYTES).unwrap();
    first.release_reservation(released);
    let fresh = first.hold_reservation(EVENT_BYTES).unwrap();
    assert_eq!(
        first.append_owned_event(released, event(1)),
        Err(Errno::EINVAL)
    );
    let mut second = journal(2);
    let own = second.hold_reservation(EVENT_BYTES).unwrap();
    assert_eq!(
        second.append_owned_event(fresh, event(2)),
        Err(Errno::EINVAL)
    );
    assert_eq!(second.append_owned_event(own, event(2)), Ok(1));
    assert_eq!(first.append_owned_event(fresh, event(1)), Ok(1));
}

#[test]
fn owned_holds_survive_record_and_byte_backpressure() {
    for bounds in [
        JournalBounds {
            bytes: 65_536,
            reserve_bytes: 12_288,
        },
        JournalBounds {
            bytes: 1200,
            reserve_bytes: 1080,
        },
    ] {
        let mut journal = Journal::new(1, bounds, Instant::now());
        let holds: Vec<_> = (0..9)
            .map(|_| journal.hold_reservation(EVENT_BYTES).unwrap())
            .collect();
        assert!(journal.hold_reservation(EVENT_BYTES).is_none());
        while let Ok(prepared) = journal.prepare_owned_event(event(1), false) {
            prepared.commit();
        }
        while let Ok(prepared) = journal.prepare_owned_event(event(1), true) {
            prepared.commit();
        }
        let before = journal.records();
        for (i, hold) in holds.into_iter().rev().enumerate() {
            assert_eq!(
                journal.append_owned_event(hold, event(1)),
                Ok((before + i + 1) as u64)
            );
        }
        assert!(journal.prepare_owned_event(event(1), true).is_err());
    }
}
