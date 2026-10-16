//! Scratch reuse must not alias queued replies or weaken delivery authority.
mod support;
use sophia_9p::{Epoch, Errno, ReadOutcome};
use support::*;

fn buffered(offset: u64, count: u32, scratch: &mut Vec<u8>) -> Result<ReadOutcome, Errno> {
    match offset {
        1 => {
            scratch.extend_from_slice(b"private pending bytes");
            Ok(ReadOutcome::Pending)
        }
        2 => {
            scratch.extend_from_slice(b"private error bytes");
            Err(Errno::EINVAL)
        }
        3 => Ok(ReadOutcome::Ready(vec![0; count as usize + 1])),
        4 => {
            scratch.reserve_exact(65536);
            Ok(ReadOutcome::Pending)
        }
        _ => {
            scratch.extend_from_slice(&[offset as u8; 32]);
            Ok(ReadOutcome::Ready(std::mem::take(scratch)))
        }
    }
}

fn opened() -> Harness {
    let mut h = Harness::attached();
    h.export.guarded = true;
    h.export.buffered_read = Some(buffered);
    h.open(1, &[b"info"], 0);
    h
}

#[test]
fn reuse_preserves_queued_bytes_and_partial_delivery_guards() {
    let mut h = opened();
    h.connection
        .receive(&mut h.export, &tread(10, 1, 7, 32))
        .unwrap();
    h.connection
        .receive(&mut h.export, &tread(11, 1, 8, 32))
        .unwrap();
    let replies = frames(h.connection.output());
    assert_eq!(replies[0].data(), [7; 32]);
    assert_eq!(replies[1].data(), [8; 32]);
    assert_eq!(h.export.state().scratch_capacities, [0, 32]);
    h.connection
        .write_output(&mut h.export, |bytes| assert_eq!(bytes.len(), 43))
        .unwrap();
    h.connection.sent(3);
    h.export.revoke(Epoch(1));
    assert_eq!(
        h.connection
            .write_output(&mut h.export, |_| panic!("revoked delivery")),
        Err::<(), _>(Errno::ESTALE)
    );
}

#[test]
fn pending_error_oversize_and_reset_leave_no_stale_payload() {
    let mut h = opened();
    assert_eq!(h.one(&tread(10, 1, 7, 32)).data(), [7; 32]);
    assert_eq!(h.errno(&tread(11, 1, 2, 32)), EINVAL);
    assert_eq!(h.errno(&tread(12, 1, 3, 32)), 5); // EIO
    assert!(h.send(&tread(13, 1, 1, 32)).unwrap().is_empty());
    h.one(&tflush(14, 13));
    assert_eq!(h.one(&tread(15, 1, 8, 32)).data(), [8; 32]);
    assert!(h.send(&tread(16, 1, 4, 32)).unwrap().is_empty());
    h.one(&tflush(17, 16));
    assert_eq!(h.one(&tread(18, 1, 9, 32)).data(), [9; 32]);
    assert_eq!(*h.export.state().scratch_capacities.last().unwrap(), 0);
    h.versioned(4096);
    h.attach(0);
    h.open(1, &[b"info"], 0);
    assert_eq!(h.one(&tread(19, 1, 10, 32)).data(), [10; 32]);
    assert_eq!(*h.export.state().scratch_capacities.last().unwrap(), 0);
}
