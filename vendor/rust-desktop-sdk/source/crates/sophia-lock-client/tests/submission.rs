use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Errno, Fid, Qid, QidKind};
use sophia_lock_client::{Candidate, protocol::lock_files::*, submission::*};
fn candidate() -> Candidate {
    Candidate::FrameDemand(LockFrameDemand {
        transaction: 4,
        lock_epoch: 5,
        allocation_id: 6,
        allocation_generation: 7,
        demand_id: 8,
    })
}
fn qid() -> Qid {
    Qid {
        kind: QidKind::File,
        version: 0,
        path: 1,
    }
}
fn prepared() -> (Submission, u64) {
    let mut s = Submission::new(9, Fid(3), 7).unwrap();
    let id = s.admit(&candidate()).unwrap();
    while let Some(Operation::Write { bytes, .. }) = s.operation(0, 4096).unwrap() {
        let n = bytes.len() as u32;
        s.issued(0, 4096, Fid::NOFID).unwrap();
        s.reply(Reply::Write(n), true).unwrap();
    }
    (s, id)
}
fn submit(s: &mut Submission) -> Vec<u8> {
    let Some(Operation::Submit(bytes)) = s.operation(0, 4096).unwrap() else {
        panic!()
    };
    let bytes = bytes.to_vec();
    s.issued(0, 4096, Fid::NOFID).unwrap();
    bytes
}
fn observed(s: &mut Submission, id: u64) {
    s.submitted(
        11,
        LockSubmitted {
            submission_id: id,
            candidate_kind: LockFileKind::FrameDemand,
        },
    )
    .unwrap();
}
#[test]
fn custody_wins_both_reply_orders_and_reopen_waits_for_ack() {
    for first in [false, true] {
        let (mut s, id) = prepared();
        submit(&mut s);
        if first {
            observed(&mut s, id);
        }
        s.reply(Reply::Write(24), true).unwrap();
        if !first {
            observed(&mut s, id);
        }
        assert_eq!(s.latest(), Some((id, Custody::Submitted)));
        assert_eq!(s.admit(&candidate()).err(), Some(Error::Busy));
        s.issued(0, 4096, Fid::NOFID).unwrap();
        s.reply(Reply::Clunk, true).unwrap();
        s.issued(0, 4096, Fid(4)).unwrap();
        s.reply(Reply::Walk(vec![qid()]), true).unwrap();
        assert!(s.waiting_for_ack(10));
        assert_eq!(s.operation(10, 4096).unwrap(), None);
        s.issued(11, 4096, Fid::NOFID).unwrap();
        s.reply(
            Reply::Lopen {
                qid: qid(),
                iounit: 0,
            },
            true,
        )
        .unwrap();
        assert!(s.idle());
        assert_eq!(s.admit(&candidate()).unwrap(), 2);
    }
}
#[test]
fn explicit_retry_keeps_control_bytes_and_custody() {
    let (mut s, id) = prepared();
    let original = submit(&mut s);
    s.reply(Reply::Error(Errno(11)), true).unwrap();
    assert!(s.waiting_for_retry());
    assert_eq!(s.operation(0, 4096).unwrap(), None);
    assert_eq!(s.latest(), Some((id, Custody::Staged)));
    s.retry().unwrap();
    assert!(s.retry().is_err());
    assert_eq!(submit(&mut s), original);
    observed(&mut s, id);
    s.reply(Reply::Error(Errno(114)), true).unwrap();
    assert_eq!(s.latest(), Some((id, Custody::Submitted)));
}
#[test]
fn refusals_and_disconnect_preserve_latest_result() {
    for (errno, expected) in [
        (22, Custody::Refused(22)),
        (114, Custody::UnknownDisconnected),
        (116, Custody::UnknownDisconnected),
    ] {
        let (mut s, id) = prepared();
        submit(&mut s);
        let result = s.reply(Reply::Error(Errno(errno)), true);
        assert_eq!(result.is_ok(), errno == 22);
        s.disconnect();
        assert_eq!(s.latest(), Some((id, expected)));
    }
    let (mut s, id) = prepared();
    s.disconnect();
    assert_eq!(s.latest(), Some((id, Custody::DroppedUnsent)));
    let (mut s, id) = prepared();
    submit(&mut s);
    observed(&mut s, id);
    s.disconnect();
    assert_eq!(s.latest(), Some((id, Custody::Submitted)));
}
#[test]
fn wrong_controls_and_oversized_write_replies_are_rejected() {
    let (mut s, id) = prepared();
    assert!(
        s.submitted(
            1,
            LockSubmitted {
                submission_id: id,
                candidate_kind: LockFileKind::FrameDemand
            }
        )
        .is_err()
    );
    submit(&mut s);
    assert!(
        s.submitted(
            1,
            LockSubmitted {
                submission_id: id + 1,
                candidate_kind: LockFileKind::FrameDemand
            }
        )
        .is_err()
    );
    observed(&mut s, id);
    assert!(s.reply(Reply::Error(Errno(11)), true).is_err());
    s.disconnect();
    assert_eq!(s.latest(), Some((id, Custody::Submitted)));
    let mut s = Submission::new(9, Fid(3), 7).unwrap();
    s.admit(&candidate()).unwrap();
    s.issued(0, 4096, Fid::NOFID).unwrap();
    assert_eq!(s.reply(Reply::Write(8), true), Err(Error::Invalid));
}
