use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Errno, Fid, Qid, QidKind};
use sophia_portal_client::requester::{
    Error,
    upload::{Operation, Upload},
};
fn qid(kind: QidKind) -> Qid {
    Qid {
        kind,
        version: 0,
        path: 1,
    }
}
fn open(u: &mut Upload, unit: u32) {
    assert!(matches!(
        u.operation(1, 512).unwrap(),
        Some(Operation::Walk)
    ));
    u.issued(1, 512, Fid(4)).unwrap();
    u.reply(
        Reply::Walk(vec![qid(QidKind::Directory), qid(QidKind::File)]),
        1,
    )
    .unwrap();
    u.issued(1, 512, Fid::NOFID).unwrap();
    u.reply(
        Reply::Lopen {
            qid: qid(QidKind::File),
            iounit: unit,
        },
        1,
    )
    .unwrap();
}
#[test]
fn bounded_writes_hold_writer_until_explicit_cleanup_and_reuse_storage() {
    for size in [0, 1, 65536] {
        for unit in [0, 7, 1024] {
            let bytes = vec![17; size];
            let pointer = bytes.as_ptr();
            let mut u = Upload::new(bytes, 65536, 0, 100).unwrap();
            open(&mut u, unit);
            let mut offset = 0;
            while !u.held() {
                let Some(Operation::Write {
                    fid,
                    offset: at,
                    bytes,
                }) = u.operation(1, 512).unwrap()
                else {
                    panic!()
                };
                assert_eq!(fid, Fid(4));
                assert_eq!(at, offset as u64);
                assert!(bytes.len() <= 489 && (unit == 0 || bytes.len() <= unit as usize));
                assert_eq!(bytes.as_ptr(), pointer.wrapping_add(offset));
                assert!(bytes.iter().all(|b| *b == 17));
                let count = bytes.len();
                u.issued(1, 512, Fid::NOFID).unwrap();
                assert!(u.operation(1, 512).unwrap().is_none());
                u.reply(Reply::Write(count as u32), 1).unwrap();
                offset += count;
            }
            assert_eq!(u.written(), size);
            assert!(!u.done());
            assert!(u.operation(2, 512).unwrap().is_none());
            u.release(2).unwrap();
            assert!(matches!(
                u.operation(2, 512).unwrap(),
                Some(Operation::Clunk(Fid(4)))
            ));
            u.issued(2, 512, Fid::NOFID).unwrap();
            u.reply(Reply::Clunk, 2).unwrap();
            assert!(u.done());
            let returned = u.into_buffer();
            assert_eq!(returned.as_ptr(), pointer);
        }
    }
}
#[test]
fn short_write_stale_writer_and_deadline_are_terminal_without_replay() {
    for fault in 0..4 {
        let mut u = Upload::new(vec![1; 9], 65536, 0, 100).unwrap();
        open(&mut u, 7);
        u.issued(1, 512, Fid::NOFID).unwrap();
        let result = match fault {
            0 => u.reply(Reply::Write(6), 2),
            1 => u.reply(Reply::Error(Errno(116)), 2),
            2 => u.reply(Reply::Write(7), 100),
            _ => {
                u.close();
                u.reply(Reply::Write(7), 2)
            }
        };
        assert!(result.is_err());
        assert_eq!(u.written(), 0);
        assert!(!u.held() && !u.done());
        assert!(u.operation(100, 512).is_err());
    }
    let mut u = Upload::new(vec![], 65536, 0, 100).unwrap();
    u.issued(1, 512, Fid(4)).unwrap();
    assert_eq!(
        u.reply(Reply::Walk(vec![qid(QidKind::File)]), 1),
        Err(Error::Protocol)
    );
    let mut u = Upload::new(vec![], 65536, 0, 100).unwrap();
    open(&mut u, 0);
    u.release(2).unwrap();
    u.issued(2, 512, Fid::NOFID).unwrap();
    assert_eq!(u.reply(Reply::Error(Errno(5)), 3), Err(Error::Remote(5)));
    assert!(!u.done());
}

#[test]
fn terminal_settlement_drains_one_write_then_clunks_without_more_bytes() {
    for stale in [false, true] {
        let mut u = Upload::new(vec![17; 32], 65536, 0, 100).unwrap();
        open(&mut u, 7);
        u.issued(1, 512, Fid::NOFID).unwrap();
        u.settle(2).unwrap();
        assert!(u.operation(2, 512).unwrap().is_none());
        u.reply(
            if stale {
                Reply::Error(Errno(116))
            } else {
                Reply::Write(7)
            },
            3,
        )
        .unwrap();
        assert_eq!(u.written(), if stale { 0 } else { 7 });
        assert!(matches!(
            u.operation(3, 512).unwrap(),
            Some(Operation::Clunk(Fid(4)))
        ));
        u.issued(3, 512, Fid::NOFID).unwrap();
        u.reply(Reply::Clunk, 4).unwrap();
        assert!(u.done());
    }
    let mut u = Upload::new(vec![17; 32], 65536, 0, 100).unwrap();
    u.settle(1).unwrap();
    assert!(u.done());
    let mut u = Upload::new(vec![17; 32], 65536, 0, 100).unwrap();
    u.issued(1, 512, Fid(4)).unwrap();
    u.settle(2).unwrap();
    u.reply(Reply::Error(Errno(116)), 3).unwrap();
    assert!(u.done());
}
