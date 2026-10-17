use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Errno, Fid, Qid, QidKind};
use sophia_portal_client::snapshot::{Error, Fetch, File, Operation};

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
fn fragmented_snapshots_hidden_until_eof_and_clunk() {
    for (file, bytes) in [
        (
            File::Api,
            b"sophia-portal-client version=1 epoch=1 request=unavailable fd_transfer=none\n"
                .as_slice(),
        ),
        (
            File::CaptureStatus,
            &[
                1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0,
            ],
        ),
        (File::Status, b"request=unavailable transfer=ready\n"),
    ] {
        let mut fetch = Fetch::new(file, 0, 100).unwrap();
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
    }
}

#[test]
fn limits_and_terminal_refusals() {
    let mut fetch = Fetch::new(File::Status, 0, 100).unwrap();
    open(&mut fetch, 0);
    assert_eq!(
        fetch.operation(1, 20),
        Ok(Some(Operation::Read {
            fid: Fid(4),
            offset: 0,
            count: 9
        }))
    );
    reply(&mut fetch, Reply::Read(vec![b'a'; 43]), 1).unwrap();
    assert_eq!(
        fetch.operation(1, 4096),
        Ok(Some(Operation::Read {
            fid: Fid(4),
            offset: 43,
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
        let mut fetch = Fetch::new(File::Api, 0, 100).unwrap();
        assert!(reply(&mut fetch, response, 1).is_err());
        assert!(fetch.error().is_some());
        assert!(fetch.operation(2, 4096).is_err());
    }
    let mut fetch = Fetch::new(File::Api, 0, 100).unwrap();
    open(&mut fetch, 1);
    assert_eq!(
        reply(&mut fetch, Reply::Read(vec![b'a'; 2]), 1),
        Err(Error::Invalid)
    );
}

#[test]
fn invalid_eof_clunk_failure_deadlines_and_clock() {
    let mut fetch = Fetch::new(File::Api, 0, 100).unwrap();
    open(&mut fetch, 0);
    assert_eq!(
        reply(&mut fetch, Reply::Read(vec![]), 1),
        Err(Error::Invalid)
    );
    let mut fetch = Fetch::new(File::Status, 0, 100).unwrap();
    open(&mut fetch, 0);
    reply(
        &mut fetch,
        Reply::Read(b"request=unavailable transfer=ready\n".to_vec()),
        1,
    )
    .unwrap();
    reply(&mut fetch, Reply::Read(vec![]), 1).unwrap();
    assert_eq!(
        reply(&mut fetch, Reply::Error(Errno(5)), 1),
        Err(Error::Remote(5))
    );
    assert!(fetch.result().is_none());
    let mut fetch = Fetch::new(File::Api, 10, 20).unwrap();
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

#[test]
fn opaque_transfer_reuses_caller_storage_and_requires_eof_and_clunk() {
    use sophia_portal_client::snapshot::TRANSFER_MAX_BYTES;
    for length in [0, 1, TRANSFER_MAX_BYTES] {
        let mut storage = vec![0xa5; TRANSFER_MAX_BYTES + 8];
        let pointer = storage.as_ptr();
        let payload: Vec<u8> = (0..length).map(|i| i as u8).collect();
        let mut fetch = Fetch::transfer(&mut storage, 0, 100).unwrap();
        assert_eq!(
            fetch.operation(1, 4096),
            Ok(Some(Operation::Walk(File::Transfer)))
        );
        open(&mut fetch, 33);
        for (index, chunk) in payload.chunks(17).enumerate() {
            let Some(Operation::Read { offset, count, .. }) = fetch.operation(1, 4096).unwrap()
            else {
                panic!()
            };
            assert_eq!(offset, (index * 17) as u64);
            assert!(count as usize >= chunk.len() && count <= 33);
            reply(&mut fetch, Reply::Read(chunk.to_vec()), 1).unwrap();
            assert!(fetch.result().is_none());
        }
        if length == TRANSFER_MAX_BYTES {
            assert_eq!(
                fetch.operation(1, 4096),
                Ok(Some(Operation::Read {
                    fid: Fid(4),
                    offset: length as u64,
                    count: 1
                }))
            );
        }
        reply(&mut fetch, Reply::Read(vec![]), 1).unwrap();
        assert!(fetch.result().is_none());
        reply(&mut fetch, Reply::Clunk, 1).unwrap();
        let result = fetch.result().unwrap();
        assert_eq!(result.as_ptr(), pointer);
        assert_eq!(result, payload);
        assert!(storage[length..].iter().all(|b| *b == 0xa5));
    }
}

#[test]
fn transfer_overflow_stale_and_clunk_refusal_never_expose_scratch() {
    use sophia_portal_client::snapshot::TRANSFER_MAX_BYTES;
    let mut storage = vec![0; TRANSFER_MAX_BYTES];
    assert!(Fetch::new(File::Transfer, 0, 100).is_err());
    assert!(Fetch::transfer(&mut storage[..TRANSFER_MAX_BYTES - 1], 0, 100).is_err());
    for fault in 0..3 {
        let mut fetch = Fetch::transfer(&mut storage, 0, 100).unwrap();
        open(&mut fetch, 0);
        if fault == 0 {
            let mut total = 0;
            while total < TRANSFER_MAX_BYTES {
                let n = (TRANSFER_MAX_BYTES - total).min(4000);
                reply(&mut fetch, Reply::Read(vec![17; n]), 1).unwrap();
                total += n;
            }
            assert_eq!(
                reply(&mut fetch, Reply::Read(vec![1]), 1),
                Err(Error::Invalid)
            );
        } else {
            reply(&mut fetch, Reply::Read(vec![1, 2, 3]), 1).unwrap();
            if fault == 2 {
                reply(&mut fetch, Reply::Read(vec![]), 1).unwrap();
            }
            assert_eq!(
                reply(&mut fetch, Reply::Error(Errno(116)), 1),
                Err(Error::Remote(116))
            );
        }
        assert!(fetch.result().is_none());
        assert!(fetch.operation(2, 4096).is_err());
    }
}
