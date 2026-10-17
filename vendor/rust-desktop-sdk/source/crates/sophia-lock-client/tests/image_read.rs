use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Errno, Fid, Qid, QidKind};
use sophia_lock_client::image_read::*;
fn qid() -> Qid {
    Qid {
        kind: QidKind::File,
        version: 0,
        path: 10,
    }
}
fn issue(f: &mut ImageRead, reply: Reply) -> Result<(), Error> {
    let fid = if f.operation(4096).unwrap() == Some(Operation::Walk) {
        Fid(8)
    } else {
        Fid::NOFID
    };
    f.issued(4096, fid).unwrap();
    assert!(f.operation(4096).unwrap().is_none());
    f.reply(reply)
}
fn opened(cap: usize) -> ImageRead {
    let mut f = ImageRead::new(cap).unwrap();
    issue(&mut f, Reply::Walk(vec![qid()])).unwrap();
    issue(
        &mut f,
        Reply::Lopen {
            qid: qid(),
            iounit: 7,
        },
    )
    .unwrap();
    f
}
#[test]
fn capacity_requires_eof_probe_and_successful_clunk() {
    let mut f = opened(8);
    issue(&mut f, Reply::Read(vec![1; 7])).unwrap();
    assert_eq!(
        f.operation(4096).unwrap(),
        Some(Operation::Read {
            fid: Fid(8),
            offset: 7,
            count: 1
        })
    );
    issue(&mut f, Reply::Read(vec![2])).unwrap();
    assert_eq!(
        f.operation(4096).unwrap(),
        Some(Operation::Read {
            fid: Fid(8),
            offset: 8,
            count: 1
        })
    );
    assert!(!f.done());
    issue(&mut f, Reply::Read(vec![])).unwrap();
    assert!(!f.done());
    issue(&mut f, Reply::Clunk).unwrap();
    assert_eq!(f.into_result().unwrap().unwrap(), [1, 1, 1, 1, 1, 1, 1, 2]);
    let mut f = opened(1);
    issue(&mut f, Reply::Read(vec![1])).unwrap();
    issue(&mut f, Reply::Read(vec![2])).unwrap();
    assert!(!f.done());
    issue(&mut f, Reply::Clunk).unwrap();
    assert_eq!(f.into_result().unwrap(), Err(71));
}
#[test]
fn read_errors_retain_errno_and_release_the_fid_before_result() {
    for errno in [2, 13, 116, 5] {
        let mut f = opened(16);
        issue(&mut f, Reply::Error(Errno(errno))).unwrap();
        assert_eq!(f.operation(4096).unwrap(), Some(Operation::Clunk(Fid(8))));
        assert!(!f.done());
        issue(&mut f, Reply::Clunk).unwrap();
        assert_eq!(f.into_result().unwrap(), Err(errno));
        let mut f = ImageRead::new(16).unwrap();
        issue(&mut f, Reply::Error(Errno(errno))).unwrap();
        assert_eq!(f.into_result().unwrap(), Err(errno));
    }
    let mut f = opened(16);
    issue(&mut f, Reply::Error(Errno(2))).unwrap();
    issue(&mut f, Reply::Error(Errno(5))).unwrap();
    assert_eq!(f.into_result().unwrap(), Err(5));
}
#[test]
fn malformed_replies_and_oversized_capacity_are_fatal() {
    assert!(ImageRead::new(0).is_err());
    assert!(ImageRead::new(64 + 40 * 4096 + 1).is_err());
    let mut f = opened(16);
    assert_eq!(issue(&mut f, Reply::Read(vec![0; 8])), Err(Error::Invalid));
    let mut f = ImageRead::new(16).unwrap();
    assert_eq!(issue(&mut f, Reply::Walk(vec![])), Err(Error::Invalid));
}
