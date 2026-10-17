use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Errno, Fid, Qid, QidKind};
use sophia_wm_client::{
    Candidate,
    protocol::{wm_files::*, *},
    submission::*,
};

fn progress() -> Progress {
    Progress {
        now: 10,
        pass: 1,
        consumed: 0,
        acked: 0,
    }
}
fn qid() -> Qid {
    Qid {
        kind: QidKind::File,
        version: 0,
        path: 1,
    }
}
fn candidate(domain: u64) -> Candidate {
    Candidate::SessionOperation(WmFileSessionOperation {
        transaction: TransactionId::from_raw(domain),
        request: PolicySessionOperationRequest {
            connection_epoch: 7,
            request_id: 1,
            operation: 1,
            target: None,
        },
    })
}
fn admit(s: &mut Submission, domain: u64, p: Progress) -> u64 {
    s.admit(
        &candidate(domain),
        SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS,
        p.now,
        50_000,
    )
    .unwrap()
}
fn assemble(s: &mut Submission, p: Progress) -> Vec<u8> {
    assert_eq!(s.operation(p, 4096).unwrap(), Some(Operation::Walk));
    s.issued(p, 4096, Fid(5)).unwrap();
    assert!(s.operation(p, 4096).unwrap().is_none());
    s.reply(Reply::Walk(vec![qid()]), p).unwrap();
    assert_eq!(s.operation(p, 4096).unwrap(), Some(Operation::Open(Fid(5))));
    s.issued(p, 4096, Fid::NOFID).unwrap();
    s.reply(
        Reply::Lopen {
            qid: qid(),
            iounit: 7,
        },
        p,
    )
    .unwrap();
    let mut all = vec![];
    while let Some(Operation::Write { bytes, offset, .. }) = s.operation(p, 4096).unwrap() {
        assert_eq!(offset as usize, all.len());
        assert!(bytes.len() <= 7);
        let n = bytes.len().min(3);
        all.extend(&bytes[..n]);
        s.issued(p, 4096, Fid::NOFID).unwrap();
        s.reply(Reply::Write(n as u32), p).unwrap();
    }
    all
}
fn submit(s: &mut Submission, p: Progress) -> [u8; 24] {
    let Some(Operation::Submit(bytes)) = s.operation(p, 4096).unwrap() else {
        panic!("not submit")
    };
    let bytes = *bytes;
    s.issued(p, 4096, Fid::NOFID).unwrap();
    bytes
}
fn observed(s: &mut Submission, ticket: u64, sequence: u64) {
    s.submitted(
        sequence,
        WmFileSubmitted {
            submission_id: ticket,
            candidate_kind: WmFileKind::SessionOperation,
        },
    )
    .unwrap();
}
fn clunk(s: &mut Submission, p: Progress) {
    assert_eq!(
        s.operation(p, 4096).unwrap(),
        Some(Operation::Clunk(Fid(5)))
    );
    s.issued(p, 4096, Fid::NOFID).unwrap();
    s.reply(Reply::Clunk, p).unwrap();
    assert!(s.idle());
}

#[test]
fn submission_event_wins_both_reply_orders_and_ack_blocks_next_walk() {
    for event_first in [true, false] {
        let mut s = Submission::new(7).unwrap();
        let mut p = progress();
        let id = admit(&mut s, 19, p);
        let bytes = assemble(&mut s, p);
        assert_eq!(
            decode_wm_file_session_operation(&bytes, SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS)
                .unwrap()
                .transaction
                .raw(),
            19
        );
        let submitted = submit(&mut s, p);
        assert_eq!(decode_wm_file_submit(&submitted).unwrap().submission_id, id);
        if event_first {
            observed(&mut s, id, 3);
        }
        s.reply(Reply::Write(24), p).unwrap();
        if !event_first {
            assert!(s.operation(p, 4096).unwrap().is_none());
            observed(&mut s, id, 3);
        }
        assert_eq!(s.custody(id), Some(Custody::Submitted));
        clunk(&mut s, p);
        assert_eq!(s.next_transaction(), Ok(20));
        assert_eq!(
            s.admit(
                &candidate(19),
                SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS,
                p.now,
                50_000
            ),
            Err(Error::Invalid)
        );
        admit(&mut s, 21, p);
        assert!(s.operation(p, 4096).unwrap().is_none());
        p.consumed = 3;
        assert!(s.operation(p, 4096).unwrap().is_none());
        p.acked = 3;
        assert_eq!(s.operation(p, 4096).unwrap(), Some(Operation::Walk));
    }
}

#[test]
fn eagain_reuses_identical_submit_and_requires_later_pass_and_backoff_or_progress() {
    let mut s = Submission::new(7).unwrap();
    let mut p = progress();
    let id = admit(&mut s, 19, p);
    assemble(&mut s, p);
    let original = submit(&mut s, p);
    for delay in [4, 8, 16, 32, 64, 128, 256, 256] {
        s.reply(Reply::Error(Errno(11)), p).unwrap();
        assert_eq!(s.custody(id), Some(Custody::Admitted));
        assert_eq!(s.retry_at(), Some(p.now + delay));
        assert!(s.operation(p, 4096).unwrap().is_none());
        p.pass += 1;
        assert!(s.operation(p, 4096).unwrap().is_none());
        p.now += delay;
        assert_eq!(submit(&mut s, p), original);
    }
    s.reply(Reply::Error(Errno(11)), p).unwrap();
    p.consumed += 1;
    assert!(s.operation(p, 4096).unwrap().is_none());
    p.pass += 1;
    assert_eq!(submit(&mut s, p), original);
}

#[test]
fn disconnect_distinguishes_unsent_uncertain_refused_and_submitted() {
    for mode in 0..5 {
        let mut s = Submission::new(7).unwrap();
        let p = progress();
        let id = admit(&mut s, 19, p);
        if mode > 0 {
            assemble(&mut s, p);
            submit(&mut s, p);
        }
        let expected = match mode {
            0 => Custody::DroppedUnsent,
            1 => Custody::UnknownDisconnected,
            2 => {
                s.reply(Reply::Error(Errno(11)), p).unwrap();
                Custody::DroppedUnsent
            }
            3 => {
                s.reply(Reply::Error(Errno(22)), p).unwrap();
                Custody::Refused(22)
            }
            _ => {
                observed(&mut s, id, 3);
                Custody::Submitted
            }
        };
        s.disconnect();
        s.disconnect();
        assert_eq!(s.custody(id), Some(expected));
        assert_eq!(s.next_transaction(), Err(Error::Closed));
    }
}

#[test]
fn ealready_requires_valid_submitted_and_other_refusals_cannot_undo_custody() {
    for (errno, observation, ok) in [
        (114, false, false),
        (114, true, true),
        (22, true, false),
        (116, false, false),
    ] {
        let mut s = Submission::new(7).unwrap();
        let p = progress();
        let id = admit(&mut s, 19, p);
        assemble(&mut s, p);
        submit(&mut s, p);
        if observation {
            observed(&mut s, id, 3);
        }
        assert_eq!(s.reply(Reply::Error(Errno(errno)), p).is_ok(), ok);
        s.disconnect();
        if observation {
            assert_eq!(s.custody(id), Some(Custody::Submitted));
        }
    }
}

#[test]
fn refused_domain_does_not_advance_watermark_and_history_retains_last64() {
    let mut s = Submission::new(7).unwrap();
    let p = progress();
    for id in 1..=65 {
        assert_eq!(admit(&mut s, 19, p), id);
        assemble(&mut s, p);
        submit(&mut s, p);
        s.reply(Reply::Error(Errno(22)), p).unwrap();
        clunk(&mut s, p);
    }
    assert_eq!(s.next_transaction(), Ok(1));
    assert_eq!(s.custody(1), None);
    for id in 2..=65 {
        assert_eq!(s.custody(id), Some(Custody::Refused(22)));
    }
}

#[test]
fn original_deadline_and_assembly_deadline_bound_retries() {
    let mut s = Submission::new(7).unwrap();
    let mut p = progress();
    admit(&mut s, 19, p);
    assemble(&mut s, p);
    submit(&mut s, p);
    s.reply(Reply::Error(Errno(11)), p).unwrap();
    assert_eq!(s.deadline(), Some(12_010));
    p.now = 12_010;
    p.pass += 1;
    assert_eq!(s.operation(p, 4096), Err(Error::Deadline));
    let mut s = Submission::new(7).unwrap();
    s.admit(
        &candidate(19),
        SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS,
        10,
        20,
    )
    .unwrap();
    assert_eq!(
        s.operation(
            Progress {
                now: 20,
                ..progress()
            },
            4096
        ),
        Err(Error::Deadline)
    );
}

#[test]
fn submitted_rejects_unissued_wrong_ticket_wrong_kind_and_duplicate() {
    let mut s = Submission::new(7).unwrap();
    let p = progress();
    let id = admit(&mut s, 19, p);
    let value = WmFileSubmitted {
        submission_id: id,
        candidate_kind: WmFileKind::SessionOperation,
    };
    assert_eq!(s.submitted(3, value), Err(Error::Invalid));
    assemble(&mut s, p);
    submit(&mut s, p);
    assert_eq!(
        s.submitted(
            3,
            WmFileSubmitted {
                submission_id: 8,
                ..value
            }
        ),
        Err(Error::Invalid)
    );
    assert_eq!(
        s.submitted(
            3,
            WmFileSubmitted {
                candidate_kind: WmFileKind::Dirty,
                ..value
            }
        ),
        Err(Error::Invalid)
    );
    s.submitted(3, value).unwrap();
    assert_eq!(s.submitted(4, value), Err(Error::Invalid));
}

#[test]
fn journal_control_establishes_custody_before_a_later_malformed_record() {
    use sophia_wm_client::journal::{Control, Error as JournalError, Journal};
    let mut s = Submission::new(7).unwrap();
    let p = progress();
    let id = admit(&mut s, 19, p);
    assemble(&mut s, p);
    submit(&mut s, p);
    let mut j = Journal::new(
        7,
        WmFileLimits {
            capability_ceiling: 0,
            profile_required: false,
        },
        WmFileNegotiationOffer {
            required_capabilities: 0,
            optional_capabilities: 0,
        },
        0,
    )
    .unwrap();
    let mut bytes = 48u32.to_le_bytes().to_vec();
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(17u16.to_le_bytes());
    bytes.extend(7u64.to_le_bytes());
    bytes.extend(0u64.to_le_bytes());
    bytes.extend(1u64.to_le_bytes());
    bytes.extend(id.to_le_bytes());
    bytes.extend(263u16.to_le_bytes());
    bytes.extend([0; 6]);
    bytes.extend([0; 4]);
    let result = j.append(&bytes, p.now, |header, control| {
        if let Control::Submitted(value) = control {
            s.submitted(header.sequence, value)
                .map_err(|_| JournalError::Invalid)?;
        }
        Ok(())
    });
    assert_eq!(result, Err(JournalError::Invalid));
    s.disconnect();
    assert_eq!(s.custody(id), Some(Custody::Submitted));
}

#[test]
fn negotiation_is_an_immutable_candidate_and_refusal_is_terminal() {
    let mut s = Submission::new(7).unwrap();
    let p = progress();
    let offer = WmFileNegotiationOffer {
        required_capabilities: 0,
        optional_capabilities: 3,
    };
    let id = s.negotiate(offer, p.now, 1000).unwrap();
    let bytes = assemble(&mut s, p);
    assert_eq!(decode_wm_file_negotiate(&bytes).unwrap(), offer);
    assert_eq!(s.negotiate(offer, p.now, 1000), Err(Error::Busy));
    submit(&mut s, p);
    assert_eq!(s.reply(Reply::Error(Errno(22)), p), Err(Error::Invalid));
    s.disconnect();
    assert_eq!(s.custody(id), Some(Custody::Refused(22)));
}
