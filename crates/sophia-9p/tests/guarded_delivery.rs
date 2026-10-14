//! Queued replies retain authority through backpressure and partial writes.
mod support;
use sophia_9p::{Epoch, Errno};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use support::*;

#[test]
fn revoked_reply_cannot_resume_after_real_socket_backpressure() {
    for partial in [false, true] {
        let mut h = Harness::attached();
        h.export.guarded = true;
        h.open(1, &[b"info"], 0);
        h.connection
            .receive(&mut h.export, &tread(9, 1, 0, 4000))
            .unwrap();
        let (mut tx, mut rx) = UnixStream::pair().unwrap();
        tx.set_nonblocking(true).unwrap();
        rx.set_nonblocking(true).unwrap();
        if partial {
            let count = h
                .connection
                .write_output(&mut h.export, |bytes| tx.write(&bytes[..3]))
                .unwrap()
                .unwrap();
            assert_eq!(count, 3);
            h.connection.sent(count);
            let mut header = [0; 3];
            rx.read_exact(&mut header).unwrap();
        }
        // Fill the actual kernel queue, then prove this reply hit WouldBlock.
        let padding = [0xa5; 4096];
        let mut filled = 0;
        loop {
            match tx.write(&padding) {
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                result => panic!("unexpected fill result: {result:?}"),
            }
        }
        assert_eq!(
            h.connection
                .write_output(&mut h.export, |bytes| tx.write(bytes))
                .unwrap()
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
        // Clunk and reuse the fid before revocation: delivery still belongs to
        // the old read, not the new fid's state or the replies behind it.
        h.connection.receive(&mut h.export, &tclunk(10, 1)).unwrap();
        h.connection
            .receive(&mut h.export, &twalk(11, 0, 1, &[b"info"]))
            .unwrap();
        h.export.revoke(Epoch(1));
        let mut drain = vec![0; filled];
        rx.read_exact(&mut drain).unwrap();
        assert!(drain.iter().all(|b| *b == 0xa5));
        assert_eq!(
            h.connection
                .write_output(&mut h.export, |_| panic!("revoked write reached socket")),
            Err::<(), _>(Errno::ESTALE)
        );
        h.connection.close(&mut h.export);
        drop(tx);
        assert_eq!(rx.read(&mut [0; 1]).unwrap(), 0);
        assert!(h.connection.output().is_empty());
    }
}

#[test]
fn unguarded_prefix_cannot_carry_a_guarded_reply_past_its_check() {
    let mut h = Harness::attached();
    h.export.guarded = true;
    h.open(1, &[b"info"], 0);
    h.connection
        .receive(&mut h.export, &tgetattr(8, 1))
        .unwrap();
    h.connection
        .receive(&mut h.export, &tread(9, 1, 0, 4))
        .unwrap();
    h.export.revoke(Epoch(1));
    let bytes = h
        .connection
        .write_output(&mut h.export, |bytes| bytes.to_vec())
        .unwrap();
    assert_eq!(frames(&bytes).len(), 1);
    assert_eq!(frames(&bytes)[0].tag, 8);
    h.connection.sent(bytes.len());
    assert_eq!(
        h.connection.write_output(&mut h.export, |_| ()),
        Err(Errno::ESTALE)
    );
}
