//! A transparent 9P2000.L tap between the production shell client and the
//! production shell transport: it forwards every byte in order, in both
//! directions, and records what crossed the wire so a fixture can prove
//! what was actually issued and answered -- a `submit` write reaching the
//! transport, the transport's reply to it, each `Submitted` the journal
//! handed out, each acknowledgement and each snapshot object read.
//!
//! It never reorders, rewrites or invents a frame. It can only delay a
//! direction, always from one frame onward and in order, which is what a
//! slow socket may do anyway:
//! - [`Tap::hold_after_submit`] stops delivering server-to-client bytes once
//!   a `submit` write has been forwarded, and never delivers them: the
//!   client then sees exactly the connection a revocation left it.
//! - [`Tap::hold_up_from_submit`] delays client-to-server bytes from a
//!   `submit` write onward until [`Tap::release_up`].
//! - [`Tap::hold_down_from_ack_reply`] delays server-to-client bytes from the
//!   next reply to an `ack` write onward until [`Tap::release_down`].
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use sophia_protocol::shell_files::*;

const RLERROR: u8 = 7;
const TLOPEN: u8 = 12;
const RLOPEN: u8 = 13;
const TATTACH: u8 = 104;
const TWALK: u8 = 110;
const TREAD: u8 = 116;
const RREAD: u8 = 117;
const TWRITE: u8 = 118;
const RWRITE: u8 = 119;

/// One thing the tap saw cross the wire, in order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Seen {
    /// A record the client staged in `transaction`.
    Staged(Vec<u8>),
    /// A `submit` write the client issued, forwarded to the transport.
    Submit { tag: u16, bytes: Vec<u8> },
    /// The transport's answer to a `submit` write: `Ok(count)` or the errno.
    SubmitReply { tag: u16, reply: Result<u32, u32> },
    /// An acknowledgement the client wrote.
    Ack(u64),
    /// One whole `events` record the transport returned.
    Event {
        sequence: u64,
        kind: ShellFileKind,
        submitted: Option<ShellFileSubmitted>,
        published: Option<ShellFileObjectPublished>,
    },
    /// A walk the client issued, by the path it names.
    Walk(String),
    /// The transport's answer to an open: `Ok(())` or the errno.
    OpenReply {
        node: String,
        reply: Result<(), u32>,
    },
    /// A snapshot object read the transport answered.
    ObjectRead {
        node: String,
        offset: u64,
        returned: usize,
    },
}

struct Request {
    kind: u8,
    node: String,
    offset: u64,
}

pub struct Tap {
    listener: UnixListener,
    upstream: PathBuf,
    down: Option<UnixStream>,
    up: Option<UnixStream>,
    down_in: Vec<u8>,
    up_in: Vec<u8>,
    to_up: Vec<u8>,
    to_down: Vec<u8>,
    down_closed: bool,
    up_closed: bool,
    requests: HashMap<u16, Request>,
    fids: HashMap<u32, String>,
    events: Vec<u8>,
    /// See the module doc.
    pub hold_after_submit: bool,
    /// See the module doc.
    pub hold_up_from_submit: bool,
    /// See the module doc.
    pub hold_down_from_ack_reply: bool,
    holding: bool,
    holding_up: bool,
    held_up: Vec<u8>,
    held_down: Vec<u8>,
    /// What each delayed server-to-client frame answers: request type and node.
    pub held_replies: Vec<(u8, String)>,
    pub seen: Vec<Seen>,
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

/// Splits one whole 9P frame off the front of `buffer`, if one is there.
fn frame(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    if buffer.len() < 7 {
        return None;
    }
    let size = u32_at(buffer, 0) as usize;
    assert!(size >= 7, "a 9P frame shorter than its header");
    (buffer.len() >= size).then(|| buffer.drain(..size).collect())
}

impl Tap {
    /// Listens at `path` for the client; connects to `upstream` (the
    /// transport's endpoint) once the client arrives.
    pub fn bind(path: &Path, upstream: &Path) -> Self {
        let listener = UnixListener::bind(path).unwrap();
        listener.set_nonblocking(true).unwrap();
        Self {
            listener,
            upstream: upstream.to_owned(),
            down: None,
            up: None,
            down_in: Vec::new(),
            up_in: Vec::new(),
            to_up: Vec::new(),
            to_down: Vec::new(),
            down_closed: false,
            up_closed: false,
            requests: HashMap::new(),
            fids: HashMap::new(),
            events: Vec::new(),
            hold_after_submit: false,
            hold_up_from_submit: false,
            hold_down_from_ack_reply: false,
            holding: false,
            holding_up: false,
            held_up: Vec::new(),
            held_down: Vec::new(),
            held_replies: Vec::new(),
            seen: Vec::new(),
        }
    }

    /// Moves whatever either side has written, without waiting.
    pub fn pump(&mut self) {
        if self.down.is_none() {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(true).unwrap();
                    let up = UnixStream::connect(&self.upstream).unwrap();
                    up.set_nonblocking(true).unwrap();
                    self.down = Some(stream);
                    self.up = Some(up);
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => return,
                Err(error) => panic!("tap accept: {error}"),
            }
        }
        for _ in 0..8 {
            let moved_down = self.read_down();
            while let Some(frame) = frame(&mut self.down_in) {
                self.client_frame(&frame);
                if self.holding_up {
                    self.held_up.extend_from_slice(&frame);
                } else {
                    self.to_up.extend_from_slice(&frame);
                }
            }
            let moved_up = self.read_up();
            while let Some(frame) = frame(&mut self.up_in) {
                let answers = self.server_frame(&frame);
                if self.holding {
                    self.held_down.extend_from_slice(&frame);
                    self.held_replies.push(answers);
                } else {
                    self.to_down.extend_from_slice(&frame);
                }
            }
            self.flush();
            if !moved_down && !moved_up {
                break;
            }
        }
        if self.up_closed && !self.down_closed && self.to_down.is_empty() {
            if let Some(down) = &self.down {
                let _ = down.shutdown(std::net::Shutdown::Both);
            }
            self.down_closed = true;
        }
        if self.down_closed && !self.up_closed {
            if let Some(up) = &self.up {
                let _ = up.shutdown(std::net::Shutdown::Both);
            }
            self.up_closed = true;
        }
    }

    /// Whether client-to-server bytes are being delayed.
    pub fn holding_up(&self) -> bool {
        self.holding_up
    }

    /// Forwards every delayed client-to-server byte, in order, and stops
    /// delaying.
    pub fn release_up(&mut self) {
        self.hold_up_from_submit = false;
        self.holding_up = false;
        self.to_up.append(&mut self.held_up);
        self.pump();
    }

    /// Delivers every delayed server-to-client byte, in order, and stops
    /// delaying.
    pub fn release_down(&mut self) {
        self.hold_down_from_ack_reply = false;
        self.holding = false;
        self.to_down.append(&mut self.held_down);
        self.held_replies.clear();
        self.pump();
    }

    pub fn walks(&self, node: &str) -> usize {
        self.seen
            .iter()
            .filter(|seen| matches!(seen, Seen::Walk(walked) if walked == node))
            .count()
    }

    pub fn open_replies(&self, node: &str) -> Vec<Result<(), u32>> {
        self.seen
            .iter()
            .filter_map(|seen| match seen {
                Seen::OpenReply {
                    node: opened,
                    reply,
                } if opened == node => Some(*reply),
                _ => None,
            })
            .collect()
    }

    pub fn submits(&self) -> Vec<(u16, Vec<u8>)> {
        self.seen
            .iter()
            .filter_map(|seen| match seen {
                Seen::Submit { tag, bytes } => Some((*tag, bytes.clone())),
                _ => None,
            })
            .collect()
    }

    pub fn submit_replies(&self) -> Vec<Result<u32, u32>> {
        self.seen
            .iter()
            .filter_map(|seen| match seen {
                Seen::SubmitReply { reply, .. } => Some(*reply),
                _ => None,
            })
            .collect()
    }

    pub fn staged(&self) -> Vec<Vec<u8>> {
        self.seen
            .iter()
            .filter_map(|seen| match seen {
                Seen::Staged(bytes) => Some(bytes.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn acks(&self) -> Vec<u64> {
        self.seen
            .iter()
            .filter_map(|seen| match seen {
                Seen::Ack(sequence) => Some(*sequence),
                _ => None,
            })
            .collect()
    }

    /// Every `Submitted` the journal returned for `submission_id`.
    pub fn submitted(&self, submission_id: u64) -> usize {
        self.seen
            .iter()
            .filter(|seen| {
                matches!(seen, Seen::Event { submitted: Some(value), .. }
                    if value.submission_id == submission_id)
            })
            .count()
    }

    fn read_side(stream: Option<&UnixStream>, into: &mut Vec<u8>, closed: &mut bool) -> bool {
        let Some(mut stream) = stream else {
            return false;
        };
        if *closed {
            return false;
        }
        let mut moved = false;
        let mut chunk = [0u8; 65536];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => {
                    *closed = true;
                    return moved;
                }
                Ok(count) => {
                    into.extend_from_slice(&chunk[..count]);
                    moved = true;
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) if error.kind() == ErrorKind::WouldBlock => return moved,
                Err(_) => {
                    *closed = true;
                    return moved;
                }
            }
        }
    }

    fn read_down(&mut self) -> bool {
        Self::read_side(self.down.as_ref(), &mut self.down_in, &mut self.down_closed)
    }

    fn read_up(&mut self) -> bool {
        Self::read_side(self.up.as_ref(), &mut self.up_in, &mut self.up_closed)
    }

    fn flush(&mut self) {
        Self::write_side(self.up.as_ref(), &mut self.to_up, self.up_closed);
        Self::write_side(self.down.as_ref(), &mut self.to_down, self.down_closed);
    }

    fn write_side(stream: Option<&UnixStream>, pending: &mut Vec<u8>, closed: bool) {
        let Some(mut stream) = stream else {
            return;
        };
        if closed {
            pending.clear();
            return;
        }
        while !pending.is_empty() {
            match stream.write(pending) {
                Ok(0) => return,
                Ok(count) => {
                    pending.drain(..count);
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => return,
            }
        }
    }

    /// Records one client-to-server request.
    fn client_frame(&mut self, frame: &[u8]) {
        let (kind, tag, body) = (frame[4], u16_at(frame, 5), &frame[7..]);
        let node = match kind {
            TATTACH => {
                self.fids.insert(u32_at(body, 0), String::new());
                String::new()
            }
            TWALK => {
                let (fid, newfid, names) = (u32_at(body, 0), u32_at(body, 4), u16_at(body, 8));
                let mut at = 10;
                let mut path = Vec::new();
                for _ in 0..names {
                    let length = usize::from(u16_at(body, at));
                    path.push(String::from_utf8_lossy(&body[at + 2..at + 2 + length]).into_owned());
                    at += 2 + length;
                }
                let node = if path.is_empty() {
                    self.fids.get(&fid).cloned().unwrap_or_default()
                } else {
                    path.join("/")
                };
                if !path.is_empty() {
                    self.seen.push(Seen::Walk(node.clone()));
                }
                self.fids.insert(newfid, node.clone());
                node
            }
            TLOPEN | TREAD | TWRITE => self.fids.get(&u32_at(body, 0)).cloned().unwrap_or_default(),
            _ => String::new(),
        };
        let offset = if matches!(kind, TREAD | TWRITE) {
            u64_at(body, 4)
        } else {
            0
        };
        if kind == TWRITE {
            let data = body[16..].to_vec();
            match node.as_str() {
                "transaction" => self.seen.push(Seen::Staged(data)),
                "submit" => {
                    self.seen.push(Seen::Submit { tag, bytes: data });
                    if self.hold_after_submit {
                        self.holding = true;
                    }
                    if self.hold_up_from_submit {
                        self.holding_up = true;
                    }
                }
                "ack" => {
                    let ack = decode_shell_file_ack(&data).expect("an ack");
                    self.seen.push(Seen::Ack(ack.sequence));
                }
                _ => {}
            }
        }
        self.requests.insert(tag, Request { kind, node, offset });
    }

    /// Records one server-to-client reply and returns what it answers,
    /// starting a delay first if it is the reply that begins one.
    fn server_frame(&mut self, frame: &[u8]) -> (u8, String) {
        let (kind, tag, body) = (frame[4], u16_at(frame, 5), &frame[7..]);
        let Some(request) = self.requests.remove(&tag) else {
            return (0, String::new());
        };
        let answers = (request.kind, request.node.clone());
        if self.hold_down_from_ack_reply && request.kind == TWRITE && request.node == "ack" {
            self.holding = true;
        }
        if request.kind == TLOPEN {
            let reply = match kind {
                RLOPEN => Ok(()),
                RLERROR => Err(u32_at(body, 0)),
                other => panic!("open answered with type {other}"),
            };
            self.seen.push(Seen::OpenReply {
                node: request.node.clone(),
                reply,
            });
        }
        if request.kind == TWRITE && request.node == "submit" {
            let reply = match kind {
                RWRITE => Ok(u32_at(body, 0)),
                RLERROR => Err(u32_at(body, 0)),
                other => panic!("submit answered with type {other}"),
            };
            self.seen.push(Seen::SubmitReply { tag, reply });
        }
        if request.kind != TREAD || kind != RREAD {
            return answers;
        }
        let data = &body[4..];
        match request.node.as_str() {
            "events" => {
                self.events.extend_from_slice(data);
                while self.events.len() >= 4
                    && self.events.len() >= u32_at(&self.events, 0) as usize
                {
                    let size = u32_at(&self.events, 0) as usize;
                    let record: Vec<u8> = self.events.drain(..size).collect();
                    let header = decode_shell_file_record(&record, ShellFileClass::Event)
                        .expect("a journal record")
                        .header;
                    self.seen.push(Seen::Event {
                        sequence: header.sequence,
                        kind: header.kind,
                        submitted: (header.kind == ShellFileKind::Submitted)
                            .then(|| decode_shell_file_submitted(&record).unwrap()),
                        published: (header.kind == ShellFileKind::ObjectPublished)
                            .then(|| decode_shell_file_object_published(&record).unwrap()),
                    });
                }
            }
            "catalog" | "indicators" | "outputs" | "limits" => self.seen.push(Seen::ObjectRead {
                node: request.node,
                offset: request.offset,
                returned: data.len(),
            }),
            _ => {}
        }
        answers
    }
}
