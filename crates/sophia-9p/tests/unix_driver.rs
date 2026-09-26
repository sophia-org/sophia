//! The Unix-socket driver over real sockets: a harness listener, an adopted
//! socket, wakeups for waiting reads, disconnect, the connection limit,
//! backpressure and stop.

mod support;

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sophia_9p::Limits;
use sophia_9p::unix::{Server, Wake};
use support::*;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Socket(PathBuf);

impl Socket {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "sophia-9p-{}-{}.sock",
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

struct Running {
    export: StaticExport,
    wake: Wake,
    thread: Option<JoinHandle<()>>,
}

impl Running {
    fn start(server: Server<StaticExport>) -> Self {
        let export = server.export().clone();
        let wake = server.wake();
        let mut server = server;
        let thread = std::thread::spawn(move || server.run().unwrap());
        Self {
            export,
            wake,
            thread: Some(thread),
        }
    }

    fn listening(socket: &Socket, limits: Limits) -> Self {
        let mut server = Server::new(StaticExport::new(), limits).unwrap();
        server
            .listen(UnixListener::bind(&socket.0).unwrap())
            .unwrap();
        Self::start(server)
    }

    fn stop(mut self) -> StaticExport {
        self.wake.stop();
        self.thread.take().unwrap().join().unwrap();
        self.export.clone()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.wake.stop();
            let _ = thread.join();
        }
    }
}

fn connect(socket: &Socket) -> UnixStream {
    let stream = UnixStream::connect(&socket.0).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
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

fn exchange(stream: &mut UnixStream, request: &[u8]) -> Frame {
    stream.write_all(request).unwrap();
    reply(stream)
}

fn session(stream: &mut UnixStream) {
    let version = exchange(stream, &tversion(NOTAG, 8192, b"9P2000.L"));
    assert_eq!(version.version(), (8192, b"9P2000.L".to_vec()));
    exchange(stream, &tattach(1, 0, NOFID, b"", b"")).attach();
}

fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    let start = Instant::now();
    while !condition() {
        assert!(start.elapsed() < Duration::from_secs(5), "never: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_listener_client_walks_opens_and_reads() {
    let socket = Socket::new();
    let running = Running::listening(&socket, Limits::default());
    let mut stream = connect(&socket);
    session(&mut stream);
    exchange(&mut stream, &twalk(2, 0, 1, &[b"dir", b"leaf"])).walk();
    exchange(&mut stream, &tlopen(3, 1, 0)).lopen();
    assert_eq!(
        exchange(&mut stream, &tread(4, 1, 0, 64)).data(),
        vec![0, 1, 2, 3]
    );
    let peer = running.export.state().attaches[0].1.unwrap();
    assert_eq!(peer.pid, i32::try_from(std::process::id()).unwrap());
}

#[test]
fn an_adopted_socket_is_served_like_an_accepted_one() {
    let (ours, theirs) = UnixStream::pair().unwrap();
    let mut server = Server::new(StaticExport::new(), Limits::default()).unwrap();
    server
        .adopt(theirs)
        .map_err(|refused| refused.error)
        .unwrap();
    let running = Running::start(server);
    let mut stream = ours;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    session(&mut stream);
    assert_eq!(exchange(&mut stream, &tgetattr(2, 0)).getattr().2, 0o040555);
    drop(running);
}

#[test]
fn past_the_connection_limit_a_socket_is_handed_back() {
    let limits = Limits::new(8192, 4096, 4, 16, 32768, 1).unwrap();
    let mut server = Server::new(StaticExport::new(), limits).unwrap();
    let (_first, theirs) = UnixStream::pair().unwrap();
    server
        .adopt(theirs)
        .map_err(|refused| refused.error)
        .unwrap();
    let (_second, theirs) = UnixStream::pair().unwrap();
    let Err(refused) = server.adopt(theirs) else {
        panic!("the second is refused");
    };
    assert_eq!(refused.error.to_string(), "connection limit reached");
    assert_eq!(server.connection_count(), 1);
}

#[test]
fn a_wake_answers_a_waiting_read() {
    let socket = Socket::new();
    let running = Running::listening(&socket, Limits::default());
    let mut stream = connect(&socket);
    session(&mut stream);
    exchange(&mut stream, &twalk(2, 0, 1, &[b"events"])).walk();
    exchange(&mut stream, &tlopen(3, 1, 0)).lopen();
    stream.write_all(&tread(9, 1, 0, 64)).unwrap();
    eventually("the read waits", || running.export.state().reads == 1);
    running.export.push_event(b"woken");
    running.wake.wake();
    let answered = reply(&mut stream);
    assert_eq!((answered.tag, answered.data()), (9, b"woken".to_vec()));
}

#[test]
fn a_disconnect_releases_every_fid_and_handle() {
    let socket = Socket::new();
    let running = Running::listening(&socket, Limits::default());
    let mut stream = connect(&socket);
    session(&mut stream);
    exchange(&mut stream, &twalk(2, 0, 1, &[b"events"])).walk();
    exchange(&mut stream, &tlopen(3, 1, 0)).lopen();
    stream.write_all(&tread(9, 1, 0, 64)).unwrap();
    eventually("the read waits", || running.export.state().reads == 1);
    drop(stream);
    eventually("both fids released", || {
        running.export.state().released.len() == 2
    });
    let released = running.export.state().released.clone();
    assert_eq!(released.iter().filter(|(_, _, open)| *open).count(), 1);
}

#[test]
fn a_fatal_violation_closes_the_connection() {
    let socket = Socket::new();
    let running = Running::listening(&socket, Limits::default());
    let mut stream = connect(&socket);
    session(&mut stream);
    stream.write_all(&tclunk(NOTAG, 0)).unwrap();
    let mut byte = [0; 1];
    assert_eq!(stream.read(&mut byte).unwrap(), 0, "closed without a reply");
    eventually("the root fid released", || {
        running.export.state().released.len() == 1
    });
}

#[test]
fn a_client_that_does_not_read_gets_no_more_work_done() {
    let socket = Socket::new();
    let running = Running::listening(&socket, Limits::default());
    let mut stream = connect(&socket);
    let version = exchange(&mut stream, &tversion(NOTAG, 65536, b"9P2000.L"));
    assert_eq!(version.version().0, 65536);
    exchange(&mut stream, &tattach(1, 0, NOFID, b"", b"")).attach();
    exchange(&mut stream, &twalk(2, 0, 1, &[b"info"])).walk();
    exchange(&mut stream, &tlopen(3, 1, 0)).lopen();
    let before = running.export.state().reads;
    let mut requests = Vec::new();
    for tag in 0..64u16 {
        requests.extend(tread(tag, 1, 0, 60_000));
    }
    stream.write_all(&requests).unwrap();
    // Four replies of 60011 bytes fit the 256 KiB bound; the kernel socket
    // buffer takes a few more. Far fewer than 64 reads are performed.
    std::thread::sleep(Duration::from_millis(200));
    let performed = running.export.state().reads - before;
    assert!(
        (1..32).contains(&performed),
        "{performed} reads performed unread"
    );
    for tag in 0..64u16 {
        let answered = reply(&mut stream);
        assert_eq!(answered.tag, tag);
        assert_eq!(answered.data().len(), 60_000);
    }
    assert_eq!(running.export.state().reads - before, 64);
}

#[test]
fn stop_closes_every_connection() {
    let socket = Socket::new();
    let running = Running::listening(&socket, Limits::default());
    let mut stream = connect(&socket);
    session(&mut stream);
    exchange(&mut stream, &twalk(2, 0, 1, &[b"dir"])).walk();
    let export = running.stop();
    assert_eq!(export.state().released.len(), 2);
    let mut byte = [0; 1];
    assert_eq!(stream.read(&mut byte).unwrap(), 0);
}
