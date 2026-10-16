//! Count retries separately from the events that require progress. The counts
//! are diagnostic output, not a contract requiring redundant work to remain.
mod support;

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use sophia_9p::unix::Server;
use sophia_9p::{Epoch, Limits};
use support::*;

fn reply(client: &mut UnixStream) -> Frame {
    let mut size = [0; 4];
    client.read_exact(&mut size).unwrap();
    let mut bytes = vec![0; u32::from_le_bytes(size) as usize];
    bytes[..4].copy_from_slice(&size);
    client.read_exact(&mut bytes[4..]).unwrap();
    frames(&bytes).remove(0)
}

fn turn(server: &mut Server<StaticExport>) {
    // Exercise cancellation, delivery guards and real socket backpressure
    // through the owner's readiness handoff as well as the shared core.
    if let Some(subscription) = server.subscription() {
        let mut fds = server.poll_fds();
        rustix::event::poll(&mut fds, Some(&rustix::event::Timespec::default())).unwrap();
        let observed = subscription.observe(&fds).unwrap();
        drop(fds);
        assert!(server.turn_observed(observed).unwrap());
    } else {
        assert!(server.turn(Some(Duration::ZERO)).unwrap());
    }
}

fn opened() -> (Server<StaticExport>, UnixStream, UnixStream) {
    let (mut client, socket) = UnixStream::pair().unwrap();
    let fill = socket.try_clone().unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut server = Server::new(StaticExport::new(), Limits::default()).unwrap();
    server.adopt(socket).map_err(|e| e.error).unwrap();
    for request in [
        tversion(NOTAG, 8192, b"9P2000.L"),
        tattach(1, 0, NOFID, b"", b""),
        twalk(2, 0, 1, &[b"events"]),
        tlopen(3, 1, 0),
    ] {
        client.write_all(&request).unwrap();
        turn(&mut server);
        reply(&mut client);
    }
    (server, client, fill)
}

#[test]
fn unrelated_replies_do_not_consume_a_pending_event() {
    let (mut server, mut client, _) = opened();
    client.write_all(&tread(4, 1, 0, 64)).unwrap();
    turn(&mut server);
    let before = server.export().state().reads;
    for tag in 10..110 {
        client.write_all(&tgetattr(tag, 0)).unwrap();
        turn(&mut server);
        assert_eq!(reply(&mut client).tag, tag);
    }
    let retries = server.export().state().reads - before;
    println!("unrelated_replies=100 pending_read_retries={retries}");
    server.export().push_event(b"published");
    server.wake().wake();
    server.flush().unwrap();
    let event = reply(&mut client);
    assert_eq!(event.tag, 4);
    assert_eq!(event.data(), b"published");
    assert!(server.export().state().events.is_empty());
}

#[test]
fn revocation_retries_check_authority_before_consuming_data() {
    let (mut server, mut client, _) = opened();
    client.write_all(&tread(4, 1, 0, 64)).unwrap();
    turn(&mut server);
    let before = server.export().state().reads;
    server.export().push_event(b"must remain private");
    server.export().revoke(Epoch(1));
    server.wake().wake();
    server.flush().unwrap();
    let denied = reply(&mut client);
    assert_eq!(denied.tag, 4);
    assert_eq!(denied.errno(), Some(ESTALE));
    assert_eq!(server.export().state().reads, before);
    assert_eq!(server.export().state().events.len(), 1);
    drop(client);
    turn(&mut server);
    assert_eq!(server.connection_count(), 0);
}

#[test]
fn freeing_reply_capacity_retries_without_another_publication() {
    // The socket is not the only backpressure boundary: the core must reserve
    // a whole reply before calling an export that consumes its event.
    let mut h = Harness::with(
        StaticExport::new(),
        sophia_9p::ConnectionId(1),
        Limits::new(8192, 8192, 16, 16, 8192, 1).unwrap(),
    );
    h.versioned(8192);
    h.attach(0);
    h.open(1, &[b"events"], 0);
    h.open(2, &[b"info"], 0);
    assert!(h.send(&tread(4, 1, 0, 8192 - 11)).unwrap().is_empty());
    h.connection
        .receive(&mut h.export, &tread(5, 2, 0, 4000))
        .unwrap();
    h.export.push_event(b"capacity");
    let before = h.export.state().reads;
    h.connection.retry_waiting(&mut h.export).unwrap();
    assert_eq!(h.export.state().reads, before);
    assert_eq!(h.export.state().events.len(), 1);
    h.connection.sent(1);
    h.connection.retry_waiting(&mut h.export).unwrap();
    assert_eq!(h.export.state().reads, before);
    let remaining = h.connection.output().len();
    h.connection.sent(remaining);
    h.connection.resume(&mut h.export).unwrap();
    h.connection.retry_waiting(&mut h.export).unwrap();
    let replies = h.take();
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].tag, 4);
    assert_eq!(replies[0].data(), b"capacity");
    assert_eq!(h.export.state().reads, before + 1);
}

#[test]
fn a_request_can_publish_to_a_waiting_read_without_a_wake() {
    let (mut server, mut client, _) = opened();
    server.export_mut().publish_sink_writes = true;
    for request in [twalk(5, 0, 2, &[b"sink"]), tlopen(6, 2, 1)] {
        client.write_all(&request).unwrap();
        turn(&mut server);
        reply(&mut client);
    }
    client.write_all(&tread(4, 1, 0, 64)).unwrap();
    turn(&mut server);
    client.write_all(&twrite(7, 2, 0, b"from request")).unwrap();
    turn(&mut server);
    assert_eq!(reply(&mut client).written(), 12);
    let event = reply(&mut client);
    assert_eq!(event.tag, 4);
    assert_eq!(event.data(), b"from request");
}

#[test]
fn socket_backpressure_preserves_a_publication_and_flush_cancels_a_waiter() {
    let (mut server, mut client, mut fill) = opened();
    client.write_all(&tread(4, 1, 0, 64)).unwrap();
    turn(&mut server);
    let mut filled = 0;
    loop {
        match fill.write(&[0xa5; 4096]) {
            Ok(n) => {
                filled += n;
                assert!(filled < 16 * 1024 * 1024, "socket did not backpressure");
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            other => panic!("unexpected fill result: {other:?}"),
        }
    }
    server.export().push_event(b"backpressured");
    server.wake().wake();
    server.flush().unwrap();
    assert!(server.export().state().events.is_empty());
    let mut padding = vec![0; filled];
    client.read_exact(&mut padding).unwrap();
    assert!(padding.iter().all(|b| *b == 0xa5));
    drop(fill);
    turn(&mut server);
    let event = reply(&mut client);
    assert_eq!(event.tag, 4);
    assert_eq!(event.data(), b"backpressured");

    client.write_all(&tread(8, 1, 0, 64)).unwrap();
    turn(&mut server);
    client.write_all(&tflush(9, 8)).unwrap();
    turn(&mut server);
    assert_eq!(reply(&mut client).tag, 9);
    server.export().push_event(b"after cancellation");
    server.wake().wake();
    server.flush().unwrap();
    client.set_nonblocking(true).unwrap();
    assert_eq!(
        client.read(&mut [0; 1]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(server.export().state().events.len(), 1);
}
