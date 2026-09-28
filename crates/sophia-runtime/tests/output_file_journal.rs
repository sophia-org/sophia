use sophia_9p::journal::{JournalBounds, JournalPosition};
use sophia_9p::{Errno, ReadOutcome};
use sophia_protocol::output_files::*;
use sophia_protocol::*;
use sophia_runtime::*;

fn tx(id: u64) -> TransactionId {
    TransactionId::from_raw(id)
}

fn outcome() -> OutputV1Outcome {
    OutputV1Outcome {
        connection_epoch: 7,
        topology_epoch: 9,
        kind: OutputV1OutcomeKind::Committed,
        reason: 0,
    }
}

fn publication() -> OutputFilePublication {
    OutputFilePublication {
        topology_epoch: 9,
        qid_path: 42,
    }
}

fn welcome() -> OutputV1ServerWelcome {
    OutputV1ServerWelcome {
        connection_epoch: 7,
        selected_revision: 1,
        capabilities: 3,
        max_heads: 16,
        max_groups: 16,
        max_modes_per_head: 128,
        max_heads_per_group: 4,
    }
}

fn ready(journal: &OutputFileJournal, offset: u64) -> Vec<u8> {
    match journal.read(offset, u32::MAX).unwrap() {
        ReadOutcome::Ready(bytes) => bytes,
        ReadOutcome::Pending => panic!("expected records"),
    }
}

fn records(bytes: &[u8]) -> Vec<OutputFileRecord<'_>> {
    let mut left = bytes;
    let mut result = Vec::new();
    while !left.is_empty() {
        let size = u32::from_le_bytes(left[..4].try_into().unwrap()) as usize;
        result.push(decode_output_file_record(&left[..size], OutputFileClass::Event).unwrap());
        left = &left[size..];
    }
    result
}

#[test]
fn a_dropped_admission_spends_no_receipt_identity_or_terminal_credit() {
    let mut journal = OutputFileJournal::new(7, OUTPUT_FILE_JOURNAL_BOUNDS).unwrap();
    let before = journal.position();
    drop(journal.prepare_proposal(1, tx(99), None).unwrap());
    assert_eq!(journal.position(), before);
    assert_eq!(journal.reserved_outcomes(), 0);
    assert_eq!(
        journal.prepare_proposal(1, tx(99), None).unwrap().commit(),
        1
    );
    assert_eq!(journal.reserved_outcomes(), 1);
    assert_eq!(
        decode_output_file_submitted(records(&ready(&journal, 0))[0].body).unwrap(),
        OutputFileSubmitted {
            submission_id: 1,
            candidate_kind: OutputFileKind::Proposal
        }
    );
}

#[test]
fn publication_pressure_cannot_spend_either_reserved_terminal_outcome() {
    // Two receipts (96) plus one publication (56) and two terminal credits
    // (112) use all five records and all 264 bytes.
    let mut journal = OutputFileJournal::new(
        7,
        JournalBounds {
            records: 5,
            bytes: 264,
        },
    )
    .unwrap();
    journal.prepare_proposal(1, tx(99), None).unwrap().commit();
    journal.prepare_proposal(2, tx(2), None).unwrap().commit();
    journal.publish_topology(publication()).unwrap();
    let before = journal.position();
    assert_eq!(journal.publish_topology(publication()), Err(Errno::EAGAIN));
    assert_eq!(journal.position(), before);
    assert_eq!(
        journal.prepare_proposal(3, tx(3), None).err(),
        Some(Errno::EAGAIN)
    );
    assert_eq!(journal.finish(tx(99), outcome()), Ok(4));
    assert_eq!(journal.finish(tx(2), outcome()), Ok(5));
    assert_eq!(journal.reserved_outcomes(), 0);
    assert_eq!(
        journal.position(),
        JournalPosition {
            next_sequence: 6,
            tail: 264,
            records: 5,
            bytes: 264
        }
    );
    let bytes = ready(&journal, 0);
    let records = records(&bytes);
    assert_eq!(
        records
            .iter()
            .map(|r| r.header.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
    assert_eq!(
        decode_output_file_outcome(records[3].body, 7).unwrap().0,
        tx(99)
    );
    assert_eq!(
        decode_output_file_outcome(records[4].body, 7).unwrap().0,
        tx(2)
    );
}

#[test]
fn byte_and_record_limits_each_preserve_credits_independently() {
    for bounds in [
        JournalBounds {
            records: 2,
            bytes: 16_384,
        },
        JournalBounds {
            records: 64,
            bytes: 104,
        },
    ] {
        let mut journal = OutputFileJournal::new(7, bounds).unwrap();
        journal.prepare_proposal(1, tx(1), None).unwrap().commit();
        assert_eq!(journal.publish_topology(publication()), Err(Errno::EAGAIN));
        assert_eq!(journal.finish(tx(1), outcome()), Ok(2));
        assert_eq!(journal.reserved_outcomes(), 0);
    }
}

#[test]
fn queued_replacement_has_one_atomic_receipt_and_stale_outcome() {
    let mut journal = OutputFileJournal::new(
        7,
        JournalBounds {
            records: 4,
            bytes: 216,
        },
    )
    .unwrap();
    journal.prepare_proposal(1, tx(99), None).unwrap().commit();
    journal.prepare_proposal(2, tx(2), None).unwrap().commit();
    let replacement = OutputFileReplacement {
        transaction: tx(2),
        topology_epoch: 9,
    };
    let before = journal.position();
    assert_eq!(
        journal.prepare_proposal(3, tx(3), Some(replacement)).err(),
        Some(Errno::EAGAIN)
    );
    assert_eq!(journal.position(), before);
    assert_eq!(journal.reserved_outcomes(), 2);
    journal
        .acknowledge(OutputFileAck {
            connection_epoch: 7,
            sequence: 2,
        })
        .unwrap();
    assert_eq!(journal.reserved_outcomes(), 2); // Acks do not settle proposals.
    let tail = journal.position().tail;
    let before = journal.position();
    drop(
        journal
            .prepare_proposal(3, tx(3), Some(replacement))
            .unwrap(),
    );
    assert_eq!(journal.position(), before);
    assert_eq!(journal.reserved_outcomes(), 2);
    assert_eq!(
        journal
            .prepare_proposal(3, tx(3), Some(replacement))
            .unwrap()
            .commit(),
        3
    );
    let bytes = ready(&journal, tail);
    let events = records(&bytes);
    assert_eq!(events.len(), 2);
    assert_eq!(
        decode_output_file_submitted(events[0].body)
            .unwrap()
            .submission_id,
        3
    );
    let (old, stale) = decode_output_file_outcome(events[1].body, 7).unwrap();
    assert_eq!(old, tx(2));
    assert_eq!(stale.kind, OutputV1OutcomeKind::Stale);
    assert_eq!(stale.reason, SOPHIA_OUTPUT_OUTCOME_REASON_STALE);
    assert_eq!(journal.finish(tx(2), outcome()), Err(Errno::EINVAL));
    assert_eq!(journal.publish_topology(publication()), Err(Errno::EAGAIN));
    assert_eq!(journal.finish(tx(99), outcome()), Ok(5));
    assert_eq!(journal.finish(tx(3), outcome()), Ok(6));
    assert_eq!(journal.position().bytes, 216);
}

#[test]
fn invalid_outcomes_and_old_epochs_cannot_consume_a_terminal_credit() {
    let mut journal = OutputFileJournal::new(7, OUTPUT_FILE_JOURNAL_BOUNDS).unwrap();
    journal.prepare_proposal(1, tx(1), None).unwrap().commit();
    let before = journal.position();
    for epoch in [0, 6, 8] {
        assert_eq!(
            journal.finish(
                tx(1),
                OutputV1Outcome {
                    connection_epoch: epoch,
                    ..outcome()
                }
            ),
            Err(Errno::ESTALE)
        );
    }
    assert_eq!(
        journal.finish(
            tx(1),
            OutputV1Outcome {
                topology_epoch: 0,
                ..outcome()
            }
        ),
        Err(Errno::EINVAL)
    );
    assert_eq!(journal.finish(tx(9), outcome()), Err(Errno::EINVAL));
    assert_eq!(journal.position(), before);
    assert_eq!(journal.reserved_outcomes(), 1);
    assert_eq!(
        journal.acknowledge(OutputFileAck {
            connection_epoch: 6,
            sequence: 1
        }),
        Err(Errno::ESTALE)
    );
    assert_eq!(journal.finish(tx(1), outcome()), Ok(2));
    assert_eq!(journal.finish(tx(1), outcome()), Err(Errno::EINVAL));
}

#[test]
fn immediate_rejection_does_not_need_or_consume_an_existing_proposals_credit() {
    let mut journal = OutputFileJournal::new(
        7,
        JournalBounds {
            records: 4,
            bytes: 216,
        },
    )
    .unwrap();
    journal.prepare_proposal(1, tx(1), None).unwrap().commit();
    journal.prepare_proposal(2, tx(2), None).unwrap().commit();
    journal
        .acknowledge(OutputFileAck {
            connection_epoch: 7,
            sequence: 2,
        })
        .unwrap();
    let rejected = OutputV1Outcome {
        kind: OutputV1OutcomeKind::Rejected,
        reason: SOPHIA_OUTPUT_OUTCOME_REASON_INVARIANT,
        ..outcome()
    };
    assert_eq!(
        journal.prepare_rejection(3, tx(1), rejected).err(),
        Some(Errno::EINVAL)
    );
    assert_eq!(
        journal.prepare_rejection(3, tx(3), outcome()).err(),
        Some(Errno::EINVAL)
    );
    let before = journal.position();
    drop(journal.prepare_rejection(3, tx(3), rejected).unwrap());
    assert_eq!(journal.position(), before);
    assert_eq!(journal.reserved_outcomes(), 2);
    assert_eq!(
        journal
            .prepare_rejection(3, tx(3), rejected)
            .unwrap()
            .commit(),
        4
    );
    assert_eq!(journal.reserved_outcomes(), 2);
    assert_eq!(journal.finish(tx(1), outcome()), Ok(5));
    assert_eq!(journal.finish(tx(2), outcome()), Ok(6));
    let bytes = ready(&journal, before.tail);
    let events = records(&bytes);
    assert_eq!(
        decode_output_file_outcome(events[1].body, 7).unwrap(),
        (tx(3), rejected)
    );
    assert_eq!(journal.position().bytes, 216);
}

#[test]
fn bootstrap_is_atomic_and_a_refusal_stays_readable_until_acknowledged() {
    let mut small = OutputFileJournal::new(
        7,
        JournalBounds {
            records: 2,
            bytes: 160,
        },
    )
    .unwrap();
    let before = small.position();
    assert_eq!(
        small.negotiated(1, welcome(), publication()),
        Err(Errno::EAGAIN)
    );
    assert_eq!(small.position(), before);
    assert_eq!(
        small.refused(1, OutputFileRefusal::ObservationRequired),
        Ok(2)
    );
    let events = ready(&small, 0);
    let decoded = records(&events);
    assert_eq!(decoded.len(), 2);
    assert_eq!(
        decode_output_file_refused(decoded[1].body).unwrap(),
        OutputFileRefusal::ObservationRequired
    );
    assert_eq!(ready(&small, 0), events);
    assert_eq!(
        small.acknowledge(OutputFileAck {
            connection_epoch: 7,
            sequence: 2
        }),
        Ok(true)
    );
    assert_eq!(small.read(0, 1), Err(Errno::ESTALE));
    let mut journal = OutputFileJournal::new(7, OUTPUT_FILE_JOURNAL_BOUNDS).unwrap();
    assert_eq!(journal.negotiated(1, welcome(), publication()), Ok(3));
    let bytes = ready(&journal, 0);
    let records = records(&bytes);
    assert_eq!(
        records.iter().map(|r| r.header.kind).collect::<Vec<_>>(),
        vec![
            OutputFileKind::Submitted,
            OutputFileKind::Negotiated,
            OutputFileKind::ObjectPublished
        ]
    );
}

#[test]
fn malformed_admissions_and_bounds_leave_the_journal_unchanged() {
    for (epoch, bounds) in [
        (0, OUTPUT_FILE_JOURNAL_BOUNDS),
        (
            7,
            JournalBounds {
                records: 1,
                bytes: 104,
            },
        ),
        (
            7,
            JournalBounds {
                records: 65,
                bytes: 104,
            },
        ),
        (
            7,
            JournalBounds {
                records: 2,
                bytes: 103,
            },
        ),
        (
            7,
            JournalBounds {
                records: 2,
                bytes: 16_385,
            },
        ),
    ] {
        assert!(OutputFileJournal::new(epoch, bounds).is_err());
    }
    let mut journal = OutputFileJournal::new(7, OUTPUT_FILE_JOURNAL_BOUNDS).unwrap();
    let before = journal.position();
    assert_eq!(
        journal.prepare_proposal(0, tx(1), None).err(),
        Some(Errno::EINVAL)
    );
    assert_eq!(
        journal.prepare_proposal(1, tx(0), None).err(),
        Some(Errno::EINVAL)
    );
    assert_eq!(
        journal
            .prepare_proposal(
                1,
                tx(1),
                Some(OutputFileReplacement {
                    transaction: tx(2),
                    topology_epoch: 9
                })
            )
            .err(),
        Some(Errno::EINVAL)
    );
    assert_eq!(journal.position(), before);
    assert_eq!(journal.reserved_outcomes(), 0);
}
