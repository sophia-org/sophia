use sophia_output_client::journal::{Control, Error, Event, Journal};
use sophia_output_protocol::{output_files::*, *};

// Literal peer records, independent of the SDK encoders.
fn record(kind: u16, sequence: u64, body: &[u8]) -> Vec<u8> {
    let mut b = vec![0; 32];
    b[..4].copy_from_slice(&((32 + body.len()) as u32).to_le_bytes());
    b[4..6].copy_from_slice(&1u16.to_le_bytes());
    b[6..8].copy_from_slice(&kind.to_le_bytes());
    b[8..16].copy_from_slice(&9u64.to_le_bytes());
    b[24..32].copy_from_slice(&sequence.to_le_bytes());
    b.extend_from_slice(body);
    b
}
fn welcome(caps: u64) -> Vec<u8> {
    let mut b = vec![0; 24];
    b[..2].copy_from_slice(&1u16.to_le_bytes());
    b[8..16].copy_from_slice(&caps.to_le_bytes());
    b[16..].copy_from_slice(&[16, 0, 16, 0, 128, 0, 4, 0]);
    record(16, 1, &b)
}
fn outcome(sequence: u64, kind: u16) -> Vec<u8> {
    let mut b = vec![0; 24];
    b[..8].copy_from_slice(&41u64.to_le_bytes());
    b[8..16].copy_from_slice(&7u64.to_le_bytes());
    b[16..18].copy_from_slice(&kind.to_le_bytes());
    b[18..20].copy_from_slice(&60000u16.to_le_bytes());
    record(32, sequence, &b)
}
fn publication(sequence: u64) -> Vec<u8> {
    let mut b = vec![0; 24];
    b[0] = 2;
    b[8..16].copy_from_slice(&7u64.to_le_bytes());
    b[16..24].copy_from_slice(&55u64.to_le_bytes());
    record(19, sequence, &b)
}
fn submitted(sequence: u64) -> Vec<u8> {
    let mut b = vec![0; 16];
    b[0] = 2;
    b[8..10].copy_from_slice(&257u16.to_le_bytes());
    record(18, sequence, &b)
}
fn journal(caps: u64) -> Journal {
    Journal::new(
        9,
        OutputFileLimits::default(),
        OutputV1ClientHello {
            minimum_revision: 1,
            maximum_revision: 1,
            capabilities: caps,
        },
        0,
    )
    .unwrap()
}
fn append(j: &mut Journal, b: &[u8], now: u64) -> Result<(), Error> {
    j.append(b, now, |_, _| Ok(()))
}

#[test]
fn held_publication_blocks_delivery_and_ack_but_not_custody_validation() {
    let mut j = journal(3);
    append(&mut j, &welcome(3), 0).unwrap();
    let bytes = [publication(2), submitted(3), outcome(4, 2)].concat();
    let mut controls = vec![];
    j.append(&bytes, 1, |h, c| {
        controls.push((h.sequence, c));
        Ok(())
    })
    .unwrap();
    assert!(matches!(controls.as_slice(), [(3, Control::Submitted(_))]));
    assert_eq!(j.consumed(), 1);
    assert!(j.head().is_none());
    assert_eq!(j.consume(), Err(Error::Empty));
    let (seq, p) = j.pending_publication().unwrap();
    assert_eq!(j.publication_verified(seq + 1, p), Err(Error::Invalid));
    assert_eq!(
        j.publication_verified(seq, OutputFilePublication { qid_path: 56, ..p }),
        Err(Error::Invalid)
    );
    j.publication_verified(seq, p).unwrap();
    assert!(matches!(j.head().unwrap().1, Event::ObjectPublished(_)));
    assert_eq!(j.consume(), Ok(3));
    assert_eq!(j.head().unwrap().0.sequence, 4);
    assert_eq!(j.consume(), Ok(4));
    append(&mut j, &publication(5), 2).unwrap();
    assert!(j.head().is_none()); // Verification never carries to a new publication.
}

#[test]
fn all_outcomes_preserve_unknown_reasons_and_order() {
    let mut j = journal(3);
    append(&mut j, &welcome(1), 0).unwrap();
    for kind in 1..=6 {
        append(&mut j, &outcome(u64::from(kind) + 1, kind), 1).unwrap();
    }
    for sequence in 2..=7 {
        let (h, Event::Outcome(tx, value)) = j.head().unwrap() else {
            panic!()
        };
        assert_eq!(h.sequence, sequence);
        assert_eq!(tx.raw(), 41);
        assert_eq!(value.reason, 60000);
        assert_eq!(value.connection_epoch, 9);
        assert_eq!(j.consume(), Ok(sequence));
    }
}

#[test]
fn refusal_is_internal_ack_target_and_no_later_record_is_allowed() {
    let refused = record(17, 1, &[1, 0, 0, 0, 0, 0, 0, 0]);
    let mut j = journal(1);
    append(&mut j, &refused, 0).unwrap();
    assert_eq!(
        j.refusal(),
        Some((1, OutputFileRefusal::UnsupportedRevision))
    );
    assert_eq!(j.consumed(), 1);
    assert!(j.head().is_none());
    assert_eq!(append(&mut j, &submitted(2), 0), Err(Error::Invalid));
    let mut j = journal(1);
    append(&mut j, &welcome(1), 0).unwrap();
    assert_eq!(
        append(&mut j, &record(17, 2, &[2, 0, 0, 0, 0, 0, 0, 0]), 1),
        Err(Error::Invalid)
    );
}

#[test]
fn negotiation_rejects_unrequested_grants_duplicate_and_pre_negotiation_events() {
    for bytes in [welcome(3), outcome(1, 1), publication(1)] {
        assert_eq!(append(&mut journal(1), &bytes, 0), Err(Error::Invalid));
    }
    let mut j = journal(3);
    append(&mut j, &welcome(3), 0).unwrap();
    let mut duplicate = welcome(3);
    duplicate[24..32].copy_from_slice(&2u64.to_le_bytes());
    assert_eq!(append(&mut j, &duplicate, 0), Err(Error::Invalid));
    let mut j = Journal::new(
        9,
        OutputFileLimits::default(),
        OutputV1ClientHello {
            minimum_revision: 2,
            maximum_revision: 3,
            capabilities: 3,
        },
        0,
    )
    .unwrap();
    assert_eq!(append(&mut j, &welcome(3), 0), Err(Error::Invalid));
}

#[test]
fn fragmentation_deadline_and_clock_regression_preserve_bounds() {
    let mut j = Journal::new(
        9,
        OutputFileLimits {
            ack_progress_timeout_millis: 17,
            ..OutputFileLimits::default()
        },
        OutputV1ClientHello {
            minimum_revision: 1,
            maximum_revision: 1,
            capabilities: 1,
        },
        0,
    )
    .unwrap();
    let w = welcome(1);
    append(&mut j, &w[..3], 10).unwrap();
    assert_eq!(j.fragment_deadline(), Some(27));
    assert_eq!(append(&mut j, &w[3..4], 9), Err(Error::ClockRegression));
    assert_eq!(j.offset(), 3);
    append(&mut j, &w[3..4], 26).unwrap();
    assert_eq!(j.fragment_deadline(), Some(27));
    assert_eq!(append(&mut j, &w[4..], 27), Err(Error::Deadline));
    assert_eq!(j.capacity(), 0);
    let mut j = journal(1);
    for byte in w {
        append(&mut j, &[byte], 0).unwrap();
    }
    assert_eq!(j.selected(), Some(1));
    assert_eq!(j.fragment_deadline(), None);
}

#[test]
fn malformed_record_stops_later_custody_and_poison_is_sticky() {
    let mut j = journal(1);
    append(&mut j, &welcome(1), 0).unwrap();
    let bytes = [submitted(2), outcome(3, 99), submitted(4)].concat();
    let mut seen = vec![];
    assert_eq!(
        j.append(&bytes, 1, |h, _| {
            seen.push(h.sequence);
            Ok(())
        }),
        Err(Error::Invalid)
    );
    assert_eq!(seen, [2]);
    assert!(j.head().is_none());
    assert_eq!(append(&mut j, &submitted(5), 2), Err(Error::Invalid));
}

#[test]
fn retained_record_capacity_is_bounded() {
    let mut j = journal(1);
    append(&mut j, &welcome(1), 0).unwrap();
    for sequence in 2..=65 {
        append(&mut j, &outcome(sequence, 1), 0).unwrap();
    }
    assert_eq!(j.capacity(), 0);
    assert_eq!(append(&mut j, &outcome(66, 1), 0), Err(Error::Overflow));
}
