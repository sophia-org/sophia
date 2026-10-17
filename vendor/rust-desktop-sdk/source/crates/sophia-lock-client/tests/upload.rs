use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Errno, Fid, Qid, QidKind};
use sophia_lock_client::{Candidate, protocol::lock_files::*, upload::*};

fn limits() -> LockFileLimits {
    LockFileLimits {
        max_outputs: 16,
        upload_slots: 2,
        max_chords: 8,
        max_width_px: 2560,
        max_height_px: 1440,
        max_resource_bytes: 2560 * 1440 * 4,
        max_live_resources: 32,
        journal_records: 128,
        journal_bytes: 32768,
        assembly_timeout_ms: 2000,
        ack_progress_timeout_ms: 2000,
    }
}
fn begin() -> LockResourceBegin {
    LockResourceBegin {
        transaction: 30,
        resource: LockResourceId {
            id: 21,
            generation: 2,
        },
        width_px: 16,
        height_px: 2,
        slot: 0,
    }
}
fn admit(c: &Candidate) -> Result<(), Error> {
    c.prepare(7, 1).map(|_| ()).map_err(|_| Error::Invalid)
}
fn status(u: &mut Upload, state: LockResourceState) {
    u.status(LockResourceStatus {
        transaction: 30,
        resource: begin().resource,
        status: state,
        reason: 0,
        admitted_bytes: 128,
    });
}
fn qid() -> Qid {
    Qid {
        kind: QidKind::File,
        version: 0,
        path: 3,
    }
}
fn ready(window: usize) -> Upload {
    let mut u = Upload::new(16).unwrap();
    u.set_window(window).unwrap();
    u.begin(begin(), limits(), admit).unwrap();
    status(&mut u, LockResourceState::Admitted);
    assert_eq!(u.operation(4096).unwrap(), Some(Operation::Walk(0)));
    u.issued(4096, Fid(8)).unwrap();
    u.reply(Reply::Walk(vec![qid(), qid()])).unwrap();
    u.issued(4096, Fid::NOFID).unwrap();
    u.reply(Reply::Lopen {
        qid: qid(),
        iounit: 4,
    })
    .unwrap();
    assert!(u.ready());
    u
}
fn fill(u: &mut Upload, expected: &mut u64) -> Vec<(usize, usize)> {
    let mut issued = vec![];
    while let Some(Operation::Write {
        slot,
        offset,
        bytes,
        ..
    }) = u.operation(4096).unwrap()
    {
        assert_eq!(offset, *expected);
        assert_eq!(
            bytes,
            &(offset as u8..offset as u8 + bytes.len() as u8).collect::<Vec<_>>()
        );
        *expected += bytes.len() as u64;
        issued.push((slot, bytes.len()));
        u.issued(4096, Fid::NOFID).unwrap();
    }
    issued
}
fn clunk(u: &mut Upload) {
    assert_eq!(u.operation(4096).unwrap(), Some(Operation::Clunk(Fid(8))));
    u.issued(4096, Fid::NOFID).unwrap();
    u.reply(Reply::Clunk).unwrap();
    assert!(!u.pending());
}

#[test]
fn reverse_replies_refill_while_first_is_held_without_copying_chunk() {
    let mut u = ready(8);
    let bytes: Vec<_> = (0..128).collect();
    let pointer = bytes.as_ptr();
    u.chunk(bytes).unwrap();
    let mut offset = 0;
    let held = fill(&mut u, &mut offset);
    assert_eq!(held.len(), 8);
    assert_eq!(u.end(admit), Err(Error::Busy));
    assert!(u.take_chunk().is_none());
    for &(slot, n) in held[1..].iter().rev() {
        u.write_reply(slot, Reply::Write(n as u32)).unwrap();
    }
    while offset < 128 {
        for (slot, n) in fill(&mut u, &mut offset) {
            u.write_reply(slot, Reply::Write(n as u32)).unwrap();
        }
    }
    assert!(!u.ready());
    assert!(u.take_chunk().is_none());
    u.write_reply(held[0].0, Reply::Write(held[0].1 as u32))
        .unwrap();
    assert!(u.ready());
    let bytes = u.take_chunk().unwrap();
    assert_eq!(bytes.as_ptr(), pointer);
    u.end(|c| {
        assert!(matches!(c, Candidate::ResourceEnd(v) if v.transaction == 30 && v.total_bytes == Some(128)));
        admit(c)
    }).unwrap();
    status(&mut u, LockResourceState::Accepted);
    clunk(&mut u);
}

#[test]
fn cancellation_short_write_and_errno_stop_issuance_then_drain() {
    for fault in 0..3 {
        let mut u = ready(8);
        u.chunk((0..128).collect()).unwrap();
        let issued = fill(&mut u, &mut 0);
        if fault == 0 {
            u.cancel().unwrap();
        }
        let reply = match fault {
            0 => Reply::Write(4),
            1 => Reply::Write(2),
            _ => Reply::Error(Errno(5)),
        };
        u.write_reply(issued[0].0, reply).unwrap();
        assert_eq!(u.remote_error(), (fault == 2).then_some(5));
        assert_eq!(u.operation(4096).unwrap(), None);
        assert!(u.take_chunk().is_none());
        for &(slot, n) in issued[1..].iter().rev() {
            u.write_reply(slot, Reply::Write(n as u32)).unwrap();
        }
        assert!(u.take_chunk().is_some());
        let Some(Operation::Cancel(value)) = u.operation(4096).unwrap() else {
            panic!()
        };
        assert_eq!(value.transaction, 30);
        assert_eq!(value.total_bytes, None);
        assert_eq!(u.end(admit), Err(Error::Busy));
        u.issued(4096, Fid::NOFID).unwrap();
        status(&mut u, LockResourceState::Cancelled);
        clunk(&mut u);
    }
}

#[test]
fn terminal_status_drains_without_cancel_and_disconnect_releases_storage() {
    let mut u = ready(8);
    u.chunk((0..128).collect()).unwrap();
    let issued = fill(&mut u, &mut 0);
    status(&mut u, LockResourceState::Rejected);
    assert_eq!(u.operation(4096).unwrap(), None);
    assert!(u.take_chunk().is_none());
    for (slot, n) in issued {
        u.write_reply(slot, Reply::Write(n as u32)).unwrap();
    }
    clunk(&mut u);
    assert!(u.take_chunk().is_some());
    let mut u = ready(8);
    u.chunk((0..128).collect()).unwrap();
    fill(&mut u, &mut 0);
    u.disconnect();
    assert!(!u.pending());
    assert!(u.take_chunk().is_some());
    assert_eq!(u.end(admit), Err(Error::Closed));
    assert_eq!(u.cancel(), Err(Error::Closed));
    assert_eq!(u.operation(4096), Err(Error::Closed));
}

#[test]
fn single_write_short_reply_continues_exact_tail_and_multiple_chunks() {
    let mut u = ready(1);
    u.chunk((0..64).collect()).unwrap();
    let first = fill(&mut u, &mut 0);
    assert_eq!(first.len(), 1);
    u.write_reply(first[0].0, Reply::Write(2)).unwrap();
    let mut offset = 2;
    while offset < 64 {
        for (slot, n) in fill(&mut u, &mut offset) {
            u.write_reply(slot, Reply::Write(n as u32)).unwrap();
        }
    }
    assert!(u.ready());
    assert_eq!(u.end(admit), Err(Error::Invalid));
    u.chunk((64..128).collect()).unwrap();
    while offset < 128 {
        for (slot, n) in fill(&mut u, &mut offset) {
            u.write_reply(slot, Reply::Write(n as u32)).unwrap();
        }
    }
    u.end(admit).unwrap();
    status(&mut u, LockResourceState::Accepted);
    clunk(&mut u);
}

#[test]
fn window_limits_failed_admission_and_invalid_chunks_are_nonmutating() {
    let mut u = Upload::new(8).unwrap();
    assert_eq!(u.window(), 1);
    for width in [0, 4, 8, 9] {
        assert_eq!(u.set_window(width), Err(Error::Invalid));
    }
    u.set_window(3).unwrap();
    assert_eq!(
        u.begin(begin(), limits(), |_| Err(Error::Busy)),
        Err(Error::Busy)
    );
    assert!(!u.pending());
    let mut bad = begin();
    bad.width_px = u32::MAX;
    assert_eq!(u.begin(bad, limits(), |_| panic!()), Err(Error::Invalid));
    u.begin(begin(), limits(), admit).unwrap();
    assert_eq!(u.set_window(1), Err(Error::Busy));
    u.refused(LockFileKind::ResourceBegin);
    assert_eq!(u.operation(4096).unwrap(), None);
    assert!(!u.pending());
    let mut u = ready(1);
    let bytes = vec![0; 129];
    let ptr = bytes.as_ptr();
    let (error, bytes) = u.chunk(bytes).unwrap_err();
    assert_eq!(error, Error::Invalid);
    assert_eq!(bytes.as_ptr(), ptr);
    assert!(u.ready());
}

#[test]
fn slot_errors_close_only_upload_and_early_cancel_waits_for_admission() {
    let mut u = Upload::new(8).unwrap();
    u.begin(begin(), limits(), admit).unwrap();
    u.cancel().unwrap();
    assert_eq!(u.operation(4096).unwrap(), None);
    status(&mut u, LockResourceState::Admitted);
    u.issued(4096, Fid(8)).unwrap();
    u.reply(Reply::Error(Errno(11))).unwrap();
    assert_eq!(u.operation(4096).unwrap(), None);
    assert!(!u.pending());
    assert_eq!(u.remote_error(), Some(11));
    u.begin(begin(), limits(), admit).unwrap();
    status(&mut u, LockResourceState::Admitted);
    u.issued(4096, Fid(8)).unwrap();
    u.reply(Reply::Walk(vec![qid(), qid()])).unwrap();
    u.issued(4096, Fid::NOFID).unwrap();
    u.reply(Reply::Error(Errno(5))).unwrap();
    clunk(&mut u);
}

#[test]
fn malformed_or_unissued_write_reply_is_rejected() {
    for count in [0, 5] {
        let mut u = ready(1);
        u.chunk((0..128).collect()).unwrap();
        fill(&mut u, &mut 0);
        assert_eq!(u.write_reply(0, Reply::Write(count)), Err(Error::Invalid));
    }
    let mut u = ready(1);
    assert_eq!(u.write_reply(0, Reply::Write(4)), Err(Error::Invalid));
    assert_eq!(u.write_reply(8, Reply::Write(4)), Err(Error::Invalid));
    u.chunk((0..128).collect()).unwrap();
    fill(&mut u, &mut 0);
    u.write_reply(0, Reply::Write(4)).unwrap();
    assert_eq!(u.write_reply(0, Reply::Write(4)), Err(Error::Invalid));
}
