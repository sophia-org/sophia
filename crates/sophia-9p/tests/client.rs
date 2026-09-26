//! The read-only client against the real driver and static export, and
//! against a scripted server that breaks the protocol on purpose.

mod support;

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sophia_9p::client::{Client, ClientError, ClientLimits};
use sophia_9p::unix::{Server, Wake};
use sophia_9p::{Epoch, Errno, Limits, QidKind};
use support::StaticExport;
use support::export::{INFO_LEN, info_byte};

/// The static export served by the real driver on its own thread.
struct Served {
    export: StaticExport,
    wake: Wake,
    thread: Option<JoinHandle<()>>,
    server: Option<Server<StaticExport>>,
}

impl Served {
    fn new() -> Self {
        let export = StaticExport::new();
        let server = Server::new(export.clone(), Limits::default()).unwrap();
        Self {
            export,
            wake: server.wake(),
            thread: None,
            server: Some(server),
        }
    }

    /// A client stream adopted by the server; call before `start`.
    fn stream(&mut self) -> UnixStream {
        let (client, server) = UnixStream::pair().unwrap();
        assert!(
            self.server.as_mut().unwrap().adopt(server).is_ok(),
            "adopted"
        );
        client
    }

    fn start(&mut self) {
        let mut server = self.server.take().unwrap();
        self.thread = Some(std::thread::spawn(move || server.run().unwrap()));
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        self.wake.stop();
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

fn client() -> (Served, Client) {
    let mut served = Served::new();
    let stream = served.stream();
    served.start();
    let client = Client::over(stream, ClientLimits::default()).unwrap();
    (served, client)
}

#[test]
fn reads_stats_and_lists_through_the_real_driver() {
    let (_served, mut client) = client();
    assert_eq!(client.msize(), 65536);
    let root = client.attach(b"", b"").unwrap();
    assert_eq!(root.qid().kind, QidKind::Directory);

    let mut leaf = client.walk(&root, &[b"dir", b"leaf"]).unwrap();
    assert_eq!(leaf.qid().kind, QidKind::File);
    client.open(&mut leaf, false).unwrap();
    assert_eq!(client.read(&leaf, 0, 64).unwrap(), [0, 1, 2, 3]);
    assert_eq!(client.read(&leaf, 4, 64).unwrap(), Vec::<u8>::new());

    let mut info = client.walk(&root, &[b"info"]).unwrap();
    let attr = client.getattr(&info).unwrap();
    assert_eq!((attr.mode, attr.size), (0o100444, INFO_LEN as u64));
    client.open(&mut info, false).unwrap();
    let data = client.read_to_end(&info, INFO_LEN).unwrap();
    assert_eq!(data.len(), INFO_LEN);
    assert!(
        data.iter()
            .enumerate()
            .all(|(at, byte)| *byte == info_byte(at))
    );
    assert_eq!(
        client.read_to_end(&info, INFO_LEN - 1),
        Err(ClientError::Limit("file larger than max_bytes"))
    );

    let mut listing = client.walk(&root, &[]).unwrap();
    client.open(&mut listing, true).unwrap();
    let entries = client.list_all(&listing, 16).unwrap();
    let names: Vec<&[u8]> = entries.iter().map(|entry| entry.name.as_slice()).collect();
    assert_eq!(names, [&b"info"[..], b"sink", b"events", b"dir"]);
    assert_eq!(entries[3].qid.kind, QidKind::Directory);
    assert_eq!(entries[3].dtype, 4);
    assert_eq!(
        client.list_all(&listing, 3),
        Err(ClientError::Limit("directory larger than max_entries"))
    );
    let page = client.readdir(&listing, 1, 28).unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].name, b"sink");

    for file in [leaf, info, listing, root] {
        client.clunk(file).unwrap();
    }
    assert!(!client.is_poisoned());
}

#[test]
fn server_refusals_leave_the_client_usable() {
    let (served, mut client) = client();
    let root = client.attach(b"", b"").unwrap();
    assert_eq!(
        client.walk(&root, &[b"hidden"]).unwrap_err(),
        ClientError::Remote(Errno::EACCES)
    );
    assert_eq!(
        client.walk(&root, &[b"missing"]).unwrap_err(),
        ClientError::Remote(Errno::ENOENT)
    );
    assert_eq!(
        client.walk(&root, &[b"dir", b"missing"]).unwrap_err(),
        ClientError::Remote(Errno::ENOENT),
        "a walk that stops short creates no fid"
    );
    let mut info = client.walk(&root, &[b"info"]).unwrap();
    assert_eq!(
        client.readdir(&info, 0, 64).unwrap_err(),
        ClientError::Limit("not open")
    );
    assert_eq!(
        client.open(&mut info, true).unwrap_err(),
        ClientError::Remote(Errno::ENOTDIR)
    );
    client.open(&mut info, false).unwrap();
    assert_eq!(
        client.walk(&info, &[]).unwrap_err(),
        ClientError::Limit("walk from an open file")
    );
    let mut dir = client.walk(&root, &[b"dir"]).unwrap();
    client.open(&mut dir, false).unwrap();
    assert_eq!(
        client.read(&dir, 0, 10).unwrap_err(),
        ClientError::Remote(Errno::EISDIR)
    );
    served.export.revoke(Epoch(1));
    assert_eq!(
        client.getattr(&root).unwrap_err(),
        ClientError::Remote(Errno::ESTALE)
    );
    assert!(!client.is_poisoned());
}

#[test]
fn fids_are_bounded_and_belong_to_their_client() {
    let mut served = Served::new();
    let first = served.stream();
    let second = served.stream();
    served.start();
    let limits = ClientLimits {
        max_fids: 2,
        ..ClientLimits::default()
    };
    let mut client = Client::over(first, limits).unwrap();
    let mut other = Client::over(second, limits).unwrap();
    let root = client.attach(b"", b"").unwrap();
    let dir = client.walk(&root, &[b"dir"]).unwrap();
    assert_eq!(
        client.walk(&root, &[b"info"]).unwrap_err(),
        ClientError::Limit("max_fids reached")
    );
    client.clunk(dir).unwrap();
    let info = client.walk(&root, &[b"info"]).unwrap();
    assert_eq!(other.getattr(&info).unwrap_err(), ClientError::ForeignFile);
    assert_eq!(other.clunk(info).unwrap_err(), ClientError::ForeignFile);
    assert!(!other.is_poisoned());
}

#[test]
fn a_waiting_read_is_flushed_at_its_deadline_and_can_be_read_again() {
    let (served, mut client) = client();
    let root = client.attach(b"", b"").unwrap();
    let mut events = client.walk(&root, &[b"events"]).unwrap();
    client.open(&mut events, false).unwrap();
    let started = Instant::now();
    let waited = client
        .read_until(&events, 0, 64, Instant::now() + Duration::from_millis(100))
        .unwrap();
    assert_eq!(waited, None);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(!client.is_poisoned());
    assert_eq!(
        client.read_until(&events, 0, 64, Instant::now()).unwrap(),
        None,
        "a deadline already passed sends nothing"
    );
    served.export.push_event(b"one");
    served.wake.wake();
    let read = client
        .read_until(&events, 0, 64, Instant::now() + Duration::from_secs(2))
        .unwrap();
    assert_eq!(read, Some(b"one".to_vec()));
}

#[test]
fn connecting_is_bounded_by_the_request_deadline() {
    let directory = std::env::temp_dir().join(format!("sophia-9p-client-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let missing = directory.join("missing");
    assert_eq!(
        Client::connect(&missing, ClientLimits::default()).err(),
        Some(ClientError::Io(std::io::ErrorKind::NotFound))
    );
    // A listener that never accepts or answers: the connect completes into
    // its queue and the version negotiation meets the deadline.
    let path = directory.join("silent");
    let _ = std::fs::remove_file(&path);
    let _listener = UnixListener::bind(&path).unwrap();
    let limits = ClientLimits {
        request_deadline: Duration::from_millis(200),
        ..ClientLimits::default()
    };
    let started = Instant::now();
    assert_eq!(
        Client::connect(&path, limits).err(),
        Some(ClientError::Timeout)
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    let _ = std::fs::remove_dir_all(&directory);
}

// ---- a scripted server, written from the specification ----

/// One request as the scripted server reads it: type, tag and body.
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

const QID_DIR: [u8; 13] = [0x80, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];
const QID_FILE: [u8; 13] = [0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0];

/// Runs `script` as the server of a new client's stream.
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

fn fast() -> ClientLimits {
    ClientLimits {
        request_deadline: Duration::from_millis(500),
        flush_deadline: Duration::from_millis(300),
        ..ClientLimits::default()
    }
}

#[test]
fn a_version_other_than_the_offer_is_refused() {
    for (msize, version, what) in [
        (65536, &b"9P2000.u"[..], "dialect other than 9P2000.L"),
        (65536, b"9P2000.L.Google.7", "dialect other than 9P2000.L"),
        (1 << 20, b"9P2000.L", "negotiated msize out of range"),
        (512, b"9P2000.L", "negotiated msize out of range"),
    ] {
        let (client, thread) = scripted(
            move |stream| {
                request(stream);
                stream.write_all(&rversion(msize, version)).unwrap();
            },
            fast(),
        );
        assert_eq!(client.err(), Some(ClientError::Protocol(what)));
        thread.join().unwrap();
    }
}

#[test]
fn a_malformed_reply_poisons_the_client() {
    type Answer = fn(u16) -> Vec<u8>;
    let cases: [(&str, Answer, ClientError); 7] = [
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

/// Serves version, attach, a walk to one file and its open, then the script.
fn opened(stream: &mut UnixStream, directory: bool) {
    versioned_and_attached(stream);
    let (_, tag, _) = request(stream);
    let qid = if directory { QID_DIR } else { QID_FILE };
    let mut walked = vec![1, 0];
    walked.extend_from_slice(&qid);
    stream.write_all(&reply(111, tag, &walked)).unwrap();
    let (_, tag, _) = request(stream);
    let mut open = qid.to_vec();
    open.extend_from_slice(&0u32.to_le_bytes());
    stream.write_all(&reply(13, tag, &open)).unwrap();
}

fn open_client(
    client: Result<Client, ClientError>,
    directory: bool,
) -> (Client, sophia_9p::client::File) {
    let mut client = client.unwrap();
    let root = client.attach(b"", b"").unwrap();
    let mut file = client.walk(&root, &[b"x"]).unwrap();
    client.open(&mut file, directory).unwrap();
    (client, file)
}

#[test]
fn a_listing_that_breaks_the_entry_rules_poisons_the_client() {
    let (client, thread) = scripted(
        |stream| {
            opened(stream, true);
            let (_, tag, _) = request(stream);
            // One entry named ".": qid, offset 1, d_type 4, name.
            let mut entry = QID_DIR.to_vec();
            entry.extend_from_slice(&1u64.to_le_bytes());
            entry.push(4);
            entry.extend_from_slice(&1u16.to_le_bytes());
            entry.push(b'.');
            let mut body = (entry.len() as u32).to_le_bytes().to_vec();
            body.extend(entry);
            stream.write_all(&reply(41, tag, &body)).unwrap();
        },
        fast(),
    );
    let (mut client, directory) = open_client(client, true);
    assert_eq!(
        client.readdir(&directory, 0, 1024).unwrap_err(),
        ClientError::Protocol("Rreaddir entry")
    );
    assert!(client.is_poisoned());
    thread.join().unwrap();
}

#[test]
fn a_reply_cut_by_the_deadline_is_finished_under_the_flush() {
    let (client, thread) = scripted(
        |stream| {
            opened(stream, false);
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
    let (mut client, file) = open_client(client, false);
    let deadline = Instant::now() + Duration::from_millis(100);
    assert_eq!(client.read_until(&file, 0, 64, deadline).unwrap(), None);
    assert!(
        !client.is_poisoned(),
        "the cut frame was finished, not reparsed"
    );
    client.clunk(file).unwrap();
    thread.join().unwrap();
}

#[test]
fn a_flush_that_is_never_answered_poisons_the_client() {
    let (client, thread) = scripted(
        |stream| {
            opened(stream, false);
            let (_, read_tag, _) = request(stream);
            stream
                .write_all(&reply(117, read_tag, &[3, 0, 0, 0, 7])[..5])
                .unwrap();
            request(stream);
            // Neither the rest of the read nor the flush's answer is sent.
            let mut rest = Vec::new();
            let _ = stream.read_to_end(&mut rest);
        },
        fast(),
    );
    let (mut client, file) = open_client(client, false);
    let deadline = Instant::now() + Duration::from_millis(100);
    assert_eq!(
        client.read_until(&file, 0, 64, deadline).unwrap_err(),
        ClientError::Timeout
    );
    assert!(client.is_poisoned());
    drop(client);
    thread.join().unwrap();
}
