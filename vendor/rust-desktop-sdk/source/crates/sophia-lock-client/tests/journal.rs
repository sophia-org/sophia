use sophia_lock_client::{journal::*, protocol::lock_files::*};

// Literal peer records: do not use the SDK's envelope encoder.
fn record(kind: u16, sequence: u64, body: &[u8]) -> Vec<u8> {
    let mut b = vec![0; 32];
    b[..4].copy_from_slice(&((32 + body.len()) as u32).to_le_bytes());
    b[4..6].copy_from_slice(&1u16.to_le_bytes());
    b[6..8].copy_from_slice(&kind.to_le_bytes());
    b[8..16].copy_from_slice(&7u64.to_le_bytes());
    b[24..32].copy_from_slice(&sequence.to_le_bytes());
    b.extend_from_slice(body);
    b
}
fn journal() -> Journal {
    Journal::new(
        7,
        &LockNegotiate {
            minimum_revision: 1,
            maximum_revision: 1,
            requested_capabilities: 1,
            chords: vec![],
        },
    )
    .unwrap()
}
fn welcome() -> Vec<u8> {
    record(16, 1, &[1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0])
}
fn entry(seq: u64) -> Vec<u8> {
    record(40, seq, &[9, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0])
}
fn publication(seq: u64, generation: u64) -> Vec<u8> {
    let mut b = vec![2, 0, 0, 0, 0, 0, 0, 0];
    b.extend_from_slice(&generation.to_le_bytes());
    b.extend_from_slice(&(90 + generation).to_le_bytes());
    record(19, seq, &b)
}
fn advance(j: &mut Journal) {
    j.advance(|_, _| Ok(())).unwrap();
}

#[test]
fn fragments_retained_heads_and_controls_are_strictly_ordered() {
    let mut j = journal();
    let bytes = welcome();
    for b in &bytes[..bytes.len() - 1] {
        j.append(&[*b]).unwrap();
        advance(&mut j);
        assert!(j.welcome().is_none());
    }
    j.append(&bytes[bytes.len() - 1..]).unwrap();
    advance(&mut j);
    assert_eq!(j.consumed(), 1);
    let mut bytes = entry(2);
    bytes.extend(record(
        18,
        3,
        &[2, 0, 0, 0, 0, 0, 0, 0, 7, 1, 0, 0, 0, 0, 0, 0],
    ));
    j.append(&bytes).unwrap();
    j.advance(|_, _| panic!("must not scan behind Entry"))
        .unwrap();
    assert_eq!(j.head().unwrap().0.sequence, 2);
    assert_eq!(j.consumed(), 1);
    j.advance(|_, _| panic!("held head")).unwrap();
    j.consume().unwrap();
    let mut count = 0;
    j.advance(|h, c| {
        count += 1;
        assert_eq!(h.sequence, 3);
        assert!(matches!(c, Control::Submitted(v) if v.submission_id == 2 && v.candidate_kind == LockFileKind::FrameDemand));
        Ok(())
    }).unwrap();
    assert_eq!(count, 1);
    assert_eq!(j.consumed(), 3);
    assert_eq!(j.capacity(), JOURNAL_BYTES);
}

#[test]
fn publication_requires_resolution_and_superseded_head_is_internal() {
    let mut j = journal();
    let mut b = welcome();
    b.extend(publication(2, 1));
    b.extend(publication(3, 2));
    j.append(&b).unwrap();
    advance(&mut j);
    assert!(j.head().is_none());
    assert_eq!(j.consume(), Err(Error::Empty));
    let (s, p) = j.pending_publication().unwrap();
    assert!(j.publication_resolved(s + 1, p, true).is_err());
    j.publication_resolved(s, p, true).unwrap();
    assert_eq!(j.consumed(), 2);
    advance(&mut j);
    let (s, p) = j.pending_publication().unwrap();
    j.publication_resolved(s, p, false).unwrap();
    assert!(matches!(j.head().unwrap().1, Event::ObjectPublished(v) if v.object_generation == 2));
    j.consume().unwrap();
    j.append(&publication(4, 2)).unwrap();
    assert_eq!(j.advance(|_, _| Ok(())), Err(Error::Invalid));
}

#[test]
fn malformed_suffix_preserves_prefix_but_never_scans_past_fault() {
    let mut j = journal();
    let mut b = welcome();
    b.extend(record(40, 2, &[0; 16]));
    b.extend(entry(3));
    j.append(&b).unwrap();
    let mut controls = 0;
    assert_eq!(
        j.advance(|_, _| {
            controls += 1;
            Ok(())
        }),
        Err(Error::Invalid)
    );
    assert_eq!(controls, 1);
    assert!(j.welcome().is_some());
    assert_eq!(j.consumed(), 1);
    assert!(j.head().is_none());
    assert_eq!(j.capacity(), 0);
    assert_eq!(j.advance(|_, _| panic!()), Err(Error::Invalid));
    assert_eq!(j.append(&entry(4)), Err(Error::Invalid));
}

#[test]
fn compaction_preserves_partial_record_and_exact_offset() {
    let mut j = journal();
    j.append(&welcome()).unwrap();
    advance(&mut j);
    // Retain a one-byte prefix while advancing the cursor toward the end.
    let mut b = entry(2);
    b.push(48);
    j.append(&b).unwrap();
    advance(&mut j);
    j.consume().unwrap();
    for seq in 3..80 {
        let record = entry(seq);
        let mut tail = record[1..].to_vec();
        tail.push(48);
        j.append(&tail).unwrap();
        advance(&mut j);
        assert_eq!(j.head().unwrap().0.sequence, seq);
        j.consume().unwrap();
        assert_eq!(j.capacity(), JOURNAL_BYTES - 1);
    }
    assert_eq!(j.offset(), 48 * 79 + 1);
}

#[test]
fn grants_chords_refusal_and_capacity_fail_closed() {
    let mut granted = Journal::new(
        7,
        &LockNegotiate {
            minimum_revision: 1,
            maximum_revision: 1,
            requested_capabilities: 3,
            chords: vec![LockChordRequest {
                keysym: 0x62,
                modifiers: 2,
            }],
        },
    )
    .unwrap();
    let mut grant = welcome();
    grant[34] = 1;
    grant[40] = 3;
    granted.append(&grant).unwrap();
    advance(&mut granted);
    granted
        .append(&record(
            41,
            2,
            &[9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ))
        .unwrap();
    advance(&mut granted);
    assert!(matches!(granted.head().unwrap().1, Event::Chord(v) if v.chord == 0));
    let mut j = journal();
    let mut b = welcome();
    b[40] = 3; // unrequested Chords capability
    j.append(&b).unwrap();
    assert_eq!(j.advance(|_, _| panic!()), Err(Error::Invalid));
    let mut j = journal();
    j.append(&welcome()).unwrap();
    advance(&mut j);
    j.append(&record(
        41,
        2,
        &[9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    ))
    .unwrap();
    assert_eq!(j.advance(|_, _| panic!()), Err(Error::Invalid));
    let mut j = journal();
    j.append(&record(17, 1, &[1, 0, 0, 0, 0, 0, 0, 0])).unwrap();
    advance(&mut j);
    assert_eq!(j.refusal().unwrap().0, 1);
    assert_eq!(j.consumed(), 1); // driver must settle this ACK before terminating
    j.append(&welcome()).unwrap();
    assert_eq!(j.advance(|_, _| panic!()), Err(Error::Invalid));
    let mut j = journal();
    assert_eq!(j.append(&[0; JOURNAL_BYTES + 1]), Err(Error::Overflow));
    assert_eq!(j.offset(), 0);
}

#[test]
fn resource_presentation_and_pacing_events_remain_public_heads() {
    let mut j = journal();
    j.append(&welcome()).unwrap();
    advance(&mut j);
    for (index, (kind, size, ids)) in [(33, 40, 3), (34, 32, 3), (35, 56, 5), (36, 48, 5)]
        .into_iter()
        .enumerate()
    {
        let mut body = vec![0; size];
        for id in 0..ids {
            body[id * 8] = 1;
        }
        if kind == 33 {
            body[24] = 1;
        }
        if kind == 35 {
            body[40] = 1;
        }
        if kind == 36 {
            body[40] = 1;
        }
        let sequence = index as u64 + 2;
        j.append(&record(kind, sequence, &body)).unwrap();
        advance(&mut j);
        let (header, event) = j.head().unwrap();
        assert_eq!(header.sequence, sequence);
        assert!(matches!(
            (kind, event),
            (33, Event::ResourceStatus(_))
                | (34, Event::ResourceReleased(_))
                | (35, Event::CandidateOutcome(_))
                | (36, Event::FramePermit(_))
        ));
        assert_eq!(j.consumed(), sequence - 1);
        assert_eq!(j.consume().unwrap(), sequence);
    }
}
