//! The client grows with the groups, so parts of it are unused until the
//! group that needs them lands.
#![allow(dead_code)]

//! An X client for the acceptance groups: enough wire to ask, and enough to
//! tell a reply from an error from an event.
//!
//! Written here rather than borrowed from the private-input support beside
//! it, which reads fixed thirty-two byte replies and has no expression for an
//! error's fields. A group that cannot read the error a request answers with
//! cannot prove a refusal, and most of what XTEST owes is refusals.

use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

pub const WAIT: Duration = Duration::from_secs(8);
/// The name a private instance authenticates a setup against.
pub const COOKIE_NAME: &[u8] = b"SOPHIA-PRIVATE-INPUT-1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Order {
    Little,
    Big,
}

impl Order {
    pub fn u16(self, value: u16) -> [u8; 2] {
        match self {
            Self::Little => value.to_le_bytes(),
            Self::Big => value.to_be_bytes(),
        }
    }

    pub fn u32(self, value: u32) -> [u8; 4] {
        match self {
            Self::Little => value.to_le_bytes(),
            Self::Big => value.to_be_bytes(),
        }
    }

    pub fn read16(self, bytes: &[u8]) -> u16 {
        let bytes = [bytes[0], bytes[1]];
        match self {
            Self::Little => u16::from_le_bytes(bytes),
            Self::Big => u16::from_be_bytes(bytes),
        }
    }

    pub fn read32(self, bytes: &[u8]) -> u32 {
        let bytes = [bytes[0], bytes[1], bytes[2], bytes[3]];
        match self {
            Self::Little => u32::from_le_bytes(bytes),
            Self::Big => u32::from_be_bytes(bytes),
        }
    }
}

/// One completion: what the server sent back for a request, or an event that
/// arrived before it.
#[derive(Clone, Debug)]
pub enum Answer {
    Reply(Vec<u8>),
    Error(XError),
    Event(Vec<u8>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XError {
    pub code: u8,
    pub sequence: u16,
    pub value: u32,
    pub minor: u16,
    pub major: u8,
}

pub struct Client {
    stream: UnixStream,
    order: Order,
    root: u32,
    base: u32,
    sequence: u16,
}

impl Client {
    /// Connect and authenticate. A cookie presents the private credential;
    /// none presents an empty name and data, which is an ordinary ungranted
    /// client rather than a refused one.
    pub fn connect(path: &Path, order: Order, cookie: Option<[u8; 32]>) -> Result<Self, String> {
        let mut stream = UnixStream::connect(path).map_err(|error| error.to_string())?;
        let deadline = Instant::now() + WAIT;
        stream
            .set_write_timeout(Some(WAIT))
            .map_err(|e| e.to_string())?;
        let name = if cookie.is_some() { COOKIE_NAME } else { b"" };
        let mut hello = vec![
            match order {
                Order::Little => b'l',
                Order::Big => b'B',
            },
            0,
        ];
        hello.extend(order.u16(11));
        hello.extend(order.u16(0));
        hello.extend(order.u16(name.len() as u16));
        hello.extend(order.u16(if cookie.is_some() { 32 } else { 0 }));
        hello.extend([0, 0]);
        hello.extend(name);
        while hello.len() % 4 != 0 {
            hello.push(0);
        }
        if let Some(cookie) = cookie {
            hello.extend(cookie);
        }
        stream.write_all(&hello).map_err(|e| e.to_string())?;
        let mut prefix = [0; 8];
        read_until(&mut stream, &mut prefix, deadline)?;
        let mut body = vec![0; usize::from(order.read16(&prefix[6..])) * 4];
        read_until(&mut stream, &mut body, deadline)?;
        if prefix[0] != 1 {
            return Err("setup refused".into());
        }
        let vendor = usize::from(order.read16(&body[16..]));
        let formats = usize::from(body[21]);
        let screen = 32 + vendor.next_multiple_of(4) + formats * 8;
        if body.len() < screen + 40 {
            return Err("short setup screen".into());
        }
        Ok(Self {
            stream,
            order,
            root: order.read32(&body[screen..]),
            base: order.read32(&body[4..]),
            sequence: 0,
        })
    }

    pub fn order(&self) -> Order {
        self.order
    }

    pub fn root(&self) -> u32 {
        self.root
    }

    /// A resource identifier this client may create, from its own range.
    pub fn resource(&self, offset: u32) -> u32 {
        self.base + offset
    }

    /// Send one request, and answer with its sequence number.
    ///
    /// The body excludes the four-byte header this writes, and is padded
    /// here: a request whose length field disagreed with its bytes would be
    /// testing the harness rather than the server.
    pub fn send(&mut self, opcode: u8, detail: u8, body: &[u8]) -> u16 {
        let mut request = vec![opcode, detail];
        let padded = body.len().next_multiple_of(4);
        request.extend(self.order.u16(((4 + padded) / 4) as u16));
        request.extend(body);
        request.resize(4 + padded, 0);
        self.stream.write_all(&request).expect("request written");
        self.sequence = self.sequence.wrapping_add(1);
        self.sequence
    }

    /// Read the next thing the server sends, whatever kind it is.
    pub fn answer(&mut self) -> Answer {
        self.read_answer(Instant::now() + WAIT).expect("an answer")
    }

    /// The next answer if one arrives within `within`, or `None`.
    ///
    /// For a request that is expected to hold the connection: proving that
    /// nothing came is a different claim from proving what came, and a wait
    /// that panics on silence cannot make it. A connection this returned
    /// `None` for is not read again; bytes that arrived late would be read
    /// against the wrong request.
    pub fn try_answer(&mut self, within: Duration) -> Option<Answer> {
        self.read_answer(Instant::now() + within).ok()
    }

    fn read_answer(&mut self, deadline: Instant) -> Result<Answer, String> {
        let mut head = [0; 32];
        read_until(&mut self.stream, &mut head, deadline)?;
        Ok(match head[0] {
            0 => Answer::Error(XError {
                code: head[1],
                sequence: self.order.read16(&head[2..]),
                value: self.order.read32(&head[4..]),
                minor: self.order.read16(&head[8..]),
                major: head[10],
            }),
            1 => {
                let extra =
                    usize::try_from(self.order.read32(&head[4..])).expect("reply length") * 4;
                let mut reply = head.to_vec();
                if extra > 0 {
                    let mut tail = vec![0; extra];
                    read_until(&mut self.stream, &mut tail, deadline)?;
                    reply.extend(tail);
                }
                Answer::Reply(reply)
            }
            _ => Answer::Event(head.to_vec()),
        })
    }

    /// The reply to a request, with events before it set aside.
    pub fn reply(&mut self, opcode: u8, detail: u8, body: &[u8]) -> Vec<u8> {
        let sequence = self.send(opcode, detail, body);
        loop {
            match self.answer() {
                Answer::Reply(reply) => {
                    assert_eq!(
                        self.order.read16(&reply[2..]),
                        sequence,
                        "reply belongs to another request"
                    );
                    return reply;
                }
                Answer::Event(_) => {}
                Answer::Error(error) => panic!("expected a reply, got {error:?}"),
            }
        }
    }

    /// The error a request must answer with, with events before it set aside.
    pub fn error(&mut self, opcode: u8, detail: u8, body: &[u8]) -> XError {
        let sequence = self.send(opcode, detail, body);
        loop {
            match self.answer() {
                Answer::Error(error) => {
                    assert_eq!(error.sequence, sequence, "error belongs to another request");
                    return error;
                }
                Answer::Event(_) => {}
                Answer::Reply(reply) => panic!("expected an error, got a reply {reply:?}"),
            }
        }
    }

    /// Whether the server advertises an extension, and at which opcode.
    ///
    /// Returns the whole reply rather than the opcode alone, because the
    /// first_event and first_error an extension declares are part of what
    /// discovery owes and are read from the same answer.
    pub fn query_extension(&mut self, name: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend(self.order.u16(name.len() as u16));
        body.extend([0, 0]);
        body.extend(name);
        self.reply(98, 0, &body)
    }

    /// Every extension name the server lists.
    pub fn extension_names(&mut self) -> Vec<String> {
        let reply = self.reply(99, 0, &[]);
        let mut names = Vec::new();
        let mut offset = 32;
        for _ in 0..reply[1] {
            let length = usize::from(reply[offset]);
            offset += 1;
            names.push(String::from_utf8_lossy(&reply[offset..offset + length]).into_owned());
            offset += length;
        }
        names
    }

    /// Round-trip one request so everything before it has been answered.
    pub fn sync(&mut self) {
        let reply = self.reply(43, 0, &[]);
        assert_eq!(reply[0], 1, "GetInputFocus answers a reply");
    }
}

fn read_until(
    stream: &mut UnixStream,
    mut bytes: &mut [u8],
    deadline: Instant,
) -> Result<(), String> {
    while !bytes.is_empty() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or("wire read deadline expired")?;
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|error| error.to_string())?;
        match stream.read(bytes) {
            Ok(0) => return Err("peer closed before the expected bytes".into()),
            Ok(read) => bytes = &mut bytes[read..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}
