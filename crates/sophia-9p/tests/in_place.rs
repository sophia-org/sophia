//! Writes received in place: an export with private staging takes a write's
//! data straight from the stream, through `Connection::receive` and through
//! real sockets, and answers exactly as a buffered write would. Refusals,
//! revocation and an owner that stops taking the data drain the rest of the
//! request and keep the next one intact; nothing is committed unless the
//! whole write arrived; the reply's room is kept; a default export never
//! receives in place.

mod support;

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sophia_9p::unix::Server;
use sophia_9p::{
    Access, AttachContext, Attachment, Connection, ConnectionId, DirEntry, Entry, Epoch, Errno,
    Export, Fatal, Limits, NodeKind, OpenFlags, Operation, ReadOutcome, WalkName,
};
use support::{
    EBADF, EINVAL, ENOSPC, EPROTO, ESTALE, Frame, NOFID, frames, tattach, tclunk, tgetattr, tlopen,
    tread, tversion, twalk, twrite,
};

const CAPACITY: usize = 16 * 1024;
const EIO: u32 = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Node {
    Root,
    Upload,
    Events,
}

/// What the owner did and decided, shared so a test can see and steer it.
#[derive(Default)]
struct Ledger {
    committed: Vec<u8>,
    destinations: usize,
    commits: usize,
    writes: usize,
    revoked: bool,
    /// Refuse a destination asked once this much has been received.
    refuse_at: Option<(u32, Errno)>,
    /// Answer `None` once this much has been received.
    none_at: Option<u32>,
    /// Offer a destination this much shorter than the rest of the write.
    short_by: usize,
    events: Vec<Vec<u8>>,
}

/// One upload file whose writes land in private staging and count only once
/// accepted, at the committed cursor; and an events file whose reads wait.
#[derive(Clone)]
struct Staging<const IN_PLACE: bool> {
    staging: Vec<u8>,
    ledger: Arc<Mutex<Ledger>>,
}

impl<const IN_PLACE: bool> Staging<IN_PLACE> {
    fn new() -> Self {
        Self {
            staging: vec![0; CAPACITY],
            ledger: Arc::default(),
        }
    }

    fn ledger(&self) -> std::sync::MutexGuard<'_, Ledger> {
        self.ledger.lock().unwrap()
    }
}

impl<const IN_PLACE: bool> Export for Staging<IN_PLACE> {
    type Node = Node;
    type Handle = ();

    const WRITES_IN_PLACE: bool = IN_PLACE;

    fn attach(&mut self, _: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        Ok(Attachment {
            root: Node::Root,
            epoch: Epoch(1),
        })
    }

    fn check(&mut self, access: &Access<'_, Node>) -> Result<(), Errno> {
        if self.ledger().revoked {
            return Err(Errno::ESTALE);
        }
        match (access.node, access.operation) {
            (Node::Events, Operation::Open(flags))
                if flags.access().is_some_and(|access| access.writes()) =>
            {
                Err(Errno::EACCES)
            }
            _ => Ok(()),
        }
    }

    fn lookup(&mut self, directory: &Node, name: WalkName<'_>) -> Result<Node, Errno> {
        match (directory, name) {
            (Node::Root, WalkName::Child(b"upload")) => Ok(Node::Upload),
            (Node::Root, WalkName::Child(b"events")) => Ok(Node::Events),
            _ => Err(Errno::ENOENT),
        }
    }

    fn describe(&self, node: &Node, _: Option<&()>) -> Entry {
        Entry {
            kind: if *node == Node::Root {
                NodeKind::Directory
            } else {
                NodeKind::File
            },
            qid_path: *node as u64 + 1,
            qid_version: 0,
            permissions: 0o600,
            size: 0,
        }
    }

    fn open(&mut self, _: &Node, _: OpenFlags) -> Result<(), Errno> {
        Ok(())
    }

    fn read(&mut self, node: &Node, _: &mut (), _: u64, count: u32) -> Result<ReadOutcome, Errno> {
        let mut ledger = self.ledger();
        match node {
            Node::Events if ledger.events.is_empty() => Ok(ReadOutcome::Pending),
            Node::Events if ledger.events[0].len() > count as usize => Err(Errno::EINVAL),
            Node::Events => Ok(ReadOutcome::Ready(ledger.events.remove(0))),
            _ => Err(Errno::EBADF),
        }
    }

    fn write(&mut self, node: &Node, _: &mut (), offset: u64, data: &[u8]) -> Result<u32, Errno> {
        let mut ledger = self.ledger();
        ledger.writes += 1;
        if *node != Node::Upload {
            return Err(Errno::EBADF);
        }
        if offset != ledger.committed.len() as u64 {
            return Err(Errno::EINVAL);
        }
        if ledger.committed.len() + data.len() > CAPACITY {
            return Err(Errno::ENOSPC);
        }
        ledger.committed.extend_from_slice(data);
        Ok(data.len() as u32)
    }

    fn write_destination(
        &mut self,
        node: &Node,
        _: &mut (),
        offset: u64,
        len: u32,
        received: u32,
    ) -> Result<Option<&mut [u8]>, Errno> {
        assert!(
            IN_PLACE,
            "a default export is never asked for a destination"
        );
        let short_by = {
            let mut ledger = self.ledger();
            ledger.destinations += 1;
            if *node != Node::Upload {
                return Err(Errno::EBADF);
            }
            if let Some((at, errno)) = ledger.refuse_at
                && received >= at
            {
                return Err(errno);
            }
            if ledger.none_at.is_some_and(|at| received >= at) {
                return Ok(None);
            }
            if offset != ledger.committed.len() as u64 {
                return Err(Errno::EINVAL);
            }
            if offset as usize + len as usize > CAPACITY {
                return Err(Errno::ENOSPC);
            }
            ledger.short_by
        };
        let start = offset as usize + received as usize;
        // More than the rest is fine: the core takes only what it needs.
        Ok(Some(&mut self.staging[start..CAPACITY - short_by]))
    }

    fn write_received(
        &mut self,
        node: &Node,
        _: &mut (),
        offset: u64,
        len: u32,
    ) -> Result<u32, Errno> {
        let mut ledger = self.ledger();
        ledger.commits += 1;
        if *node != Node::Upload || offset != ledger.committed.len() as u64 {
            return Err(Errno::EINVAL);
        }
        let range = offset as usize..offset as usize + len as usize;
        ledger.committed.extend_from_slice(&self.staging[range]);
        Ok(len)
    }

    fn readdir(&mut self, _: &Node, _: &mut (), _: u64, _: usize) -> Result<Vec<DirEntry>, Errno> {
        Err(Errno::ENOTDIR)
    }

    fn release(&mut self, _: Node, _: Option<()>) {}
}

struct Harness<const IN_PLACE: bool> {
    export: Staging<IN_PLACE>,
    connection: Connection<Staging<IN_PLACE>>,
}

/// Fid 1 is `upload` open for writing, fid 2 `events` open for reading.
fn harness<const IN_PLACE: bool>(limits: Limits) -> Harness<IN_PLACE> {
    let mut harness = Harness {
        export: Staging::<IN_PLACE>::new(),
        connection: Connection::new(ConnectionId(1), None, limits),
    };
    for (request, kind) in [
        (tversion(u16::MAX, 8192, b"9P2000.L"), 101),
        (tattach(1, 0, NOFID, b"", b""), 105),
        (twalk(2, 0, 1, &[b"upload"]), 111),
        (tlopen(3, 1, 1), 13),
        (twalk(4, 0, 2, &[b"events"]), 111),
        (tlopen(5, 2, 0), 13),
    ] {
        let replies = harness.feed(&request, usize::MAX).unwrap();
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].kind, kind, "{replies:?}");
    }
    harness
}

impl<const IN_PLACE: bool> Harness<IN_PLACE> {
    /// Offers `bytes` in pieces of at most `chunk`, as often as the
    /// connection takes them, and returns everything answered.
    fn feed(&mut self, bytes: &[u8], chunk: usize) -> Result<Vec<Frame>, Fatal> {
        let mut at = 0;
        let mut replies = Vec::new();
        while at < bytes.len() {
            let end = bytes.len().min(at.saturating_add(chunk));
            let taken = self.connection.receive(&mut self.export, &bytes[at..end])?;
            if taken == 0 {
                // Only a full output stops input; the peer reads it.
                let before = replies.len();
                replies.extend(self.take());
                self.connection.resume(&mut self.export)?;
                assert!(replies.len() > before, "no progress at byte {at}");
            }
            at += taken;
        }
        replies.extend(self.take());
        Ok(replies)
    }

    fn take(&mut self) -> Vec<Frame> {
        let replies = frames(self.connection.output());
        let length = self.connection.output().len();
        self.connection.sent(length);
        replies
    }
}

fn limits() -> Limits {
    Limits::new(8192, 512, 32, 16, 4 * 8192, 4).unwrap()
}

/// Data that would parse as a version negotiation and a flush if it were
/// ever read as requests.
fn data(len: usize, seed: u8) -> Vec<u8> {
    let mut data: Vec<u8> = (0..len)
        .map(|i| (i as u8).wrapping_mul(31) ^ seed)
        .collect();
    let decoy = [tversion(u16::MAX, 8192, b"9P2000.L"), support::tflush(7, 9)].concat();
    data[100..100 + decoy.len()].copy_from_slice(&decoy);
    data
}

fn summary(replies: &[Frame]) -> Vec<(u8, u16, Vec<u8>)> {
    replies
        .iter()
        .map(|reply| (reply.kind, reply.tag, reply.body.clone()))
        .collect()
}

#[test]
fn in_place_and_buffered_receipts_answer_alike_at_every_split() {
    let (first, second) = (data(3000, 1), data(2000, 2));
    let stream = [
        twrite(10, 1, 0, &first),
        tgetattr(11, 1),
        twrite(12, 1, 3000, &second),
        tclunk(13, 1),
    ]
    .concat();
    for chunk in [
        1,
        2,
        6,
        7,
        8,
        22,
        23,
        24,
        25,
        100,
        3022,
        3023,
        3024,
        4096,
        usize::MAX,
    ] {
        let mut buffered = harness::<false>(limits());
        let mut in_place = harness::<true>(limits());
        let expected = buffered.feed(&stream, chunk).unwrap();
        let got = in_place.feed(&stream, chunk).unwrap();
        assert_eq!(summary(&got), summary(&expected), "chunk {chunk}");
        assert_eq!(expected[0].written(), 3000);
        assert_eq!(expected[2].written(), 2000);
        let (direct, plain) = (in_place.export.ledger(), buffered.export.ledger());
        assert_eq!(
            direct.committed,
            [&first[..], &second[..]].concat(),
            "chunk {chunk}"
        );
        assert_eq!(direct.committed, plain.committed);
        // Every write went in place, each accepted once.
        assert_eq!((direct.writes, direct.commits), (0, 2), "chunk {chunk}");
        assert_eq!((plain.writes, plain.commits, plain.destinations), (2, 0, 0));
    }
}

#[test]
fn a_write_not_fit_to_receive_never_reaches_a_destination() {
    // A count that disagrees with the frame's length is malformed.
    let mut bad = twrite(10, 1, 0, &data(300, 3));
    bad[19..23].copy_from_slice(&301u32.to_le_bytes());
    // A fid never opened, and one open only for reading.
    let unopened = twrite(11, 9, 0, &data(300, 4));
    let read_only = twrite(12, 2, 0, &data(300, 5));
    for (request, errno) in [(bad, EPROTO), (unopened, EBADF), (read_only, EBADF)] {
        let mut each = harness::<true>(limits());
        let replies = each.feed(&request, 1).unwrap();
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].errno(), Some(errno));
        assert_eq!(each.export.ledger().destinations, 0);
    }
    // Revoked before the write: the buffered write answers the check.
    let mut revoked = harness::<true>(limits());
    revoked.export.ledger().revoked = true;
    let replies = revoked.feed(&twrite(13, 1, 0, &data(300, 6)), 1).unwrap();
    assert_eq!(replies[0].errno(), Some(ESTALE));
    assert_eq!(revoked.export.ledger().destinations, 0);
    // A reused tag and NOTAG are violations before any destination.
    let mut reused = harness::<true>(limits());
    assert!(
        reused
            .feed(&tread(20, 2, 0, 64), usize::MAX)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        reused.feed(&twrite(20, 1, 0, &data(300, 7)), 1),
        Err(Fatal::DuplicateTag(sophia_9p::Tag(20)))
    );
    assert_eq!(reused.export.ledger().destinations, 0);
    let mut notag = harness::<true>(limits());
    assert_eq!(
        notag.feed(&twrite(u16::MAX, 1, 0, &data(300, 8)), 1),
        Err(Fatal::NoTag { kind: 118 })
    );
    assert_eq!(notag.export.ledger().destinations, 0);
}

#[test]
fn refusal_revocation_or_rebinding_partway_drain_the_write_and_keep_the_next() {
    let stream = [twrite(10, 1, 0, &data(3000, 9)), tgetattr(11, 1)].concat();
    // The owner refuses once 1000 bytes are in.
    let mut refused = harness::<true>(limits());
    refused.export.ledger().refuse_at = Some((1000, Errno::ENOSPC));
    let replies = refused.feed(&stream, 500).unwrap();
    assert_eq!(replies[0].errno(), Some(ENOSPC));
    assert_eq!((replies[1].kind, replies[1].tag), (25, 11));
    // Revoked between reads: the check refuses the next destination.
    let mut revoked = harness::<true>(limits());
    let replies = revoked.feed(&stream[..1500], 500).unwrap();
    assert!(replies.is_empty());
    let asked = revoked.export.ledger().destinations;
    revoked.export.ledger().revoked = true;
    let replies = revoked.feed(&stream[1500..], 500).unwrap();
    assert_eq!(replies[0].errno(), Some(ESTALE));
    // The check refused before the owner was asked again.
    assert_eq!(revoked.export.ledger().destinations, asked);
    assert_eq!((replies[1].tag, replies[1].errno()), (11, Some(ESTALE)));
    // Another writer moves the cursor between reads.
    let mut rebound = harness::<true>(limits());
    assert!(rebound.feed(&stream[..1500], 500).unwrap().is_empty());
    rebound.export.ledger().committed.push(0);
    let replies = rebound.feed(&stream[1500..], 500).unwrap();
    assert_eq!(replies[0].errno(), Some(EINVAL));
    assert_eq!((replies[1].kind, replies[1].tag), (25, 11));
    for harness in [&refused, &revoked] {
        let ledger = harness.export.ledger();
        assert_eq!((ledger.commits, ledger.writes), (0, 0));
        assert!(ledger.committed.is_empty());
    }
    assert_eq!(rebound.export.ledger().committed, [0]);
}

#[test]
fn an_owner_that_stops_taking_the_data_fails_the_write_without_buffering_it() {
    let stream = [twrite(10, 1, 0, &data(3000, 10)), tgetattr(11, 1)].concat();
    let mut harness = harness::<true>(limits());
    harness.export.ledger().none_at = Some(1000);
    let replies = harness.feed(&stream, 500).unwrap();
    assert_eq!(replies[0].errno(), Some(EIO));
    assert_eq!((replies[1].kind, replies[1].tag), (25, 11));
    let ledger = harness.export.ledger();
    assert_eq!((ledger.commits, ledger.writes), (0, 0));
}

#[test]
fn a_short_destination_is_refused_and_a_long_one_takes_only_the_write() {
    let mut short = harness::<true>(limits());
    short.export.ledger().short_by = CAPACITY - 2999;
    let stream = [twrite(10, 1, 0, &data(3000, 11)), tgetattr(11, 1)].concat();
    let replies = short.feed(&stream, 700).unwrap();
    assert_eq!(replies[0].errno(), Some(EIO));
    assert_eq!((replies[1].kind, replies[1].tag), (25, 11));
    assert_eq!(short.export.ledger().commits, 0);
    // The staging offered runs to its end; the next request is untouched.
    let mut long = harness::<true>(limits());
    let replies = long.feed(&stream, 700).unwrap();
    assert_eq!(replies[0].written(), 3000);
    assert_eq!((replies[1].kind, replies[1].tag), (25, 11));
    assert_eq!(long.export.ledger().committed, data(3000, 11));
}

#[test]
fn the_reply_room_is_kept_and_the_write_is_accepted_once() {
    // Unsent output holds one message: a read reply that would leave no room
    // for the write's must wait for it.
    let mut harness = harness::<true>(Limits::new(8192, 512, 32, 16, 8192, 4).unwrap());
    assert!(
        harness
            .feed(&tread(20, 2, 0, 8175), usize::MAX)
            .unwrap()
            .is_empty()
    );
    let write = twrite(10, 1, 0, &data(3000, 12));
    assert!(harness.feed(&write[..1000], 1000).unwrap().is_empty());
    harness.export.ledger().events.push(vec![7; 8175]);
    harness
        .connection
        .retry_waiting(&mut harness.export)
        .unwrap();
    assert!(
        harness.connection.output().is_empty(),
        "the read took the write's room"
    );
    let replies = harness.feed(&write[1000..], 1000).unwrap();
    assert_eq!((replies.len(), replies[0].written()), (1, 3000));
    for _ in 0..3 {
        harness.connection.resume(&mut harness.export).unwrap();
    }
    assert_eq!(harness.export.ledger().commits, 1);
    harness
        .connection
        .retry_waiting(&mut harness.export)
        .unwrap();
    let replies = harness.take();
    assert_eq!((replies[0].kind, replies[0].tag), (117, 20));
    assert_eq!(harness.export.ledger().commits, 1);
}

#[test]
fn a_receipt_cut_off_commits_nothing() {
    let mut harness = harness::<true>(limits());
    let write = twrite(10, 1, 0, &data(3000, 13));
    assert!(harness.feed(&write[..2000], 400).unwrap().is_empty());
    assert!(harness.export.ledger().destinations > 0);
    harness.connection.close(&mut harness.export);
    let ledger = harness.export.ledger();
    assert_eq!((ledger.commits, ledger.writes), (0, 0));
    assert!(ledger.committed.is_empty());
}

#[test]
fn real_sockets_receive_in_place_across_split_writes() {
    let (client, server_side) = UnixStream::pair().unwrap();
    let export = Staging::<true>::new();
    let ledger = Arc::clone(&export.ledger);
    let mut server = Server::new(export, limits()).unwrap();
    assert!(server.adopt(server_side).is_ok());
    let (first, second) = (data(5000, 14), data(4000, 15));
    let setup = [
        tversion(u16::MAX, 8192, b"9P2000.L"),
        tattach(1, 0, NOFID, b"", b""),
        twalk(2, 0, 1, &[b"upload"]),
        tlopen(3, 1, 1),
    ];
    let stream = [
        twrite(10, 1, 0, &first),
        twrite(11, 1, 5000, &second),
        tgetattr(12, 1),
    ]
    .concat();
    let writer = std::thread::spawn(move || {
        let mut client = client;
        for request in &setup {
            client.write_all(request).unwrap();
        }
        // Splits inside a header, at its end, and inside the data, with the
        // next request coalesced behind the first write's tail.
        for piece in [
            &stream[..10],
            &stream[10..23],
            &stream[23..2600],
            &stream[2600..5100],
            &stream[5100..],
        ] {
            client.write_all(piece).unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut replies = Vec::new();
        let mut buffer = [0; 4096];
        while frames(&replies).len() < 7 {
            let count = client.read(&mut buffer).unwrap();
            assert!(count > 0, "the server closed");
            replies.extend_from_slice(&buffer[..count]);
        }
        frames(&replies)
    });
    while !writer.is_finished() {
        server.turn(Some(Duration::from_millis(20))).unwrap();
    }
    let replies = writer.join().unwrap();
    assert_eq!(replies[4].written(), 5000);
    assert_eq!(replies[5].written(), 4000);
    assert_eq!((replies[6].kind, replies[6].tag), (25, 12));
    let ledger = ledger.lock().unwrap();
    assert_eq!(ledger.committed, [&first[..], &second[..]].concat());
    assert_eq!((ledger.writes, ledger.commits), (0, 2));
}

/// Codex's regression (REVIEW-CODEX-direct-decline-regression.rs).
#[test]
fn an_initial_decline_keeps_this_write_buffered() {
    let mut h = harness::<true>(limits());
    let write = twrite(40, 1, 0, &data(3000, 21));
    h.export.ledger().none_at = Some(0);
    assert!(h.feed(&write[..23], 23).unwrap().is_empty());
    assert_eq!(h.export.ledger().destinations, 1);
    // Staging becomes available after this request was already declined.
    h.export.ledger().none_at = None;
    let replies = h.feed(&write[23..], 500).unwrap();
    assert_eq!(replies[0].written(), 3000);
    let ledger = h.export.ledger();
    assert_eq!(ledger.committed, data(3000, 21));
    assert_eq!(
        (ledger.writes, ledger.commits),
        (1, 0),
        "a receipt declined at its header must stay buffered"
    );
    assert_eq!(
        ledger.destinations, 1,
        "do not offer the same declined receipt again"
    );
}

#[test]
fn a_write_after_a_declined_one_is_offered_afresh() {
    let mut h = harness::<true>(limits());
    let (first, second) = (data(3000, 22), data(2000, 23));
    let stream = [twrite(41, 1, 0, &first), twrite(42, 1, 3000, &second)].concat();
    h.export.ledger().none_at = Some(0);
    assert!(h.feed(&stream[..23], 23).unwrap().is_empty());
    h.export.ledger().none_at = None;
    let replies = h.feed(&stream[23..], 500).unwrap();
    assert_eq!((replies[0].written(), replies[1].written()), (3000, 2000));
    let ledger = h.export.ledger();
    assert_eq!(ledger.committed, [&first[..], &second[..]].concat());
    // The first was buffered; the second, offered afresh, went in place.
    assert_eq!((ledger.writes, ledger.commits), (1, 1));
}

/// Codex's control (REVIEW-CODEX-direct-commit-revocation.rs): the epoch
/// ends after the last read and before the write is accepted.
#[test]
fn revocation_after_the_last_read_prevents_commit() {
    let mut h = harness::<true>(limits());
    let payload = data(3000, 22);
    let write = twrite(41, 1, 0, &payload);
    assert!(h.feed(&write[..23], 23).unwrap().is_empty());
    match h.connection.in_place_target(&mut h.export).unwrap() {
        sophia_9p::InPlaceTarget::Destination(destination) => {
            destination.copy_from_slice(&payload);
        }
        sophia_9p::InPlaceTarget::Discard(_) => panic!("write unexpectedly refused"),
    }
    // Like an epoch expiring after read() returns but before its receipt commits.
    h.export.ledger().revoked = true;
    h.connection
        .received_in_place(&mut h.export, payload.len())
        .unwrap();
    let replies = h.take();
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].errno(), Some(ESTALE));
    let ledger = h.export.ledger();
    assert_eq!((ledger.writes, ledger.commits), (0, 0));
    assert!(ledger.committed.is_empty());
}

/// A write whose header and data are both waiting is read, received in
/// place and answered in a single server turn: the header and the data are
/// read apart, back to back.
#[test]
fn a_waiting_write_is_received_in_place_in_one_turn() {
    let (mut client, server_side) = UnixStream::pair().unwrap();
    let export = Staging::<true>::new();
    let ledger = Arc::clone(&export.ledger);
    let mut server = Server::new(export, limits()).unwrap();
    assert!(server.adopt(server_side).is_ok());
    client.set_nonblocking(true).unwrap();
    let mut buffer = [0; 4096];
    // Setup takes as many turns as it takes.
    for request in [
        tversion(u16::MAX, 8192, b"9P2000.L"),
        tattach(1, 0, NOFID, b"", b""),
        twalk(2, 0, 1, &[b"upload"]),
        tlopen(3, 1, 1),
    ] {
        client.write_all(&request).unwrap();
        let mut turns = 0;
        loop {
            server.turn(Some(Duration::from_millis(10))).unwrap();
            if let Ok(count) = client.read(&mut buffer) {
                assert_eq!(frames(&buffer[..count]).len(), 1);
                break;
            }
            turns += 1;
            assert!(turns < 100, "setup request unanswered");
        }
    }
    let payload = data(4000, 30);
    client.write_all(&twrite(10, 1, 0, &payload)).unwrap();
    server.turn(Some(Duration::from_millis(100))).unwrap();
    let count = client.read(&mut buffer).expect("answered in the same turn");
    let replies = frames(&buffer[..count]);
    assert_eq!((replies.len(), replies[0].written()), (1, 4000));
    let ledger = ledger.lock().unwrap();
    assert_eq!(ledger.committed, payload);
    assert_eq!((ledger.writes, ledger.commits), (0, 1));
}

/// However much is waiting, one turn reads at most one message's worth for a
/// connection: a read after a short one asks only for what the turn has left.
#[test]
fn a_turn_reads_at_most_one_message_worth() {
    let (mut client, server_side) = UnixStream::pair().unwrap();
    let mut server = Server::new(Staging::<false>::new(), limits()).unwrap();
    assert!(server.adopt(server_side).is_ok());
    client.set_nonblocking(true).unwrap();
    let mut buffer = vec![0; 1 << 16];
    client
        .write_all(&tversion(u16::MAX, 8192, b"9P2000.L"))
        .unwrap();
    for _ in 0..100 {
        server.turn(Some(Duration::from_millis(10))).unwrap();
        if client.read(&mut buffer).is_ok() {
            break;
        }
    }
    // Clunks of a fid that does not exist: 11 bytes each, each answered with
    // an 11-byte error, so a turn's replies count the bytes it read. 8192 is
    // no multiple of 11, so turns after the first start with a partial one.
    let clunks: Vec<u8> = (0..2000u16).flat_map(|tag| tclunk(tag, 77)).collect();
    client.write_all(&clunks).unwrap();
    let mut answered = 0;
    let mut turns = 0;
    while answered < 2000 {
        server.turn(Some(Duration::from_millis(10))).unwrap();
        let replies = match client.read(&mut buffer) {
            Ok(count) => frames(&buffer[..count]).len(),
            Err(_) => 0,
        };
        assert!(
            replies * 11 <= 8192 + 11,
            "one turn read {} bytes of requests",
            replies * 11
        );
        answered += replies;
        turns += 1;
        assert!(turns < 100, "the requests were not all answered");
    }
}
