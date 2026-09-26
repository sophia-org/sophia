//! Shared test support: a static export and a client written from the
//! 9P2000.L specification (diod `protocol.md`), not from this crate's codec.
#![allow(dead_code)]

pub mod export;

use sophia_9p::{Connection, ConnectionId, Fatal, Limits};

#[allow(unused_imports)]
pub use export::{Name, Shared, StaticExport};

pub const NOFID: u32 = u32::MAX;
pub const NOTAG: u16 = u16::MAX;

/// Linux error numbers, as the specification's `Rlerror` carries them.
pub const EBADF: u32 = 9;
pub const EAGAIN: u32 = 11;
pub const EACCES: u32 = 13;
pub const ENOENT: u32 = 2;
pub const ENOTDIR: u32 = 20;
pub const EISDIR: u32 = 21;
pub const EINVAL: u32 = 22;
pub const EMFILE: u32 = 24;
pub const ENOSPC: u32 = 28;
pub const ENOSYS: u32 = 38;
pub const EPROTO: u32 = 71;
pub const EOPNOTSUPP: u32 = 95;
pub const ESTALE: u32 = 116;

/// A request body, field by field, little-endian.
#[derive(Default)]
pub struct Body(Vec<u8>);

impl Body {
    pub fn u8(mut self, value: u8) -> Self {
        self.0.push(value);
        self
    }
    pub fn u16(mut self, value: u16) -> Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }
    pub fn u32(mut self, value: u32) -> Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }
    pub fn u64(mut self, value: u64) -> Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }
    pub fn string(self, value: &[u8]) -> Self {
        let length = u16::try_from(value.len()).unwrap();
        self.u16(length).raw(value)
    }
    pub fn raw(mut self, value: &[u8]) -> Self {
        self.0.extend_from_slice(value);
        self
    }
}

/// size[4] type[1] tag[2] body.
pub fn frame(kind: u8, tag: u16, body: Body) -> Vec<u8> {
    let size = u32::try_from(7 + body.0.len()).unwrap();
    let mut bytes = size.to_le_bytes().to_vec();
    bytes.push(kind);
    bytes.extend_from_slice(&tag.to_le_bytes());
    bytes.extend_from_slice(&body.0);
    bytes
}

pub fn tversion(tag: u16, msize: u32, version: &[u8]) -> Vec<u8> {
    frame(100, tag, Body::default().u32(msize).string(version))
}
pub fn tattach(tag: u16, fid: u32, afid: u32, uname: &[u8], aname: &[u8]) -> Vec<u8> {
    let body = Body::default()
        .u32(fid)
        .u32(afid)
        .string(uname)
        .string(aname);
    frame(104, tag, body.u32(NOFID))
}
pub fn tflush(tag: u16, old: u16) -> Vec<u8> {
    frame(108, tag, Body::default().u16(old))
}
pub fn twalk(tag: u16, fid: u32, newfid: u32, names: &[&[u8]]) -> Vec<u8> {
    let count = u16::try_from(names.len()).unwrap();
    let mut body = Body::default().u32(fid).u32(newfid).u16(count);
    for name in names {
        body = body.string(name);
    }
    frame(110, tag, body)
}
pub fn tlopen(tag: u16, fid: u32, flags: u32) -> Vec<u8> {
    frame(12, tag, Body::default().u32(fid).u32(flags))
}
pub fn tread(tag: u16, fid: u32, offset: u64, count: u32) -> Vec<u8> {
    frame(116, tag, Body::default().u32(fid).u64(offset).u32(count))
}
pub fn twrite(tag: u16, fid: u32, offset: u64, data: &[u8]) -> Vec<u8> {
    let count = u32::try_from(data.len()).unwrap();
    frame(
        118,
        tag,
        Body::default().u32(fid).u64(offset).u32(count).raw(data),
    )
}
pub fn tclunk(tag: u16, fid: u32) -> Vec<u8> {
    frame(120, tag, Body::default().u32(fid))
}
pub fn tremove(tag: u16, fid: u32) -> Vec<u8> {
    frame(122, tag, Body::default().u32(fid))
}
pub fn tgetattr(tag: u16, fid: u32) -> Vec<u8> {
    frame(24, tag, Body::default().u32(fid).u64(0x3fff))
}

/// One reply frame, as the specification lays it out.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    pub kind: u8,
    pub tag: u16,
    pub body: Vec<u8>,
}

impl Frame {
    fn u16_at(&self, at: usize) -> u16 {
        u16::from_le_bytes(self.body[at..at + 2].try_into().unwrap())
    }
    fn u32_at(&self, at: usize) -> u32 {
        u32::from_le_bytes(self.body[at..at + 4].try_into().unwrap())
    }
    fn u64_at(&self, at: usize) -> u64 {
        u64::from_le_bytes(self.body[at..at + 8].try_into().unwrap())
    }

    /// `Rlerror ecode[4]`, or `None` for any other reply.
    pub fn errno(&self) -> Option<u32> {
        (self.kind == 7).then(|| {
            assert_eq!(self.body.len(), 4, "Rlerror body");
            self.u32_at(0)
        })
    }

    /// `Rversion msize[4] version[s]`.
    pub fn version(&self) -> (u32, Vec<u8>) {
        assert_eq!(self.kind, 101, "{self:?} is not Rversion");
        let length = usize::from(self.u16_at(4));
        assert_eq!(self.body.len(), 6 + length);
        (self.u32_at(0), self.body[6..].to_vec())
    }

    /// `qid[13]` at a body offset: type, version, path.
    pub fn qid_at(&self, at: usize) -> (u8, u32, u64) {
        (self.body[at], self.u32_at(at + 1), self.u64_at(at + 5))
    }

    /// `Rattach qid[13]`.
    pub fn attach(&self) -> (u8, u32, u64) {
        assert_eq!(self.kind, 105, "{self:?} is not Rattach");
        assert_eq!(self.body.len(), 13);
        self.qid_at(0)
    }

    /// `Rwalk nwqid[2] nwqid*(wqid[13])`.
    pub fn walk(&self) -> Vec<(u8, u32, u64)> {
        assert_eq!(self.kind, 111, "{self:?} is not Rwalk");
        let count = usize::from(self.u16_at(0));
        assert_eq!(self.body.len(), 2 + 13 * count);
        (0..count)
            .map(|index| self.qid_at(2 + 13 * index))
            .collect()
    }

    /// `Rlopen qid[13] iounit[4]`.
    pub fn lopen(&self) -> ((u8, u32, u64), u32) {
        assert_eq!(self.kind, 13, "{self:?} is not Rlopen");
        assert_eq!(self.body.len(), 17);
        (self.qid_at(0), self.u32_at(13))
    }

    /// `Rread count[4] data[count]`.
    pub fn data(&self) -> Vec<u8> {
        assert_eq!(self.kind, 117, "{self:?} is not Rread");
        let count = self.u32_at(0) as usize;
        assert_eq!(self.body.len(), 4 + count);
        self.body[4..].to_vec()
    }

    /// `Rwrite count[4]`.
    pub fn written(&self) -> u32 {
        assert_eq!(self.kind, 119, "{self:?} is not Rwrite");
        assert_eq!(self.body.len(), 4);
        self.u32_at(0)
    }

    /// `Rgetattr`: valid, qid, mode, and size.
    pub fn getattr(&self) -> (u64, (u8, u32, u64), u32, u64) {
        assert_eq!(self.kind, 25, "{self:?} is not Rgetattr");
        assert_eq!(self.body.len(), 153);
        // valid[8] qid[13] mode[4] uid[4] gid[4] nlink[8] rdev[8] size[8]
        (
            self.u64_at(0),
            self.qid_at(8),
            self.u32_at(21),
            self.u64_at(8 + 13 + 12 + 16),
        )
    }
}

/// Splits a byte stream into whole reply frames; a trailing partial frame is
/// a test failure.
pub fn frames(mut bytes: &[u8]) -> Vec<Frame> {
    let mut frames = Vec::new();
    while !bytes.is_empty() {
        let size = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        assert!(size >= 7 && size <= bytes.len(), "partial frame");
        frames.push(Frame {
            kind: bytes[4],
            tag: u16::from_le_bytes([bytes[5], bytes[6]]),
            body: bytes[7..size].to_vec(),
        });
        bytes = &bytes[size..];
    }
    frames
}

/// A connection driven directly, its output taken after every exchange.
pub struct Harness {
    pub export: StaticExport,
    pub connection: Connection<StaticExport>,
}

impl Harness {
    pub fn new() -> Self {
        Self::with(StaticExport::new(), ConnectionId(1), Limits::default())
    }

    pub fn with(export: StaticExport, id: ConnectionId, limits: Limits) -> Self {
        Self {
            export,
            connection: Connection::new(id, None, limits),
        }
    }

    /// Versioned at 8192 and attached as fid 0.
    pub fn attached() -> Self {
        let mut harness = Self::new();
        harness.versioned(8192);
        harness.attach(0);
        harness
    }

    pub fn versioned(&mut self, msize: u32) {
        let reply = self.one(&tversion(NOTAG, msize, b"9P2000.L"));
        assert_eq!(reply.version(), (msize, b"9P2000.L".to_vec()));
    }

    pub fn attach(&mut self, fid: u32) -> (u8, u32, u64) {
        self.one(&tattach(1, fid, NOFID, b"", b"")).attach()
    }

    /// Everything answered to these bytes.
    pub fn send(&mut self, bytes: &[u8]) -> Result<Vec<Frame>, Fatal> {
        self.connection.receive(&mut self.export, bytes)?;
        Ok(self.take())
    }

    /// The single reply to one request.
    pub fn one(&mut self, bytes: &[u8]) -> Frame {
        let mut replies = self.send(bytes).expect("no fatal error");
        assert_eq!(replies.len(), 1, "one reply expected: {replies:?}");
        replies.remove(0)
    }

    pub fn errno(&mut self, bytes: &[u8]) -> u32 {
        let reply = self.one(bytes);
        reply
            .errno()
            .unwrap_or_else(|| panic!("error expected: {reply:?}"))
    }

    pub fn take(&mut self) -> Vec<Frame> {
        let replies = frames(self.connection.output());
        let length = self.connection.output().len();
        self.connection.sent(length);
        replies
    }

    /// Walk from fid 0 to `path` as `newfid` and open it.
    pub fn open(&mut self, newfid: u32, path: &[&[u8]], flags: u32) -> Frame {
        let walked = self.one(&twalk(2, 0, newfid, path));
        assert_eq!(walked.walk().len(), path.len(), "{walked:?}");
        self.one(&tlopen(3, newfid, flags))
    }
}
