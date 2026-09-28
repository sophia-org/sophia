//! A scripted `sophia_shell_fs_v1` peer: a small 9P2000.L server written
//! from the file contract (spec/sophia-shell-files.md) and the 9P2000.L
//! framing (size[4] type[1] tag[2], little-endian), with no server core.
//!
//! The connect handshake runs on its own thread, because
//! `ShellConnection::connect_files` blocks until negotiation ends. After
//! that the peer runs in lockstep on the test's own thread: the client's
//! `poll_io` never blocks, so a test alternates `poll_io` with
//! [`Peer::pump`] and decides exactly which replies each pass can see.
//!
//! [`Peer::pump`] answers the plumbing on its own -- walks, opens, clunks,
//! `transaction` writes, `ack` writes and snapshot object reads -- and logs
//! each. It never answers a `submit` write (each waits in
//! [`Peer::submits`] for the test to answer) and holds the one outstanding
//! `events` read until the test has events for it ([`Peer::push`]).

use std::collections::{HashMap, VecDeque};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::{Duration, Instant};

use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::{ContentGrant, ContentLimits, ShellV1ServerWelcome};

/// The connection epoch every record of the scripted attach carries.
pub const EPOCH: u64 = 41;
/// The content grant epoch the scripted `limits` object names.
pub const CONTENT_GRANT_EPOCH: u64 = 9;
/// The largest `msize` the peer negotiates: the client's default offer.
pub const MSIZE: u32 = 65536;
/// Every wait on the peer's side is bounded by this.
pub const WAIT: Duration = Duration::from_secs(2);

// 9P2000.L message types (diod protocol.md).
const RLERROR: u8 = 7;
const TLOPEN: u8 = 12;
const RLOPEN: u8 = 13;
const TVERSION: u8 = 100;
const RVERSION: u8 = 101;
const TATTACH: u8 = 104;
const RATTACH: u8 = 105;
const TFLUSH: u8 = 108;
const RFLUSH: u8 = 109;
const TWALK: u8 = 110;
const RWALK: u8 = 111;
const TREAD: u8 = 116;
const RREAD: u8 = 117;
const TWRITE: u8 = 118;
const RWRITE: u8 = 119;
const TCLUNK: u8 = 120;
const RCLUNK: u8 = 121;

const EIO: u32 = 5;
const ENOSYS: u32 = 38;

/// What the scripted session grants.
#[derive(Clone, Copy, Debug)]
pub struct Profile {
    pub role: &'static str,
    pub revision: u16,
    pub capabilities: u64,
    pub limits_published: bool,
}

impl Profile {
    pub fn welcome(&self) -> ShellV1ServerWelcome {
        ShellV1ServerWelcome {
            selected_revision: self.revision,
            connection_epoch: EPOCH,
            capabilities: self.capabilities,
            max_descriptors: 16,
            max_label_bytes: 128,
            max_pending_activations: 16,
        }
    }
}

/// A request the peer has not answered yet: its tag and the written bytes.
#[derive(Clone, Debug)]
pub struct Held {
    pub tag: u16,
    pub data: Vec<u8>,
}

/// A snapshot object as the node currently publishes it. Reads return at
/// most `chunk` bytes, so a positive short read is easy to script.
#[derive(Clone, Debug)]
pub struct Object {
    pub qid: u64,
    pub bytes: Vec<u8>,
    pub chunk: usize,
}

/// One object read as the peer answered it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectRead {
    pub node: String,
    pub offset: u64,
    pub count: u32,
    pub returned: usize,
}

/// One acknowledgement as the peer received it, with how many object reads
/// it had already answered by then.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ack {
    pub sequence: u64,
    pub after_reads: usize,
}

/// One step of the client's traffic the peer answered, in arrival order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Logged {
    Ack(u64),
    Walk(String),
    Open(String),
    ObjectRead { node: String, returned: usize },
}

struct HeldRead {
    tag: u16,
    fid: u32,
    offset: u64,
    count: u32,
}

pub struct Peer {
    stream: UnixStream,
    inbuf: Vec<u8>,
    closed: bool,
    profile: Profile,
    handshaking: bool,
    handshake_done: bool,
    fids: HashMap<u32, String>,
    /// Opened object fids: the object pinned at open time.
    pins: HashMap<u32, Object>,
    next_sequence: u64,
    /// Bytes of `events` already delivered: the next read's offset.
    journal_len: u64,
    pending_events: Vec<u8>,
    /// The outstanding `events` read: tag, offset and count.
    pub events_read: Option<(u16, u64, u32)>,
    /// `submit` writes waiting for the test's answer.
    pub submits: VecDeque<Held>,
    /// When set, `transaction` writes wait in `held_transactions` instead
    /// of being answered.
    pub hold_transactions: bool,
    pub held_transactions: VecDeque<Held>,
    /// Every record written to `transaction`, in order.
    pub transactions: Vec<Vec<u8>>,
    pub acks: Vec<Ack>,
    /// The count to answer `ack` writes with; the written length when unset.
    pub ack_reply: Option<u32>,
    /// Every walked name, in order.
    pub walks: Vec<String>,
    /// Every open: node and the qid path it reported.
    pub opens: Vec<(String, u64)>,
    /// The objects each snapshot node currently publishes.
    pub objects: HashMap<String, Object>,
    /// When set, clunks are never answered: each keeps its tag in use.
    pub hold_clunks: bool,
    pub held_clunks: usize,
    /// When set, object reads wait until [`Peer::release_object_reads`].
    pub hold_object_reads: bool,
    held_object_reads: VecDeque<HeldRead>,
    pub object_reads: Vec<ObjectRead>,
    /// Every ack, walk, open and object read, in arrival order.
    pub log: Vec<Logged>,
    /// When set, `ack` writes are logged but their replies wait in
    /// `held_replies` until [`Peer::release_replies`].
    pub hold_ack_replies: bool,
    /// When set, walks to `transaction` wait the same way.
    pub hold_transaction_walks: bool,
    /// Replies withheld by the two flags above: type, tag and body.
    pub held_replies: Vec<(u8, u16, Vec<u8>)>,
    /// The `Negotiate` record the handshake received.
    pub negotiate: Option<Vec<u8>>,
    /// Bytes written to each `upload/N` slot, at the offsets written.
    pub uploads: HashMap<String, Vec<u8>>,
    limits: Vec<u8>,
    edit_bootstrap: fn(&mut Vec<u8>),
    /// Replies collected for one write, between [`Peer::begin_batch`] and
    /// [`Peer::end_batch`].
    batch: Option<Vec<u8>>,
}

fn qid(kind: u8, path: u64) -> [u8; 13] {
    let mut qid = [0; 13];
    qid[0] = kind;
    qid[5..].copy_from_slice(&path.to_le_bytes());
    qid
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

impl Peer {
    /// Accepts one connection on `listener` and serves the whole connect
    /// handshake: version, attach, `api`, the fixed nodes, the `Negotiate`
    /// submission (answered by `Submitted` then `Negotiated`), their acks,
    /// and the `limits` object when the profile publishes it.
    pub fn handshake(listener: UnixListener, profile: Profile) -> Self {
        let limits = ContentLimits::prototype(ContentGrant {
            connection_epoch: EPOCH,
            content_grant_epoch: CONTENT_GRANT_EPOCH,
        });
        Self::handshake_with_limits(listener, profile, limits)
    }

    /// As [`Self::handshake`], publishing `limits` as the `limits` object.
    pub fn handshake_with_limits(
        listener: UnixListener,
        profile: Profile,
        limits: ContentLimits,
    ) -> Self {
        Self::handshake_with_edits(listener, profile, limits, |_| {}, |_| {})
    }

    /// Corrupts encoded bootstrap inputs for connection refusal tests. The
    /// normal peer still encodes valid records before either edit runs.
    pub fn handshake_with_edits(
        listener: UnixListener,
        profile: Profile,
        limits: ContentLimits,
        edit_bootstrap: fn(&mut Vec<u8>),
        edit_limits: fn(&mut Vec<u8>),
    ) -> Self {
        let deadline = Instant::now() + WAIT;
        listener.set_nonblocking(true).unwrap();
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "the client never connected");
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("accept: {error}"),
            }
        };
        let mut limits = encode_shell_file_limits(
            ShellFileHeader {
                kind: ShellFileKind::Limits,
                connection_epoch: EPOCH,
                submission_id: 0,
                sequence: 0,
            },
            limits,
        )
        .unwrap();
        edit_limits(&mut limits);
        let mut peer = Self {
            stream,
            inbuf: Vec::new(),
            closed: false,
            profile,
            handshaking: true,
            handshake_done: false,
            fids: HashMap::new(),
            pins: HashMap::new(),
            next_sequence: 1,
            journal_len: 0,
            pending_events: Vec::new(),
            events_read: None,
            submits: VecDeque::new(),
            hold_transactions: false,
            held_transactions: VecDeque::new(),
            transactions: Vec::new(),
            acks: Vec::new(),
            ack_reply: None,
            walks: Vec::new(),
            opens: Vec::new(),
            objects: HashMap::new(),
            hold_clunks: false,
            held_clunks: 0,
            hold_object_reads: false,
            held_object_reads: VecDeque::new(),
            object_reads: Vec::new(),
            log: Vec::new(),
            hold_ack_replies: false,
            hold_transaction_walks: false,
            held_replies: Vec::new(),
            negotiate: None,
            uploads: HashMap::new(),
            limits,
            edit_bootstrap,
            batch: None,
        };
        let (kind, tag, body) = peer.frame(WAIT).expect("Tversion");
        assert_eq!((kind, tag), (TVERSION, u16::MAX), "Tversion first");
        let msize = u32_at(&body, 0).min(MSIZE);
        let mut version = msize.to_le_bytes().to_vec();
        version.extend_from_slice(&8u16.to_le_bytes());
        version.extend_from_slice(b"9P2000.L");
        peer.send(RVERSION, tag, &version);
        while !peer.handshake_done {
            let Some((kind, tag, body)) = peer.frame(WAIT) else {
                // Negative negotiation tests may reject the welcome and
                // disconnect before fetching Limits. A timeout still fails.
                assert!(peer.closed, "a handshake request timed out");
                break;
            };
            peer.handle(kind, tag, body);
        }
        peer.handshaking = false;
        peer
    }

    /// Answers every request the client has already written, without
    /// waiting, then delivers pending events if a read is outstanding.
    pub fn pump(&mut self) {
        while let Some((kind, tag, body)) = self.frame(Duration::ZERO) {
            self.handle(kind, tag, body);
        }
        self.flush_events();
    }

    /// Clears the logs the handshake filled.
    pub fn reset_logs(&mut self) {
        self.transactions.clear();
        self.acks.clear();
        self.walks.clear();
        self.opens.clear();
        self.object_reads.clear();
        self.log.clear();
    }

    /// Sends every withheld reply, in arrival order, and stops withholding.
    pub fn release_replies(&mut self) {
        self.hold_ack_replies = false;
        self.hold_transaction_walks = false;
        for (kind, tag, body) in std::mem::take(&mut self.held_replies) {
            self.send(kind, tag, &body);
        }
    }

    pub fn walk_count(&self, name: &str) -> usize {
        self.walks.iter().filter(|walked| *walked == name).count()
    }

    /// The sequence the next event will carry.
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    /// One whole event record with the next sequence, in this attach's epoch.
    pub fn event(&mut self, kind: ShellFileKind, body: &[u8]) -> Vec<u8> {
        self.event_in(EPOCH, kind, body)
    }

    /// One whole event record with the next sequence, in `epoch`.
    pub fn event_in(&mut self, epoch: u64, kind: ShellFileKind, body: &[u8]) -> Vec<u8> {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        encode_shell_file_record(
            ShellFileHeader {
                kind,
                connection_epoch: epoch,
                submission_id: 0,
                sequence,
            },
            body,
        )
        .unwrap()
    }

    pub fn submitted(&mut self, submission_id: u64, candidate_kind: ShellFileKind) -> Vec<u8> {
        let body = encode_shell_file_submitted_body(ShellFileSubmitted {
            submission_id,
            candidate_kind,
        })
        .unwrap();
        self.event(ShellFileKind::Submitted, &body)
    }

    pub fn published(&mut self, object: ShellFileKind, generation: u64, qid: u64) -> Vec<u8> {
        let body = encode_shell_file_object_published_body(ShellFileObjectPublished {
            object,
            generation,
            qid,
        })
        .unwrap();
        self.event(ShellFileKind::ObjectPublished, &body)
    }

    /// Appends bytes to the journal; the outstanding `events` read gets
    /// them at the next [`Self::flush_events`] (every `pump` ends with one).
    pub fn push(&mut self, bytes: Vec<u8>) {
        self.pending_events.extend(bytes);
    }

    /// Answers the outstanding `events` read with every pending byte that
    /// fits it, in one `Rread`.
    pub fn flush_events(&mut self) {
        if self.pending_events.is_empty() || self.closed {
            return;
        }
        let Some((tag, offset, count)) = self.events_read.take() else {
            return;
        };
        assert_eq!(offset, self.journal_len, "events read at the journal tail");
        let size = self.pending_events.len().min(count as usize);
        let data: Vec<u8> = self.pending_events.drain(..size).collect();
        self.journal_len += size as u64;
        self.rread(tag, &data);
    }

    pub fn answer_write(&mut self, tag: u16, count: u32) {
        self.send(RWRITE, tag, &count.to_le_bytes());
    }

    pub fn answer_error(&mut self, tag: u16, errno: u32) {
        self.send(RLERROR, tag, &errno.to_le_bytes());
    }

    /// Answers every held object read, and stops holding new ones.
    pub fn release_object_reads(&mut self) {
        self.hold_object_reads = false;
        while let Some(read) = self.held_object_reads.pop_front() {
            self.serve_object_read(read);
        }
    }

    /// Collects every reply sent from here on, until [`Self::end_batch`]
    /// writes them all at once: the client then reads them in one drain.
    pub fn begin_batch(&mut self) {
        self.batch = Some(Vec::new());
    }

    pub fn end_batch(&mut self) {
        if let Some(bytes) = self.batch.take() {
            self.write(&bytes);
        }
    }

    /// Closes the peer's end: the client sees end of file.
    pub fn close(&mut self) {
        self.closed = true;
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }

    fn handle(&mut self, kind: u8, tag: u16, body: Vec<u8>) {
        match kind {
            TATTACH => {
                self.fids.insert(u32_at(&body, 0), String::new());
                self.send(RATTACH, tag, &qid(0x80, 1));
            }
            TWALK => {
                let (fid, newfid, names) = (u32_at(&body, 0), u32_at(&body, 4), u16_at(&body, 8));
                let mut at = 10;
                let mut path = Vec::new();
                for _ in 0..names {
                    let length = usize::from(u16_at(&body, at));
                    path.push(String::from_utf8(body[at + 2..at + 2 + length].to_vec()).unwrap());
                    at += 2 + length;
                }
                let node = if path.is_empty() {
                    self.fids.get(&fid).cloned().unwrap_or_default()
                } else {
                    path.join("/")
                };
                if !path.is_empty() {
                    self.walks.push(node.clone());
                    self.log.push(Logged::Walk(node.clone()));
                }
                let mut reply = names.to_le_bytes().to_vec();
                for _ in 0..names {
                    reply.extend_from_slice(&self.qid_of(&node));
                }
                let held = self.hold_transaction_walks && node == "transaction";
                self.fids.insert(newfid, node);
                if held {
                    self.held_replies.push((RWALK, tag, reply));
                } else {
                    self.send(RWALK, tag, &reply);
                }
            }
            TLOPEN => {
                let fid = u32_at(&body, 0);
                let node = self.fids.get(&fid).cloned().unwrap_or_default();
                let reported = self.qid_of(&node);
                if let Some(object) = self.objects.get(&node) {
                    self.pins.insert(fid, object.clone());
                }
                self.log.push(Logged::Open(node.clone()));
                self.opens.push((node, u64_at(&reported, 5)));
                let mut reply = reported.to_vec();
                reply.extend_from_slice(&0u32.to_le_bytes());
                self.send(RLOPEN, tag, &reply);
            }
            TREAD => {
                let (fid, offset, count) = (u32_at(&body, 0), u64_at(&body, 4), u32_at(&body, 12));
                let node = self.fids.get(&fid).cloned().unwrap_or_default();
                match node.as_str() {
                    "api" => {
                        let line = format!(
                            "sophia-shell-files version=1 role={} epoch={EPOCH} fd_transfer=none\n",
                            self.profile.role
                        );
                        self.serve_bytes(tag, line.as_bytes(), offset, count as usize);
                    }
                    "limits" => {
                        let limits = self.limits.clone();
                        let served = self.serve_bytes(tag, &limits, offset, count as usize);
                        if self.handshaking && served == 0 {
                            self.handshake_done = true;
                        }
                    }
                    "events" => {
                        assert!(self.events_read.is_none(), "one events read at a time");
                        self.events_read = Some((tag, offset, count));
                        if self.handshaking {
                            self.negotiated();
                        }
                    }
                    _ if self.pins.contains_key(&fid) => {
                        let read = HeldRead {
                            tag,
                            fid,
                            offset,
                            count,
                        };
                        if self.hold_object_reads {
                            self.held_object_reads.push_back(read);
                        } else {
                            self.serve_object_read(read);
                        }
                    }
                    _ => self.answer_error(tag, EIO),
                }
            }
            TWRITE => {
                let fid = u32_at(&body, 0);
                let count = u32_at(&body, 12);
                let data = body[16..].to_vec();
                assert_eq!(data.len(), count as usize, "Twrite count");
                let node = self.fids.get(&fid).cloned().unwrap_or_default();
                match node.as_str() {
                    "transaction" if self.handshaking => {
                        decode_shell_file_negotiate(&data).expect("a Negotiate record");
                        self.negotiate = Some(data);
                        self.answer_write(tag, count);
                    }
                    "transaction" => {
                        self.transactions.push(data.clone());
                        if self.hold_transactions {
                            self.held_transactions.push_back(Held { tag, data });
                        } else {
                            self.answer_write(tag, count);
                        }
                    }
                    "submit" if self.handshaking => {
                        let submit = decode_shell_file_submit(&data).expect("a submit");
                        assert_eq!((submit.connection_epoch, submit.submission_id), (EPOCH, 1));
                        self.answer_write(tag, count);
                    }
                    "submit" => self.submits.push_back(Held { tag, data }),
                    slot if slot.starts_with("upload/") => {
                        // Slot bytes land at the written offset, in order.
                        let offset = u64_at(&body, 4) as usize;
                        let upload = self.uploads.entry(node.clone()).or_default();
                        assert_eq!(upload.len(), offset, "slot writes arrive in order");
                        upload.extend_from_slice(&data);
                        self.answer_write(tag, count);
                    }
                    "ack" => {
                        let ack = decode_shell_file_ack(&data).expect("an ack");
                        assert_eq!(ack.connection_epoch, EPOCH, "ack epoch");
                        self.acks.push(Ack {
                            sequence: ack.sequence,
                            after_reads: self.object_reads.len(),
                        });
                        self.log.push(Logged::Ack(ack.sequence));
                        let reply = self.ack_reply.unwrap_or(count);
                        if self.hold_ack_replies {
                            self.held_replies
                                .push((RWRITE, tag, reply.to_le_bytes().to_vec()));
                        } else {
                            self.answer_write(tag, reply);
                        }
                        if self.handshaking && ack.sequence == 2 && !self.profile.limits_published {
                            self.handshake_done = true;
                        }
                    }
                    _ => self.answer_error(tag, EIO),
                }
            }
            TCLUNK => {
                let fid = u32_at(&body, 0);
                self.fids.remove(&fid);
                self.pins.remove(&fid);
                if self.hold_clunks {
                    self.held_clunks += 1;
                } else {
                    self.send(RCLUNK, tag, &[]);
                }
            }
            TFLUSH => self.send(RFLUSH, tag, &[]),
            _ => self.answer_error(tag, ENOSYS),
        }
    }

    /// The handshake's journal: `Submitted` for the `Negotiate` submission,
    /// then `Negotiated`, in one read.
    fn negotiated(&mut self) {
        let submitted = self.submitted(1, ShellFileKind::Negotiate);
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        let negotiated = encode_shell_file_negotiated(
            ShellFileHeader {
                kind: ShellFileKind::Negotiated,
                connection_epoch: EPOCH,
                submission_id: 0,
                sequence,
            },
            ShellFileNegotiated {
                welcome: self.profile.welcome(),
                limits_published: self.profile.limits_published,
            },
        )
        .unwrap();
        let mut bootstrap = submitted;
        bootstrap.extend(negotiated);
        (self.edit_bootstrap)(&mut bootstrap);
        self.push(bootstrap);
        self.flush_events();
    }

    fn qid_of(&self, node: &str) -> [u8; 13] {
        if let Some(object) = self.objects.get(node) {
            return qid(0, object.qid);
        }
        let path = match node {
            "" => return qid(0x80, 1),
            "api" => 10,
            "events" => 11,
            "submit" => 12,
            "ack" => 13,
            "transaction" => 14,
            "limits" => 15,
            slot if slot.starts_with("upload/") => {
                20 + slot["upload/".len()..].parse::<u64>().unwrap_or(0)
            }
            _ => 99,
        };
        qid(0, path)
    }

    fn serve_bytes(&mut self, tag: u16, bytes: &[u8], offset: u64, count: usize) -> usize {
        let start = (offset as usize).min(bytes.len());
        let end = start + count.min(bytes.len() - start);
        self.rread(tag, &bytes[start..end]);
        end - start
    }

    fn serve_object_read(&mut self, read: HeldRead) {
        let object = self.pins[&read.fid].clone();
        let node = self.fids.get(&read.fid).cloned().unwrap_or_default();
        let count = (read.count as usize).min(object.chunk);
        let returned = self.serve_bytes(read.tag, &object.bytes, read.offset, count);
        self.log.push(Logged::ObjectRead {
            node: node.clone(),
            returned,
        });
        self.object_reads.push(ObjectRead {
            node,
            offset: read.offset,
            count: read.count,
            returned,
        });
    }

    fn rread(&mut self, tag: u16, data: &[u8]) {
        let mut body = (data.len() as u32).to_le_bytes().to_vec();
        body.extend_from_slice(data);
        self.send(RREAD, tag, &body);
    }

    fn send(&mut self, kind: u8, tag: u16, body: &[u8]) {
        if self.closed {
            return;
        }
        let mut frame = ((7 + body.len()) as u32).to_le_bytes().to_vec();
        frame.push(kind);
        frame.extend_from_slice(&tag.to_le_bytes());
        frame.extend_from_slice(body);
        match &mut self.batch {
            Some(batch) => batch.extend_from_slice(&frame),
            None => self.write(&frame),
        }
    }

    fn write(&mut self, frame: &[u8]) {
        if self.closed {
            return;
        }
        self.stream.set_nonblocking(false).unwrap();
        self.stream.set_write_timeout(Some(WAIT)).unwrap();
        if self.stream.write_all(frame).is_err() {
            self.closed = true;
        }
    }

    /// The next whole request, waiting at most `wait` (not at all for zero).
    fn frame(&mut self, wait: Duration) -> Option<(u8, u16, Vec<u8>)> {
        let deadline = Instant::now() + wait;
        loop {
            if self.inbuf.len() >= 7 {
                let size = u32_at(&self.inbuf, 0) as usize;
                assert!(size >= 7, "a request frame shorter than its header");
                if self.inbuf.len() >= size {
                    let frame: Vec<u8> = self.inbuf.drain(..size).collect();
                    return Some((frame[4], u16_at(&frame, 5), frame[7..].to_vec()));
                }
            }
            if self.closed {
                return None;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                self.stream.set_nonblocking(true).unwrap();
            } else {
                self.stream.set_nonblocking(false).unwrap();
                self.stream.set_read_timeout(Some(left)).unwrap();
            }
            let mut chunk = [0u8; 16384];
            match self.stream.read(&mut chunk) {
                Ok(0) => {
                    self.closed = true;
                    return None;
                }
                Ok(count) => self.inbuf.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error)
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    if left.is_zero() || Instant::now() >= deadline {
                        return None;
                    }
                }
                Err(_) => {
                    self.closed = true;
                    return None;
                }
            }
        }
    }
}
