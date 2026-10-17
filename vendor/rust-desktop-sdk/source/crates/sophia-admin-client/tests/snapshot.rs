use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Errno, Fid, Qid, QidKind};
use sophia_admin_client::snapshot::{Error, Fetch, File, Operation};

fn qid() -> Qid {
    Qid {
        kind: QidKind::File,
        version: 0,
        path: 8,
    }
}
fn reply(fetch: &mut Fetch, reply: Reply, now: u64) -> Result<(), Error> {
    let fid = if matches!(fetch.operation(now, 4096)?, Some(Operation::Walk(_))) {
        Fid(4)
    } else {
        Fid::NOFID
    };
    fetch.issued(now, 4096, fid)?;
    fetch.reply(reply, now)
}
fn open(fetch: &mut Fetch, iounit: u32) {
    reply(fetch, Reply::Walk(vec![qid()]), 1).unwrap();
    reply(fetch, Reply::Lopen { qid: qid(), iounit }, 1).unwrap();
}

#[test]
fn fragmented_snapshots_hidden_until_eof_and_clunk_reuse_storage() {
    for (file, bytes) in [
        (File::Info, b"v1 info 0 1 7 10000 2000 60000\n".as_slice()),
        (
            File::Commands,
            b"v1 catalog 1 1\nv1 command policy 8:next tab\n",
        ),
        (File::Status, b"v1 pending 7 9 12\n"),
    ] {
        let storage = Vec::with_capacity(file.maximum());
        let pointer = storage.as_ptr();
        let mut fetch = Fetch::new(file, storage, 0, 100).unwrap();
        open(&mut fetch, 3);
        let mut offset = 0;
        for chunk in bytes.chunks(3) {
            assert_eq!(
                fetch.operation(1, 4096),
                Ok(Some(Operation::Read {
                    fid: Fid(4),
                    offset,
                    count: 3
                }))
            );
            reply(&mut fetch, Reply::Read(chunk.to_vec()), 1).unwrap();
            assert!(fetch.result().is_none());
            offset += chunk.len() as u64;
        }
        reply(&mut fetch, Reply::Read(vec![]), 1).unwrap();
        assert!(fetch.result().is_none());
        assert_eq!(fetch.operation(1, 4096), Ok(Some(Operation::Clunk(Fid(4)))));
        reply(&mut fetch, Reply::Clunk, 1).unwrap();
        assert_eq!(fetch.result(), Some(bytes));
        assert_eq!(fetch.deadline(), None);
        assert_eq!(fetch.operation(1000, 4096), Ok(None));
        let returned = fetch.into_storage();
        assert_eq!(returned.as_ptr(), pointer);
    }
}

#[test]
fn limits_and_terminal_refusals() {
    let mut fetch = Fetch::new(File::Status, vec![], 0, 100).unwrap();
    open(&mut fetch, 0);
    assert_eq!(
        fetch.operation(1, 20),
        Ok(Some(Operation::Read {
            fid: Fid(4),
            offset: 0,
            count: 9
        }))
    );
    reply(&mut fetch, Reply::Read(vec![b'a'; 512]), 1).unwrap();
    assert_eq!(
        fetch.operation(1, 4096),
        Ok(Some(Operation::Read {
            fid: Fid(4),
            offset: 512,
            count: 1
        }))
    );
    assert_eq!(
        reply(&mut fetch, Reply::Read(vec![b'a']), 1),
        Err(Error::Invalid)
    );
    assert_eq!(fetch.operation(2, 4096), Err(Error::Invalid));
    assert!(fetch.result().is_none());
    for response in [Reply::Error(Errno(11)), Reply::Walk(vec![]), Reply::Clunk] {
        let mut fetch = Fetch::new(File::Info, vec![], 0, 100).unwrap();
        assert!(reply(&mut fetch, response, 1).is_err());
        assert!(fetch.error().is_some());
        assert!(fetch.operation(2, 4096).is_err());
    }
    let mut fetch = Fetch::new(File::Info, vec![], 0, 100).unwrap();
    open(&mut fetch, 1);
    assert_eq!(
        reply(&mut fetch, Reply::Read(vec![b'a'; 2]), 1),
        Err(Error::Invalid)
    );
}

#[test]
fn invalid_eof_clunk_failure_deadlines_and_clock() {
    let mut fetch = Fetch::new(File::Info, vec![], 0, 100).unwrap();
    open(&mut fetch, 0);
    assert_eq!(
        reply(&mut fetch, Reply::Read(vec![]), 1),
        Err(Error::Invalid)
    );
    let mut fetch = Fetch::new(File::Status, vec![], 0, 100).unwrap();
    open(&mut fetch, 0);
    reply(&mut fetch, Reply::Read(b"v1 ready 7\n".to_vec()), 1).unwrap();
    reply(&mut fetch, Reply::Read(vec![]), 1).unwrap();
    assert_eq!(
        reply(&mut fetch, Reply::Error(Errno(5)), 1),
        Err(Error::Remote(5))
    );
    assert!(fetch.result().is_none());
    let mut fetch = Fetch::new(File::Info, vec![], 10, 20).unwrap();
    assert_eq!(fetch.operation(9, 4096), Err(Error::ClockRegression));
    fetch.issued(10, 4096, Fid(4)).unwrap();
    assert_eq!(fetch.operation(10, 4096), Ok(None));
    assert_eq!(
        fetch.reply(Reply::Walk(vec![qid()]), 9),
        Err(Error::ClockRegression)
    );
    assert_eq!(fetch.error(), None);
    assert_eq!(
        fetch.reply(Reply::Walk(vec![qid()]), 20),
        Err(Error::Deadline)
    );
    assert_eq!(fetch.error(), Some(Error::Deadline));
    fetch.close();
    assert_eq!(fetch.error(), Some(Error::Deadline));
}
