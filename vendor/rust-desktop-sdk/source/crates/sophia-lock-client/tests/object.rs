use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Errno, Fid, Qid, QidKind};
use sophia_lock_client::{object::*, protocol::lock_files::*};

fn fetch() -> Fetch {
    Fetch::new(
        7,
        LockObjectPublished {
            object_generation: 2,
            qid_path: 90,
        },
    )
    .unwrap()
}
fn qid(path: u64) -> Qid {
    Qid {
        kind: QidKind::File,
        version: 0,
        path,
    }
}
fn walk(f: &mut Fetch, path: u64) -> Result<(), Error> {
    f.issued(4096, Fid(8)).unwrap();
    f.reply(Reply::Walk(vec![qid(path)]))
}
fn open(f: &mut Fetch, path: u64) -> Result<(), Error> {
    f.issued(4096, Fid::NOFID).unwrap();
    f.reply(Reply::Lopen {
        qid: qid(path),
        iounit: 7,
    })
}
fn clunk(f: &mut Fetch) {
    assert_eq!(f.operation(4096).unwrap(), Some(Operation::Clunk(Fid(8))));
    f.issued(4096, Fid::NOFID).unwrap();
    f.reply(Reply::Clunk).unwrap();
}
fn read(f: &mut Fetch, bytes: &[u8]) -> Result<(), Error> {
    f.issued(4096, Fid::NOFID).unwrap();
    f.reply(Reply::Read(bytes.to_vec()))
}
fn object() -> Vec<u8> {
    // Literal unlocked Lock object: epoch 7, topology generation 3.
    let mut b = vec![0; 64];
    b[..4].copy_from_slice(&64u32.to_le_bytes());
    b[4..6].copy_from_slice(&1u16.to_le_bytes());
    b[6..8].copy_from_slice(&2u16.to_le_bytes());
    b[8..16].copy_from_slice(&7u64.to_le_bytes());
    b[40..48].copy_from_slice(&3u64.to_le_bytes());
    b[48] = 1;
    b
}

#[test]
fn fragments_exact_eof_and_clunk_precede_ownership_transfer() {
    let mut f = fetch();
    walk(&mut f, 90).unwrap();
    open(&mut f, 90).unwrap();
    for (i, part) in object().chunks(7).enumerate() {
        assert!(
            matches!(f.operation(4096).unwrap(), Some(Operation::Read { offset, count: 7, .. }) if offset == (i*7) as u64)
        );
        read(&mut f, part).unwrap();
        assert!(f.take_result().is_none());
    }
    assert!(matches!(
        f.operation(4096).unwrap(),
        Some(Operation::Read {
            offset: 64,
            count: 1,
            ..
        })
    ));
    read(&mut f, &[]).unwrap();
    assert!(!f.is_ready());
    assert!(f.take_result().is_none());
    clunk(&mut f);
    assert!(f.is_ready() && !f.superseded());
    let value = f.take_result().unwrap();
    assert_eq!(value.phase, LockPhase::Unlocked);
    assert_eq!(value.topology_generation, 3);
    assert!(f.take_result().is_none());
    assert!(f.operation(4096).unwrap().is_none());
}

#[test]
fn newer_walk_or_open_is_released_without_reading() {
    for at_open in [false, true] {
        let mut f = fetch();
        walk(&mut f, if at_open { 90 } else { 91 }).unwrap();
        if at_open {
            open(&mut f, 91).unwrap();
        }
        assert!(!f.superseded());
        clunk(&mut f);
        assert!(f.is_ready() && f.superseded());
        assert!(f.take_result().is_none());
    }
    let mut f = fetch();
    assert_eq!(walk(&mut f, 89), Err(Error::Invalid));
    let mut f = fetch();
    walk(&mut f, 90).unwrap();
    assert_eq!(open(&mut f, 89), Err(Error::Invalid));
}

#[test]
fn premature_eof_extra_bytes_wrong_epoch_and_oversized_reply_fail() {
    for mode in 0..4 {
        let mut f = fetch();
        walk(&mut f, 90).unwrap();
        open(&mut f, 90).unwrap();
        if mode == 0 {
            assert_eq!(read(&mut f, &[]), Err(Error::Invalid));
            continue;
        }
        if mode == 1 {
            assert_eq!(read(&mut f, &[0; 8]), Err(Error::Invalid));
            continue;
        }
        let mut b = object();
        if mode == 2 {
            b[8] = 8;
        }
        for part in b.chunks(7) {
            read(&mut f, part).unwrap();
        }
        assert_eq!(
            read(&mut f, if mode == 3 { &[0] } else { &[] }),
            Err(Error::Invalid)
        );
        assert!(f.take_result().is_none());
    }
}

#[test]
fn remote_error_is_not_retried_and_unissued_reply_is_invalid() {
    let mut f = fetch();
    assert_eq!(f.reply(Reply::Clunk), Err(Error::Invalid));
    f.issued(4096, Fid(8)).unwrap();
    assert_eq!(f.operation(4096).unwrap(), None);
    assert_eq!(f.reply(Reply::Error(Errno(11))), Err(Error::Remote(11)));
    assert!(!f.is_ready());
}
