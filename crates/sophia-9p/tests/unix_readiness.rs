//! The server's readiness as an owner borrows it for its own wait: what
//! `poll_fds` subscribes, and that the owner's next turn consumes whatever it
//! reports. The test thread is the owner; nothing here runs `Server::run`.

mod support;

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rustix::event::{Timespec, poll};
use sophia_9p::Limits;
use sophia_9p::unix::Server;
use support::*;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Generous: separates "ready" from "never ready", not one delay from another.
const PROMPT: Duration = Duration::from_secs(5);
const BUDGET: Duration = Duration::from_millis(20);

struct Socket(PathBuf);

impl Socket {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "sophia-9p-readiness-{}-{}.sock",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Whether anything the server subscribes becomes ready within `within`,
/// waited on the way an owner waits: one poll over the borrowed set.
fn ready(server: &Server<StaticExport>, within: Duration) -> bool {
    let mut fds = server.poll_fds();
    let timeout = Timespec::try_from(within).unwrap();
    poll(&mut fds, Some(&timeout)).unwrap() != 0
}

/// One owner pass: the nonblocking turn that follows the wait.
fn turn(server: &mut Server<StaticExport>) {
    assert!(server.turn(Some(Duration::ZERO)).unwrap());
}

/// Turns until nothing subscribed is ready. Bounded, because readiness a
/// turn cannot consume would end every owner wait at once.
fn settle(server: &mut Server<StaticExport>) {
    for _ in 0..256 {
        if !ready(server, Duration::ZERO) {
            return;
        }
        turn(server);
    }
    panic!("readiness never settled");
}

fn adopted(limits: Limits) -> (Server<StaticExport>, UnixStream) {
    let (ours, theirs) = UnixStream::pair().unwrap();
    ours.set_read_timeout(Some(PROMPT)).unwrap();
    let mut server = Server::new(StaticExport::new(), limits).unwrap();
    server
        .adopt(theirs)
        .map_err(|refused| refused.error)
        .unwrap();
    (server, ours)
}

fn reply(stream: &mut UnixStream) -> Frame {
    let mut size = [0; 4];
    stream.read_exact(&mut size).unwrap();
    let mut rest = vec![0; u32::from_le_bytes(size) as usize - 4];
    stream.read_exact(&mut rest).unwrap();
    let mut bytes = size.to_vec();
    bytes.extend(rest);
    frames(&bytes).remove(0)
}

fn exchange(server: &mut Server<StaticExport>, stream: &mut UnixStream, request: &[u8]) -> Frame {
    stream.write_all(request).unwrap();
    assert!(
        ready(server, PROMPT),
        "a request did not make the server ready"
    );
    settle(server);
    reply(stream)
}

#[test]
fn an_idle_connection_lets_the_wait_time_out() {
    let (server, _ours) = adopted(Limits::default());
    let started = Instant::now();
    assert!(!ready(&server, BUDGET));
    assert!(started.elapsed() >= BUDGET);
}

#[test]
fn a_request_is_ready_until_one_turn_serves_it() {
    let (mut server, mut ours) = adopted(Limits::default());
    ours.write_all(&tversion(NOTAG, 8192, b"9P2000.L")).unwrap();
    assert!(ready(&server, PROMPT));
    turn(&mut server);
    assert!(
        !ready(&server, Duration::ZERO),
        "a served request stayed ready"
    );
    assert_eq!(reply(&mut ours).version(), (8192, b"9P2000.L".to_vec()));
}

#[test]
fn an_export_wake_is_ready_until_a_turn_drains_it() {
    let (mut server, _ours) = adopted(Limits::default());
    // What an owner does after changing the export outside a request, such
    // as journaling an event a waiting read should see.
    server.wake().wake();
    assert!(ready(&server, Duration::ZERO));
    turn(&mut server);
    let started = Instant::now();
    assert!(!ready(&server, BUDGET), "a drained wake stayed ready");
    assert!(started.elapsed() >= BUDGET);
}

#[test]
fn output_the_peer_cannot_take_waits_for_its_room() {
    let (mut server, mut ours) = adopted(Limits::default());
    let version = exchange(&mut server, &mut ours, &tversion(NOTAG, 65536, b"9P2000.L"));
    assert_eq!(version.version().0, 65536);
    exchange(&mut server, &mut ours, &tattach(1, 0, NOFID, b"", b"")).attach();
    exchange(&mut server, &mut ours, &twalk(2, 0, 1, &[b"info"])).walk();
    exchange(&mut server, &mut ours, &tlopen(3, 1, 0)).lopen();
    let before = server.export().state().reads;
    let mut requests = Vec::new();
    for tag in 0..64u16 {
        requests.extend(tread(tag, 1, 0, 60_000));
    }
    ours.write_all(&requests).unwrap();
    // The peer reads nothing, so the socket fills and replies stay queued.
    settle(&mut server);
    let stalled = server.export().state().reads - before;
    assert!(
        (1..64).contains(&stalled),
        "{stalled} reads performed unread"
    );
    let started = Instant::now();
    assert!(
        !ready(&server, BUDGET),
        "output the peer cannot take woke the owner"
    );
    assert!(started.elapsed() >= BUDGET);
    // Room the peer makes is what ends the wait. Raw reads, not whole
    // replies: the socket may end in a reply the server has only partly
    // written, and only a turn writes the rest. An empty socket has room,
    // so a read here always finds bytes.
    let mut chunk = vec![0; 16 * 1024];
    let mut drained = 0;
    while !ready(&server, Duration::ZERO) {
        assert!(drained < 64 * 60_011, "draining every reply made no room");
        let read = ours.read(&mut chunk).unwrap();
        assert_ne!(read, 0, "the server closed the connection");
        drained += read;
    }
    turn(&mut server);
    assert!(
        server.export().state().reads - before > stalled,
        "the turn that consumed the room did no more work"
    );
}

#[test]
fn a_full_server_does_not_subscribe_its_listener() {
    let socket = Socket::new();
    let limits = Limits::new(8192, 4096, 4, 16, 32768, 1).unwrap();
    let mut server = Server::new(StaticExport::new(), limits).unwrap();
    server
        .listen(UnixListener::bind(&socket.0).unwrap())
        .unwrap();
    let _first = UnixStream::connect(&socket.0).unwrap();
    assert!(
        ready(&server, PROMPT),
        "a connection with room did not wake"
    );
    turn(&mut server);
    assert_eq!(server.connection_count(), 1);
    // Queued in the backlog, so the listener stays readable; the server
    // cannot take it, and a waiter must not wake for it.
    let _second = UnixStream::connect(&socket.0).unwrap();
    let started = Instant::now();
    assert!(
        !ready(&server, BUDGET),
        "a full server woke for its backlog"
    );
    assert!(started.elapsed() >= BUDGET);
    assert_eq!(server.connection_count(), 1);
}
