//! The read-only client against a scripted peer written from the
//! specification: version and attach, connecting, the server's errno, and
//! replies that break the protocol or the stream on purpose. No server core
//! is involved; the tests that run the client against Sophia's real server
//! stay with that server.

use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sophia_9p_client::client::{Client, ClientError, ClientLimits, File};
use sophia_9p_client::records::{Errno, Qid, QidKind};

// ---- a scripted peer, written from the specification ----

/// One request as the scripted peer reads it: type, tag and body.
fn request(stream: &mut UnixStream) -> (u8, u16, Vec<u8>) {
    let mut size = [0; 4];
    stream.read_exact(&mut size).unwrap();
    let mut rest = vec![0; u32::from_le_bytes(size) as usize - 4];
    stream.read_exact(&mut rest).unwrap();
    (
        rest[0],
        u16::from_le_bytes([rest[1], rest[2]]),
        rest[3..].to_vec(),
    )
}

fn reply(kind: u8, tag: u16, body: &[u8]) -> Vec<u8> {
    let mut frame = ((7 + body.len()) as u32).to_le_bytes().to_vec();
    frame.push(kind);
    frame.extend_from_slice(&tag.to_le_bytes());
    frame.extend_from_slice(body);
    frame
}

fn rversion(msize: u32, version: &[u8]) -> Vec<u8> {
    let mut body = msize.to_le_bytes().to_vec();
    body.extend_from_slice(&(version.len() as u16).to_le_bytes());
    body.extend_from_slice(version);
    reply(101, u16::MAX, &body)
}

fn rlerror(tag: u16, errno: u32) -> Vec<u8> {
    reply(7, tag, &errno.to_le_bytes())
}

fn string(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u16).to_le_bytes());
    out.extend_from_slice(value);
}

/// Keeps the peer's end open, reading and discarding, until the client
/// closes its own.
fn hold(stream: &mut UnixStream) {
    let mut rest = Vec::new();
    let _ = stream.read_to_end(&mut rest);
}

const QID_DIR: [u8; 13] = [0x80, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];
const QID_FILE: [u8; 13] = [0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0];

/// Runs `script` as the peer of a new client's stream.
fn scripted(
    script: impl FnOnce(&mut UnixStream) + Send + 'static,
    limits: ClientLimits,
) -> (Result<Client, ClientError>, JoinHandle<()>) {
    let (client, mut server) = UnixStream::pair().unwrap();
    let thread = std::thread::spawn(move || script(&mut server));
    (Client::over(client, limits), thread)
}

/// Answers version and attach correctly, then hands over.
fn versioned_and_attached(stream: &mut UnixStream) {
    request(stream);
    stream.write_all(&rversion(65536, b"9P2000.L")).unwrap();
    let (_, tag, _) = request(stream);
    stream.write_all(&reply(105, tag, &QID_DIR)).unwrap();
}

/// Serves version, attach, a walk to one file and its open, then the script.
fn opened(stream: &mut UnixStream) {
    versioned_and_attached(stream);
    let (_, tag, _) = request(stream);
    let mut walked = vec![1, 0];
    walked.extend_from_slice(&QID_FILE);
    stream.write_all(&reply(111, tag, &walked)).unwrap();
    let (_, tag, _) = request(stream);
    let mut open = QID_FILE.to_vec();
    open.extend_from_slice(&0u32.to_le_bytes());
    stream.write_all(&reply(13, tag, &open)).unwrap();
}

fn open_client(client: Result<Client, ClientError>) -> (Client, File) {
    let mut client = client.unwrap();
    let root = client.attach(b"", b"").unwrap();
    let mut file = client.walk(&root, &[b"x"]).unwrap();
    client.open(&mut file, false).unwrap();
    (client, file)
}

fn fast() -> ClientLimits {
    ClientLimits {
        request_deadline: Duration::from_millis(500),
        flush_deadline: Duration::from_millis(300),
        ..ClientLimits::default()
    }
}

/// A fresh directory for socket paths, unique to this process and test.
fn scratch(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("sophia-9p-client-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

// ---- version, attach and connect ----

#[test]
fn version_takes_the_smaller_msize_and_attach_sends_its_names() {
    let (client, thread) = scripted(
        |stream| {
            let (kind, tag, body) = request(stream);
            assert_eq!((kind, tag), (100, u16::MAX), "Tversion under NOTAG");
            let mut offer = 65536u32.to_le_bytes().to_vec();
            string(&mut offer, b"9P2000.L");
            assert_eq!(body, offer);
            stream.write_all(&rversion(8192, b"9P2000.L")).unwrap();

            let (kind, tag, body) = request(stream);
            assert_eq!(kind, 104);
            let mut attach = 0u32.to_le_bytes().to_vec();
            attach.extend_from_slice(&u32::MAX.to_le_bytes());
            string(&mut attach, b"mason");
            string(&mut attach, b"shell");
            attach.extend_from_slice(&u32::MAX.to_le_bytes());
            assert_eq!(body, attach, "fid, NOFID afid, names, NOFID n_uname");
            stream.write_all(&reply(105, tag, &QID_DIR)).unwrap();

            // Walk and open one file, then check the read is clamped to the
            // negotiated msize, not the offer.
            let (_, tag, _) = request(stream);
            let mut walked = vec![1, 0];
            walked.extend_from_slice(&QID_FILE);
            stream.write_all(&reply(111, tag, &walked)).unwrap();
            let (_, tag, _) = request(stream);
            let mut open = QID_FILE.to_vec();
            open.extend_from_slice(&0u32.to_le_bytes());
            stream.write_all(&reply(13, tag, &open)).unwrap();
            let (kind, tag, body) = request(stream);
            assert_eq!(kind, 116);
            assert_eq!(body[12..], (8192u32 - 11).to_le_bytes());
            stream
                .write_all(&reply(117, tag, &[3, 0, 0, 0, b'a', b'b', b'c']))
                .unwrap();
        },
        fast(),
    );
    let mut client = client.unwrap();
    assert_eq!(client.msize(), 8192);
    let root = client.attach(b"mason", b"shell").unwrap();
    assert_eq!(
        root.qid(),
        Qid {
            kind: QidKind::Directory,
            version: 0,
            path: 1
        }
    );
    assert!(!root.is_open());
    let mut file = client.walk(&root, &[b"x"]).unwrap();
    client.open(&mut file, false).unwrap();
    assert_eq!(client.read(&file, 0, u32::MAX).unwrap(), b"abc");
    assert!(!client.is_poisoned());
    thread.join().unwrap();
}

#[test]
fn a_version_at_either_msize_bound_is_accepted() {
    for msize in [ClientLimits::MIN_MSIZE, 65536] {
        let (client, thread) = scripted(
            move |stream| {
                request(stream);
                stream.write_all(&rversion(msize, b"9P2000.L")).unwrap();
            },
            fast(),
        );
        assert_eq!(client.unwrap().msize(), msize);
        thread.join().unwrap();
    }
}

#[test]
fn a_version_other_than_the_offer_is_refused() {
    for (msize, version, what) in [
        (65536, &b"unknown"[..], "dialect other than 9P2000.L"),
        (65536, b"9P2000", "dialect other than 9P2000.L"),
        (65536, b"9P2000.u", "dialect other than 9P2000.L"),
        (65536, b"9P2000.L.Google.7", "dialect other than 9P2000.L"),
        (65537, b"9P2000.L", "negotiated msize out of range"),
        (4095, b"9P2000.L", "negotiated msize out of range"),
    ] {
        let (client, thread) = scripted(
            move |stream| {
                request(stream);
                stream.write_all(&rversion(msize, version)).unwrap();
            },
            fast(),
        );
        assert_eq!(client.err(), Some(ClientError::Protocol(what)), "{msize}");
        thread.join().unwrap();
    }
}

#[test]
fn a_version_answer_of_the_wrong_kind_is_refused() {
    type Answer = fn() -> Vec<u8>;
    let cases: [(&str, Answer, ClientError); 4] = [
        (
            "another tag",
            || {
                let mut frame = rversion(65536, b"9P2000.L");
                frame[5..7].copy_from_slice(&0u16.to_le_bytes());
                frame
            },
            ClientError::Protocol("reply to another tag"),
        ),
        (
            "another type",
            || reply(105, u16::MAX, &QID_DIR),
            ClientError::Protocol("unexpected reply type"),
        ),
        (
            // An Rlerror to Tversion is a refusal carrying its wire errno,
            // in `Pipeline` as well.
            "refused by Rlerror",
            || rlerror(u16::MAX, Errno::EPROTO.0),
            ClientError::Remote(Errno::EPROTO),
        ),
        (
            "frame over the offered msize",
            || {
                let mut frame = rversion(65536, b"9P2000.L");
                frame[..4].copy_from_slice(&65537u32.to_le_bytes());
                frame
            },
            ClientError::Protocol("reply frame size"),
        ),
    ];
    for (case, answer, error) in cases {
        let (client, thread) = scripted(
            move |stream| {
                request(stream);
                let _ = stream.write_all(&answer());
            },
            fast(),
        );
        assert_eq!(client.err(), Some(error), "{case}");
        thread.join().unwrap();
    }
}

#[test]
fn connecting_reports_why_and_is_bounded_by_the_request_deadline() {
    let directory = scratch("connect");
    assert_eq!(
        Client::connect(&directory.join("missing"), fast()).err(),
        Some(ClientError::Io(ErrorKind::NotFound))
    );

    // A socket file whose listener is gone: nothing accepts there.
    let stale = directory.join("stale");
    drop(UnixListener::bind(&stale).unwrap());
    assert_eq!(
        Client::connect(&stale, fast()).err(),
        Some(ClientError::Io(ErrorKind::ConnectionRefused))
    );

    // A listener that never accepts or answers: the connect completes into
    // its queue and the version negotiation meets the deadline.
    let silent = directory.join("silent");
    let _listener = UnixListener::bind(&silent).unwrap();
    let limits = ClientLimits {
        request_deadline: Duration::from_millis(200),
        ..fast()
    };
    let started = Instant::now();
    assert_eq!(
        Client::connect(&silent, limits).err(),
        Some(ClientError::Timeout)
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn connecting_to_a_listener_negotiates_and_attaches() {
    let directory = scratch("listen");
    let path = directory.join("export");
    let listener = UnixListener::bind(&path).unwrap();
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        request(&mut stream);
        stream.write_all(&rversion(16384, b"9P2000.L")).unwrap();
        let (_, tag, _) = request(&mut stream);
        stream.write_all(&reply(105, tag, &QID_DIR)).unwrap();
        hold(&mut stream);
    });
    let mut client = Client::connect(&path, fast()).unwrap();
    assert_eq!(client.msize(), 16384);
    let root = client.attach(b"", b"").unwrap();
    assert_eq!(root.qid().kind, QidKind::Directory);
    drop(client);
    thread.join().unwrap();
    let _ = std::fs::remove_dir_all(directory);
}

// ---- the server's errno ----

/// `Rlerror` carries a Linux errno from the server, not the host's: it is
/// handed back exactly as `ClientError::Remote`, never mapped to an I/O
/// kind (EAGAIN is not the host's `WouldBlock`, nor a timeout), and it
/// leaves the client usable.
#[test]
fn a_server_errno_is_carried_exactly_and_leaves_the_client_usable() {
    let (client, thread) = scripted(
        |stream| {
            versioned_and_attached(stream);
            let (_, tag, _) = request(stream);
            stream.write_all(&rlerror(tag, 116)).unwrap();
            let (_, tag, _) = request(stream);
            stream.write_all(&rlerror(tag, 4242)).unwrap();
            let (_, tag, _) = request(stream);
            let mut walked = vec![1, 0];
            walked.extend_from_slice(&QID_FILE);
            stream.write_all(&reply(111, tag, &walked)).unwrap();
            let (_, tag, _) = request(stream);
            stream.write_all(&rlerror(tag, 11)).unwrap();
            for _ in 0..2 {
                let (kind, tag, _) = request(stream);
                assert_eq!(kind, 120);
                stream.write_all(&reply(121, tag, &[])).unwrap();
            }
        },
        fast(),
    );
    let mut client = client.unwrap();
    let root = client.attach(b"", b"").unwrap();
    assert_eq!(
        client.walk(&root, &[b"gone"]).unwrap_err(),
        ClientError::Remote(Errno::ESTALE)
    );
    assert_eq!(
        client.walk(&root, &[b"odd"]).unwrap_err(),
        ClientError::Remote(Errno(4242)),
        "an errno the host may not know is still carried as sent"
    );
    let mut file = client.walk(&root, &[b"x"]).unwrap();
    assert_eq!(
        client.open(&mut file, false).unwrap_err(),
        ClientError::Remote(Errno::EAGAIN)
    );
    assert!(!file.is_open());
    assert!(!client.is_poisoned());
    client.clunk(file).unwrap();
    client.clunk(root).unwrap();
    thread.join().unwrap();
}

/// A waiting read the server refuses with EAGAIN is that refusal, not its
/// own deadline passing: no flush is sent, and the client stays usable.
#[test]
fn a_waiting_read_refused_with_eagain_is_not_its_deadline() {
    let (client, thread) = scripted(
        |stream| {
            opened(stream);
            let (kind, tag, _) = request(stream);
            assert_eq!(kind, 116);
            stream.write_all(&rlerror(tag, 11)).unwrap();
            let (kind, tag, _) = request(stream);
            assert_eq!(kind, 120, "a clunk, not a flush");
            stream.write_all(&reply(121, tag, &[])).unwrap();
        },
        fast(),
    );
    let (mut client, file) = open_client(client);
    let deadline = Instant::now() + Duration::from_secs(1);
    assert_eq!(
        client.read_until(&file, 0, 64, deadline).unwrap_err(),
        ClientError::Remote(Errno::EAGAIN)
    );
    assert!(!client.is_poisoned());
    client.clunk(file).unwrap();
    thread.join().unwrap();
}

// ---- replies that break the protocol ----

#[test]
fn a_malformed_reply_poisons_the_client() {
    type Answer = fn(u16) -> Vec<u8>;
    let cases: [(&str, Answer, ClientError); 8] = [
        (
            "another tag",
            |tag| reply(111, tag + 1, &[0, 0]),
            ClientError::Protocol("reply to another tag"),
        ),
        (
            "another type",
            |tag| reply(105, tag, &QID_DIR),
            ClientError::Protocol("unexpected reply type"),
        ),
        (
            "short body",
            |tag| reply(111, tag, &[0]),
            ClientError::Protocol("Rwalk count"),
        ),
        (
            "more qids than names",
            |tag| reply(111, tag, &[1, 0]),
            ClientError::Protocol("Rwalk count"),
        ),
        (
            "long body",
            |tag| reply(111, tag, &[0, 0, 9]),
            ClientError::Protocol("Rwalk length"),
        ),
        (
            "long error",
            |tag| reply(7, tag, &[2, 0, 0, 0, 0]),
            ClientError::Protocol("Rlerror length"),
        ),
        (
            "short error",
            |tag| reply(7, tag, &[2, 0]),
            ClientError::Protocol("Rlerror length"),
        ),
        (
            "oversize frame",
            |tag| {
                let mut frame = reply(111, tag, &[0, 0]);
                frame[..4].copy_from_slice(&70_000u32.to_le_bytes());
                frame
            },
            ClientError::Protocol("reply frame size"),
        ),
    ];
    for (case, answer, error) in cases {
        let (client, thread) = scripted(
            move |stream| {
                versioned_and_attached(stream);
                let (_, tag, _) = request(stream);
                let _ = stream.write_all(&answer(tag));
            },
            fast(),
        );
        let mut client = client.unwrap();
        let root = client.attach(b"", b"").unwrap();
        assert_eq!(client.walk(&root, &[]).unwrap_err(), error, "{case}");
        assert!(client.is_poisoned(), "{case}");
        assert_eq!(
            client.getattr(&root).unwrap_err(),
            ClientError::Poisoned,
            "{case}"
        );
        thread.join().unwrap();
    }
}

/// The frame limit is the negotiated msize, not the offer: a frame one byte
/// past it, or one shorter than a header, poisons before its body is read.
#[test]
fn a_frame_outside_the_negotiated_msize_poisons_the_client() {
    for size in [8193u32, 6] {
        let (client, thread) = scripted(
            move |stream| {
                request(stream);
                stream.write_all(&rversion(8192, b"9P2000.L")).unwrap();
                let (_, tag, _) = request(stream);
                let mut frame = reply(105, tag, &QID_DIR);
                frame[..4].copy_from_slice(&size.to_le_bytes());
                let _ = stream.write_all(&frame);
                hold(stream);
            },
            fast(),
        );
        let mut client = client.unwrap();
        assert_eq!(client.msize(), 8192);
        assert_eq!(
            client.attach(b"", b"").unwrap_err(),
            ClientError::Protocol("reply frame size"),
            "{size}"
        );
        assert!(client.is_poisoned(), "{size}");
        drop(client);
        thread.join().unwrap();
    }
}

#[test]
fn a_read_answered_with_more_than_it_asked_poisons_the_client() {
    type Answer = fn(u16) -> Vec<u8>;
    let cases: [(&str, Answer); 2] = [
        ("five bytes for four", |tag| {
            reply(117, tag, &[5, 0, 0, 0, 1, 2, 3, 4, 5])
        }),
        ("a count its data does not match", |tag| {
            reply(117, tag, &[3, 0, 0, 0, 1, 2])
        }),
    ];
    for (case, answer) in cases {
        let (client, thread) = scripted(
            move |stream| {
                opened(stream);
                let (_, tag, _) = request(stream);
                let _ = stream.write_all(&answer(tag));
            },
            fast(),
        );
        let (mut client, file) = open_client(client);
        assert_eq!(
            client.read(&file, 0, 4).unwrap_err(),
            ClientError::Protocol("Rread count"),
            "{case}"
        );
        assert!(client.is_poisoned(), "{case}");
        thread.join().unwrap();
    }
}

// ---- a peer that goes away ----

#[test]
fn a_frame_cut_by_the_peer_closing_is_a_disconnect() {
    let (client, thread) = scripted(
        |stream| {
            versioned_and_attached(stream);
            let (_, tag, _) = request(stream);
            let mut walked = vec![1, 0];
            walked.extend_from_slice(&QID_FILE);
            // Header and two body bytes of the fifteen, then gone.
            stream.write_all(&reply(111, tag, &walked)[..9]).unwrap();
        },
        fast(),
    );
    let mut client = client.unwrap();
    let root = client.attach(b"", b"").unwrap();
    assert_eq!(
        client.walk(&root, &[b"x"]).unwrap_err(),
        ClientError::Io(ErrorKind::UnexpectedEof)
    );
    assert!(client.is_poisoned());
    assert_eq!(client.clunk(root).unwrap_err(), ClientError::Poisoned);
    thread.join().unwrap();
}

#[test]
fn a_peer_that_closes_before_answering_is_a_disconnect() {
    let (client, thread) = scripted(
        |stream| {
            versioned_and_attached(stream);
            // Reads the walk, then closes without answering it.
            request(stream);
        },
        fast(),
    );
    let mut client = client.unwrap();
    let root = client.attach(b"", b"").unwrap();
    assert_eq!(
        client.walk(&root, &[b"x"]).unwrap_err(),
        ClientError::Io(ErrorKind::UnexpectedEof)
    );
    assert!(client.is_poisoned());
    thread.join().unwrap();

    let (client, thread) = scripted(versioned_and_attached, fast());
    let mut client = client.unwrap();
    let root = client.attach(b"", b"").unwrap();
    thread.join().unwrap();
    // The peer is already gone before the walk is written: depending on
    // whether the write or the read notices first, a broken pipe or an end
    // of file, either way an `Io` error that poisons.
    match client.walk(&root, &[b"x"]) {
        Err(ClientError::Io(kind)) => assert!(
            matches!(kind, ErrorKind::BrokenPipe | ErrorKind::UnexpectedEof),
            "unexpected io error kind: {kind:?}"
        ),
        other => panic!("expected an Io error, got {other:?}"),
    }
    assert!(client.is_poisoned());
}

// ---- flushing a read that outlasted its deadline ----

#[test]
fn a_reply_cut_by_the_deadline_is_finished_under_the_flush() {
    let (client, thread) = scripted(
        |stream| {
            opened(stream);
            let (_, read_tag, _) = request(stream);
            let late = reply(117, read_tag, &[3, 0, 0, 0, 7, 8, 9]);
            // Half of the reply before the read's deadline; the rest, and the
            // flush's answer, only after the flush arrives.
            stream.write_all(&late[..5]).unwrap();
            let (kind, flush_tag, body) = request(stream);
            assert_eq!((kind, body), (108, read_tag.to_le_bytes().to_vec()));
            stream.write_all(&late[5..]).unwrap();
            stream.write_all(&reply(109, flush_tag, &[])).unwrap();
            let (_, tag, _) = request(stream);
            stream.write_all(&reply(121, tag, &[])).unwrap();
        },
        fast(),
    );
    let (mut client, file) = open_client(client);
    let deadline = Instant::now() + Duration::from_millis(100);
    assert_eq!(client.read_until(&file, 0, 64, deadline).unwrap(), None);
    assert!(
        !client.is_poisoned(),
        "the cut frame was finished, not reparsed"
    );
    client.clunk(file).unwrap();
    thread.join().unwrap();
}

/// flush(5): the flushed request's own answer may still beat the `Rflush`;
/// whether it is data or an error, it is discarded, once, and the read is
/// reported cancelled.
#[test]
fn an_answer_that_beats_the_rflush_is_discarded() {
    type Late = fn(u16) -> Vec<u8>;
    let cases: [(&str, Late); 2] = [
        ("data", |tag| reply(117, tag, &[1, 0, 0, 0, 7])),
        ("an error", |tag| rlerror(tag, 11)),
    ];
    for (case, late) in cases {
        let (client, thread) = scripted(
            move |stream| {
                opened(stream);
                let (_, read_tag, _) = request(stream);
                let (kind, flush_tag, _) = request(stream);
                assert_eq!(kind, 108);
                stream.write_all(&late(read_tag)).unwrap();
                stream.write_all(&reply(109, flush_tag, &[])).unwrap();
                let (_, tag, _) = request(stream);
                stream.write_all(&reply(121, tag, &[])).unwrap();
            },
            fast(),
        );
        let (mut client, file) = open_client(client);
        let deadline = Instant::now() + Duration::from_millis(100);
        assert_eq!(
            client.read_until(&file, 0, 64, deadline).unwrap(),
            None,
            "{case}"
        );
        assert!(!client.is_poisoned(), "{case}");
        client.clunk(file).unwrap();
        thread.join().unwrap();
    }
}

#[test]
fn a_stray_reply_during_a_flush_poisons_the_client() {
    let (client, thread) = scripted(
        |stream| {
            opened(stream);
            let (_, read_tag, _) = request(stream);
            let (_, flush_tag, _) = request(stream);
            // Two answers to the flushed read: only one may come.
            stream.write_all(&rlerror(read_tag, 11)).unwrap();
            stream.write_all(&rlerror(read_tag, 11)).unwrap();
            let _ = stream.write_all(&reply(109, flush_tag, &[]));
            hold(stream);
        },
        fast(),
    );
    let (mut client, file) = open_client(client);
    let deadline = Instant::now() + Duration::from_millis(100);
    assert_eq!(
        client.read_until(&file, 0, 64, deadline).unwrap_err(),
        ClientError::Protocol("reply during flush")
    );
    assert!(client.is_poisoned());
    drop(client);
    thread.join().unwrap();
}

#[test]
fn a_flush_that_is_never_answered_poisons_the_client() {
    let (client, thread) = scripted(
        |stream| {
            opened(stream);
            let (_, read_tag, _) = request(stream);
            stream
                .write_all(&reply(117, read_tag, &[3, 0, 0, 0, 7])[..5])
                .unwrap();
            request(stream);
            // Neither the rest of the read nor the flush's answer is sent.
            hold(stream);
        },
        fast(),
    );
    let (mut client, file) = open_client(client);
    let deadline = Instant::now() + Duration::from_millis(100);
    let started = Instant::now();
    assert_eq!(
        client.read_until(&file, 0, 64, deadline).unwrap_err(),
        ClientError::Timeout
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(client.is_poisoned());
    drop(client);
    thread.join().unwrap();
}
