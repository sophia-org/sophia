use super::*;

fn commit_record(generation: u64) -> ShellFrameRecord {
    let line = include_str!("../../../../spec/golden/sophia-frame-transactions-v1.tsv")
        .lines()
        .find(|l| l.starts_with("commit-retained\t"))
        .unwrap();
    let bytes: Vec<_> = line
        .split('\t')
        .nth(2)
        .unwrap()
        .as_bytes()
        .chunks_exact(2)
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect();
    let ShellFrameRecord::Commit(mut v) =
        decode_shell_frame_record(&bytes, ShellFileClass::Candidate, 13, 8192).unwrap()
    else {
        panic!("commit golden");
    };
    v.transaction = tx(20 + generation);
    v.candidate.grant = key().grant;
    v.candidate.output = key().output;
    v.candidate.facts_generation = key().facts_generation;
    v.candidate.pacing_permit = key().stream_id;
    v.candidate.surfaces[0].allocation = key().allocation;
    v.candidate.candidate_generation = generation;
    ShellFrameRecord::Commit(v)
}

fn custody(c: &mut ShellConnection, p: &mut Peer, ticket: Ticket) {
    let held = submit(c, p);
    let submitted = decode_shell_file_submit(&held.data).unwrap();
    let kind = decode_shell_file_record_at_revision(
        p.transactions.last().unwrap(),
        ShellFileClass::Candidate,
        13,
    )
    .unwrap()
    .header
    .kind;
    p.answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
    let receipt = encode_shell_file_submitted_at_revision(
        ShellFileHeader {
            kind: ShellFileKind::Submitted,
            connection_epoch: EPOCH,
            submission_id: 0,
            sequence: p.take_sequence(),
        },
        ShellFileSubmitted {
            submission_id: submitted.submission_id,
            candidate_kind: kind,
        },
        13,
    )
    .unwrap();
    p.push(receipt);
    drive(c, p, |c, _| c.custody(ticket) == Some(Custody::Submitted)).unwrap();
}
fn delivered(
    c: &mut ShellConnection,
    p: &mut Peer,
    s: &mut FrameStream,
    record: ShellFrameRecord,
) -> Result<(), ShellClientError> {
    let bytes = event(p, &record);
    let sequence = decode_shell_file_record_at_revision(&bytes, ShellFileClass::Event, 13)
        .unwrap()
        .header
        .sequence;
    p.push(bytes);
    drive(c, p, |c, _| c.journal_progress().handled >= sequence).unwrap();
    let got = c.take_frame().unwrap().unwrap();
    assert_eq!(got, record);
    s.observe(&got)
}
fn opened(key: ShellFrameStreamKey) -> ShellFrameRecord {
    ShellFrameRecord::Opened {
        transaction: tx(10),
        key,
        ttl_millis: 100,
    }
}
fn closed(transaction: u64) -> ShellFrameRecord {
    ShellFrameRecord::Closed {
        transaction: tx(transaction),
        key: key(),
        reason: 1,
    }
}
fn result(generation: u64, kind: u16, deadline: u64) -> ShellFrameRecord {
    ShellFrameRecord::Outcome {
        transaction: tx(20 + generation),
        outcome: ContentCandidateOutcome {
            grant: key().grant,
            candidate_generation: generation,
            output: key().output,
            kind,
            reason: if kind <= 2 { 0 } else { 1 },
            presentation_epoch: u64::from(kind == 2),
            work_area_generation: 0,
            wm_commit_generation: 0,
        },
        stream_id: key().stream_id,
        allocation: key().allocation,
        facts_generation: key().facts_generation,
        credit_expires_msec: deadline,
    }
}
fn refused(request_kind: ShellFileKind, transaction: u64, generation: u64) -> ShellFrameRecord {
    ShellFrameRecord::Refused {
        transaction: tx(transaction),
        grant: key().grant,
        output: key().output,
        stream_id: key().stream_id,
        candidate_generation: generation,
        request_kind,
        reason: 1,
    }
}
fn ready() -> (ShellConnection, Peer, FrameStream) {
    let (mut c, mut p) = frame_connect(profile(), frame_options(), 8192).unwrap();
    let mut s = FrameStream::new();
    let a = s.open(&mut c, tx(10), key()).unwrap();
    assert_eq!(s.state(), FrameStreamState::Opening);
    assert!(!s.has_credit());
    custody(&mut c, &mut p, a.first);
    assert!(!s.has_credit(), "custody never grants credit");
    delivered(&mut c, &mut p, &mut s, opened(key())).unwrap();
    (c, p, s)
}
fn submit_one(c: &mut ShellConnection, p: &mut Peer, s: &mut FrameStream) {
    let a = s.submit(c, &commit_record(1)).unwrap();
    custody(c, p, a.first);
    assert!(!s.has_credit());
}

#[test]
fn one_credit_returns_only_on_exact_terminal_once() {
    let (mut c, mut p, mut s) = ready();
    submit_one(&mut c, &mut p, &mut s);
    assert!(s.submit(&mut c, &commit_record(2)).is_err());
    delivered(&mut c, &mut p, &mut s, result(1, 1, 0)).unwrap();
    assert!(!s.has_credit());
    delivered(&mut c, &mut p, &mut s, result(1, 2, 123)).unwrap();
    assert!(s.has_credit());
    assert!(s.submit(&mut c, &commit_record(1)).is_err());
    assert!(delivered(&mut c, &mut p, &mut s, result(1, 2, 123)).is_err());
    assert_eq!(s.state(), FrameStreamState::Unknown);
    assert!(!s.has_credit());
}

#[test]
fn closing_drains_in_both_orders_before_reopen() {
    for close_first in [true, false] {
        let (mut c, mut p, mut s) = ready();
        submit_one(&mut c, &mut p, &mut s);
        let a = s.close(&mut c, tx(30)).unwrap();
        custody(&mut c, &mut p, a.first);
        assert!(!s.has_credit());
        let next = ShellFrameStreamKey {
            stream_id: 2,
            ..key()
        };
        let records = if close_first {
            [closed(30), result(1, 3, 0)]
        } else {
            [result(1, 3, 123), closed(30)]
        };
        delivered(&mut c, &mut p, &mut s, records[0].clone()).unwrap();
        assert!(!s.frame_work_drained());
        assert!(s.open(&mut c, tx(31), next).is_err());
        delivered(&mut c, &mut p, &mut s, records[1].clone()).unwrap();
        assert!(s.frame_work_drained());
        assert!(s.open(&mut c, tx(31), key()).is_err());
        assert!(s.open(&mut c, tx(31), next).is_ok());
    }
}

#[test]
fn automatic_close_waits_for_the_racing_close_refusal() {
    let (mut c, mut p, mut s) = ready();
    let a = s.close(&mut c, tx(30)).unwrap();
    custody(&mut c, &mut p, a.first);
    delivered(&mut c, &mut p, &mut s, closed(10)).unwrap();
    assert!(!s.frame_work_drained());
    assert!(
        s.open(
            &mut c,
            tx(31),
            ShellFrameStreamKey {
                stream_id: 2,
                ..key()
            }
        )
        .is_err()
    );
    delivered(
        &mut c,
        &mut p,
        &mut s,
        refused(ShellFileKind::FrameStreamClose, 30, 0),
    )
    .unwrap();
    assert!(s.frame_work_drained());
}

#[test]
fn refusal_releases_local_credit_but_not_generation() {
    let (mut c, mut p, mut s) = ready();
    submit_one(&mut c, &mut p, &mut s);
    delivered(
        &mut c,
        &mut p,
        &mut s,
        refused(ShellFileKind::FrameCommit, 21, 1),
    )
    .unwrap();
    assert!(s.has_credit());
    assert!(s.submit(&mut c, &commit_record(1)).is_err());
    assert!(s.submit(&mut c, &commit_record(2)).is_ok());
}

#[test]
fn terminal_without_credit_waits_for_closed_and_close_refusal_is_retryable() {
    let (mut c, mut p, mut s) = ready();
    submit_one(&mut c, &mut p, &mut s);
    delivered(&mut c, &mut p, &mut s, result(1, 3, 0)).unwrap();
    assert_eq!(s.state(), FrameStreamState::Closing);
    assert!(s.submit(&mut c, &commit_record(2)).is_err());
    let a = s.close(&mut c, tx(30)).unwrap();
    custody(&mut c, &mut p, a.first);
    delivered(
        &mut c,
        &mut p,
        &mut s,
        refused(ShellFileKind::FrameStreamClose, 30, 0),
    )
    .unwrap();
    assert!(!s.has_credit());
    let a = s.close(&mut c, tx(31)).unwrap();
    custody(&mut c, &mut p, a.first);
    delivered(&mut c, &mut p, &mut s, closed(31)).unwrap();
    assert!(s.frame_work_drained());
}

#[test]
fn wrong_completion_tuple_poisoned_without_releasing_credit() {
    for field in 0..9 {
        let (mut c, mut p, mut s) = ready();
        submit_one(&mut c, &mut p, &mut s);
        let mut wrong = result(1, 3, 123);
        if let ShellFrameRecord::Outcome {
            transaction,
            outcome,
            stream_id,
            allocation,
            facts_generation,
            ..
        } = &mut wrong
        {
            match field {
                0 => *transaction = tx(99),
                1 => outcome.grant.content_grant_epoch += 1,
                2 => outcome.candidate_generation += 1,
                3 => outcome.output.id += 1,
                4 => outcome.output.generation += 1,
                5 => *stream_id += 1,
                6 => allocation.id += 1,
                7 => allocation.generation += 1,
                8 => *facts_generation += 1,
                _ => unreachable!(),
            }
        }
        assert!(
            delivered(&mut c, &mut p, &mut s, wrong).is_err(),
            "field {field}"
        );
        assert_eq!(s.state(), FrameStreamState::Unknown);
        assert!(!s.has_credit());
        assert!(!s.frame_work_drained());
        assert!(s.observe(&result(1, 3, 0)).is_err());
    }
}

#[test]
fn queue_refusal_never_spends_stream_credit_or_candidate_generation() {
    let (mut c, _p, mut s) = ready();
    c.set_queue_limits(QueueLimits {
        records: 1,
        bytes: 8192,
        control_records: 0,
        control_bytes: 0,
    })
    .unwrap();
    let blocker = c.reserve_capacity(1, 8192).unwrap();
    assert!(s.submit(&mut c, &commit_record(1)).is_err());
    assert!(s.close(&mut c, tx(30)).is_err());
    assert!(s.has_credit());
    assert_eq!(s.state(), FrameStreamState::Open);
    c.cancel_reservation(&blocker).unwrap();
    assert!(s.submit(&mut c, &commit_record(1)).is_ok());
}

#[test]
fn opening_refusal_does_not_spend_stream_id_and_disconnect_never_reopens() {
    let (mut c, mut p) = frame_connect(profile(), frame_options(), 8192).unwrap();
    let mut s = FrameStream::new();
    let a = s.open(&mut c, tx(10), key()).unwrap();
    custody(&mut c, &mut p, a.first);
    delivered(
        &mut c,
        &mut p,
        &mut s,
        refused(ShellFileKind::FrameStreamOpen, 10, 0),
    )
    .unwrap();
    assert!(s.frame_work_drained());
    assert!(s.open(&mut c, tx(11), key()).is_ok());
    s.disconnect();
    assert!(matches!(
        s.submit(&mut c, &commit_record(1)),
        Err(ShellClientError::InvalidArgument(_))
    ));
    assert!(!s.has_credit());
    assert!(!s.frame_work_drained());
    assert!(
        s.open(
            &mut c,
            tx(12),
            ShellFrameStreamKey {
                stream_id: 2,
                ..key()
            }
        )
        .is_err()
    );
}

#[test]
fn prepared_then_refused_or_duplicate_prepared_is_a_protocol_fault() {
    for later in [result(1, 1, 0), refused(ShellFileKind::FrameCommit, 21, 1)] {
        let (mut c, mut p, mut s) = ready();
        submit_one(&mut c, &mut p, &mut s);
        delivered(&mut c, &mut p, &mut s, result(1, 1, 0)).unwrap();
        assert!(delivered(&mut c, &mut p, &mut s, later).is_err());
        assert_eq!(s.state(), FrameStreamState::Unknown);
    }
}

#[test]
fn wrong_submission_binding_preserves_available_credit() {
    let (mut c, mut p, mut s) = ready();
    for field in 0..8 {
        let mut r = commit_record(1);
        if let ShellFrameRecord::Commit(v) = &mut r {
            match field {
                0 => v.candidate.grant.content_grant_epoch += 1,
                1 => v.candidate.output.id += 1,
                2 => v.candidate.output.generation += 1,
                3 => v.candidate.facts_generation += 1,
                4 => v.candidate.pacing_permit += 1,
                5 => v.candidate.surfaces[0].allocation.id += 1,
                6 => v.candidate.surfaces[0].allocation.generation += 1,
                7 => v.candidate.surfaces.clear(),
                _ => unreachable!(),
            }
        }
        assert!(s.submit(&mut c, &r).is_err());
        assert!(s.has_credit());
    }
    assert!(
        s.close(&mut c, tx(10)).is_err(),
        "automatic-close transaction must remain distinct"
    );
    submit_one(&mut c, &mut p, &mut s);
}

#[test]
fn queue_full_open_and_wrong_grant_reopen_leave_closed_owner_unchanged() {
    let (mut c, mut p) = frame_connect(profile(), frame_options(), 8192).unwrap();
    c.set_queue_limits(QueueLimits {
        records: 1,
        bytes: 8192,
        control_records: 0,
        control_bytes: 0,
    })
    .unwrap();
    let reservation = c.reserve_capacity(1, 8192).unwrap();
    let mut s = FrameStream::new();
    assert!(s.open(&mut c, tx(10), key()).is_err());
    assert!(s.frame_work_drained());
    assert_eq!(s.key(), None);
    c.cancel_reservation(&reservation).unwrap();
    let a = s.open(&mut c, tx(10), key()).unwrap();
    custody(&mut c, &mut p, a.first);
    delivered(&mut c, &mut p, &mut s, opened(key())).unwrap();
    delivered(&mut c, &mut p, &mut s, closed(10)).unwrap();
    let mut wrong = ShellFrameStreamKey {
        stream_id: 2,
        ..key()
    };
    wrong.grant.content_grant_epoch += 1;
    assert!(s.open(&mut c, tx(11), wrong).is_err());
    assert!(s.frame_work_drained());
}

#[test]
fn closed_stream_cannot_return_new_credit_or_reaccept_old_events() {
    let (mut c, mut p, mut s) = ready();
    submit_one(&mut c, &mut p, &mut s);
    delivered(&mut c, &mut p, &mut s, closed(10)).unwrap();
    assert!(delivered(&mut c, &mut p, &mut s, result(1, 3, 100)).is_err());
    assert!(!s.has_credit());
    assert!(!s.frame_work_drained());
    let (mut c, mut p, mut s) = ready();
    assert!(delivered(&mut c, &mut p, &mut s, opened(key())).is_err());
    assert_eq!(s.state(), FrameStreamState::Unknown);
}

#[test]
fn terminal_only_accepts_terminal_without_prepared_and_rejects_prepared() {
    for prepared in [false, true] {
        let mut offered = profile();
        let mut options = frame_options();
        offered.capabilities |= SHELL_FRAME_TERMINAL_ONLY_CAPABILITY;
        options.required_capabilities |= SHELL_FRAME_TERMINAL_ONLY_CAPABILITY;
        let (mut c, mut p) = frame_connect(offered, options, 8192).unwrap();
        let mut s = FrameStream::new();
        let a = s.open(&mut c, tx(10), key()).unwrap();
        custody(&mut c, &mut p, a.first);
        delivered(&mut c, &mut p, &mut s, opened(key())).unwrap();
        submit_one(&mut c, &mut p, &mut s);
        if prepared {
            let bytes = event(&mut p, &result(1, 1, 0));
            p.push(bytes);
            assert!(matches!(
                drive(&mut c, &mut p, |_, _| false),
                Err(ShellClientError::Protocol("Prepared in terminal-only mode"))
            ));
            assert!(!s.has_credit());
        } else {
            delivered(&mut c, &mut p, &mut s, result(1, 2, 123)).unwrap();
            assert!(s.has_credit());
            assert!(s.observe(&result(1, 2, 123)).is_err());
        }
    }
}
