use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Errno, Fid, Qid, QidKind};
use sophia_output_client::{
    protocol::{output_files::*, *},
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
fn proposal() -> OutputV1Proposal {
    OutputV1Proposal {
        connection_epoch: 7,
        candidate: OutputTopologyCandidate {
            base_topology_epoch: 7,
            intent: OutputTopologyIntent::Apply,
            primary_group_index: 0,
            heads: vec![OutputHeadTargetProposal {
                head: DisplayHeadId::from_raw(3),
                head_generation: 4,
                mode: DisplayModeId::from_raw(5),
                transform: OutputTransform::Normal,
                vrr: OutputVrrPolicy::Automatic,
            }],
            groups: vec![OutputLogicalGroupProposal {
                output: OutputId::INVALID,
                logical: Rect {
                    x: -2,
                    y: 0,
                    width: 800,
                    height: 600,
                },
                members: vec![OutputGroupMember {
                    head: DisplayHeadId::from_raw(3),
                    mapping: OutputHeadMapping::Exact,
                }],
            }],
        },
    }
}

fn admit(s: &mut Submission, domain: u64, p: Progress) -> u64 {
    s.admit(
        TransactionId::from_raw(domain),
        &proposal(),
        SOPHIA_OUTPUT_CAPABILITY_CONFIGURE,
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
        OutputFileSubmitted {
            submission_id: ticket,
            candidate_kind: OutputFileKind::Proposal,
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
        let mut s = Submission::new(7, OutputFileLimits::default()).unwrap();
        let mut p = progress();
        let id = admit(&mut s, 19, p);
        let bytes = assemble(&mut s, p);
        assert_eq!(&bytes[32..40], &19u64.to_le_bytes());
        let submitted = submit(&mut s, p);
        assert_eq!(
            decode_output_file_submit(&submitted).unwrap().submission_id,
            id
        );
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
        // Unlike WM, output domain IDs may arrive out of order.
        admit(&mut s, 3, p);
        assert!(s.operation(p, 4096).unwrap().is_none());
        p.consumed = 3;
        assert!(s.operation(p, 4096).unwrap().is_none());
        p.acked = 3;
        assert_eq!(s.operation(p, 4096).unwrap(), Some(Operation::Walk));
    }
}

#[test]
fn eagain_reuses_identical_submit_and_requires_later_pass_and_backoff_or_progress() {
    let mut s = Submission::new(7, OutputFileLimits::default()).unwrap();
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
        let mut s = Submission::new(7, OutputFileLimits::default()).unwrap();
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
fn output_refusals_have_no_wm_ealready_exception() {
    for (errno, observation, ok) in [
        (114, false, true),
        (114, true, false),
        (22, true, false),
        (116, false, false),
    ] {
        let mut s = Submission::new(7, OutputFileLimits::default()).unwrap();
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
fn refused_domain_does_not_advance_allocator_and_history_retains_last64() {
    let mut s = Submission::new(7, OutputFileLimits::default()).unwrap();
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
    let mut s = Submission::new(7, OutputFileLimits::default()).unwrap();
    let mut p = progress();
    admit(&mut s, 19, p);
    assemble(&mut s, p);
    submit(&mut s, p);
    s.reply(Reply::Error(Errno(11)), p).unwrap();
    assert_eq!(s.deadline(), Some(12_010));
    p.now = 12_010;
    p.pass += 1;
    assert_eq!(s.operation(p, 4096), Err(Error::Deadline));
    let mut s = Submission::new(7, OutputFileLimits::default()).unwrap();
    s.admit(
        TransactionId::from_raw(19),
        &proposal(),
        SOPHIA_OUTPUT_CAPABILITY_CONFIGURE,
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
    let mut s = Submission::new(7, OutputFileLimits::default()).unwrap();
    let p = progress();
    let id = admit(&mut s, 19, p);
    let value = OutputFileSubmitted {
        submission_id: id,
        candidate_kind: OutputFileKind::Proposal,
    };
    assert_eq!(s.submitted(3, value), Err(Error::Invalid));
    assemble(&mut s, p);
    submit(&mut s, p);
    assert_eq!(
        s.submitted(
            3,
            OutputFileSubmitted {
                submission_id: 8,
                ..value
            }
        ),
        Err(Error::Invalid)
    );
    assert_eq!(
        s.submitted(
            3,
            OutputFileSubmitted {
                candidate_kind: OutputFileKind::Negotiate,
                ..value
            }
        ),
        Err(Error::Invalid)
    );
    s.submitted(3, value).unwrap();
    assert_eq!(s.submitted(4, value), Err(Error::Invalid));
}

#[test]
fn grant_epoch_and_advertised_assembly_deadline_are_enforced_without_spending_ticket() {
    let limits = OutputFileLimits {
        assembly_timeout_millis: 17,
        ..OutputFileLimits::default()
    };
    let mut s = Submission::new(7, limits).unwrap();
    let mut p = progress();
    let mut value = proposal();
    assert_eq!(
        s.admit(TransactionId::from_raw(19), &value, 1, p.now, 1000),
        Err(Error::Invalid)
    );
    value.connection_epoch = 8;
    assert_eq!(
        s.admit(TransactionId::from_raw(19), &value, 3, p.now, 1000),
        Err(Error::Invalid)
    );
    value.connection_epoch = 7;
    assert_eq!(
        s.admit(TransactionId::from_raw(19), &value, 3, p.now, 1000),
        Ok(1)
    );
    value.candidate.groups[0].logical.x = 999;
    let bytes = assemble(&mut s, p);
    // Source mutation cannot change the SDK's owned bytes; syntactically
    // valid negative geometry remains for the topology owner's decision.
    assert_eq!(&bytes[96..100], &(-2i32).to_le_bytes());
    assert_eq!(s.deadline(), Some(27));
    p.now = 27;
    assert_eq!(s.operation(p, 4096), Err(Error::Deadline));
}

#[test]
fn negotiation_is_an_immutable_candidate_and_refusal_is_terminal() {
    let mut s = Submission::new(7, OutputFileLimits::default()).unwrap();
    let p = progress();
    let offer = OutputV1ClientHello {
        minimum_revision: 1,
        maximum_revision: 1,
        capabilities: 3,
    };
    let id = s.negotiate(offer, p.now, 1000).unwrap();
    let bytes = assemble(&mut s, p);
    assert_eq!(
        decode_output_file_negotiate(
            decode_output_file_record(&bytes, OutputFileClass::Candidate)
                .unwrap()
                .body
        )
        .unwrap(),
        offer
    );
    assert_eq!(s.negotiate(offer, p.now, 1000), Err(Error::Busy));
    submit(&mut s, p);
    assert_eq!(s.reply(Reply::Error(Errno(22)), p), Err(Error::Invalid));
    s.disconnect();
    assert_eq!(s.custody(id), Some(Custody::Refused(22)));
}
