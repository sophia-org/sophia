use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Errno, Fid, Qid, QidKind};
use sophia_portal_client::requester::{
    Custody, Error,
    transaction::{Operation, Transaction},
};
use sophia_portal_protocol::requester::{Candidate, Event, EventRecord};
fn qid() -> Qid {
    Qid {
        kind: QidKind::File,
        version: 0,
        path: 99,
    }
}
fn new() -> Transaction {
    Transaction::new(
        7,
        9,
        Candidate::Negotiate {
            minimum: 1,
            maximum: 1,
        },
        0,
        100,
        20,
    )
    .unwrap()
}
fn receipt() -> EventRecord {
    EventRecord {
        epoch: 7,
        sequence: 1,
        event: Event::Submitted {
            submission: 9,
            candidate_kind: 256,
        },
    }
}
fn prepare(t: &mut Transaction, now: u64) {
    assert!(matches!(
        t.operation(now, 512).unwrap(),
        Some(Operation::Walk(b"transaction"))
    ));
    t.issued(now, 512, Fid(2)).unwrap();
    t.reply(Reply::Walk(vec![qid()]), now).unwrap();
    assert!(matches!(
        t.operation(now, 512).unwrap(),
        Some(Operation::Open {
            fid: Fid(2),
            flags: 2
        })
    ));
    t.issued(now, 512, Fid::NOFID).unwrap();
    t.reply(
        Reply::Lopen {
            qid: qid(),
            iounit: 7,
        },
        now,
    )
    .unwrap();
    let mut staged = Vec::new();
    while let Some(Operation::Write { fid, offset, bytes }) = t.operation(now, 512).unwrap() {
        assert_eq!(fid, Fid(2));
        assert_eq!(offset as usize, staged.len());
        assert!(bytes.len() <= 7);
        let count = bytes.len() as u32;
        staged.extend_from_slice(bytes);
        t.issued(now, 512, Fid::NOFID).unwrap();
        t.reply(Reply::Write(count), now).unwrap();
    }
    assert_eq!(staged.len(), 40);
    assert!(matches!(
        t.operation(now, 512).unwrap(),
        Some(Operation::Walk(b"submit"))
    ));
    t.issued(now, 512, Fid(3)).unwrap();
    t.reply(Reply::Walk(vec![qid()]), now).unwrap();
    t.issued(now, 512, Fid::NOFID).unwrap();
    t.reply(
        Reply::Lopen {
            qid: qid(),
            iounit: 24,
        },
        now,
    )
    .unwrap();
    assert!(
        matches!(t.operation(now, 512).unwrap(), Some(Operation::Write { fid: Fid(3), offset: 0, bytes }) if bytes.len() == 24)
    );
}
#[test]
fn stages_once_retries_only_on_request_and_releases_after_ack() {
    let mut t = new();
    prepare(&mut t, 1);
    t.issued(2, 512, Fid::NOFID).unwrap();
    t.reply(Reply::Error(Errno(11)), 3).unwrap();
    assert_eq!(t.custody(), Custody::Queued);
    assert!(t.operation(4, 512).unwrap().is_none());
    t.retry(5).unwrap();
    t.issued(5, 512, Fid::NOFID).unwrap();
    t.receipt(receipt(), 6).unwrap();
    assert_eq!(t.release(1, 7), Err(Error::Busy));
    t.reply(Reply::Write(24), 8).unwrap();
    assert_eq!(t.release(0, 9), Err(Error::Busy));
    t.release(1, 10).unwrap();
    for fid in [Fid(3), Fid(2)] {
        assert!(matches!(t.operation(11, 512).unwrap(), Some(Operation::Clunk(f)) if f == fid));
        t.issued(11, 512, Fid::NOFID).unwrap();
        t.reply(Reply::Clunk, 11).unwrap();
    }
    assert!(t.done());
    assert_eq!(t.custody(), Custody::Submitted);
}
#[test]
fn assembly_expiry_and_loss_preserve_correct_custody() {
    let mut t = new();
    prepare(&mut t, 1);
    assert!(matches!(t.operation(21, 512), Err(Error::Deadline)));
    assert_eq!(t.custody(), Custody::DroppedUnsent);
    let mut t = new();
    prepare(&mut t, 1);
    t.issued(2, 512, Fid::NOFID).unwrap();
    t.close();
    assert_eq!(t.custody(), Custody::Unknown);
    let mut t = new();
    prepare(&mut t, 1);
    t.issued(2, 512, Fid::NOFID).unwrap();
    t.receipt(receipt(), 3).unwrap();
    t.close();
    assert_eq!(t.custody(), Custody::Submitted);
}
#[test]
fn cleanup_is_bounded_and_failed_clunk_does_not_erase_receipt() {
    for fail_clunk in [false, true] {
        let mut t = new();
        prepare(&mut t, 1);
        t.issued(2, 512, Fid::NOFID).unwrap();
        t.reply(Reply::Write(24), 3).unwrap();
        t.receipt(receipt(), 4).unwrap();
        t.release(1, 5).unwrap();
        t.issued(6, 512, Fid::NOFID).unwrap();
        if fail_clunk {
            assert_eq!(t.reply(Reply::Error(Errno(5)), 7), Err(Error::Remote(5)));
        } else {
            assert!(matches!(t.operation(100, 512), Err(Error::Deadline)));
        }
        assert!(!t.done());
        assert_eq!(t.custody(), Custody::Submitted);
    }
}
