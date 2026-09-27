//! A bounded, read-only 9P2000.L client over a Unix stream socket.
//!
//! WHAT IT DOES. Version, attach, walk, open for reading, getattr, read,
//! readdir and clunk, one request at a time, each under one absolute deadline
//! that covers writing the request and assembling the reply. A read that
//! outlasts its deadline is flushed. It encodes requests and decodes replies
//! itself: it shares only the protocol's value records with the server core,
//! so the core never judges its own codec.
//!
//! WHAT IT DOES NOT. It cannot write, create, remove or change attributes:
//! no such operation exists here, and every open is read-only. Attach names
//! are sent as data; they grant nothing on either side. It never resynchronises
//! a stream it cannot trust: a reply with the wrong tag, type or shape, an I/O
//! error, or a timeout that a flush could not settle poisons the client, which
//! closes its socket and refuses every later call.

use std::collections::BTreeSet;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::client_codec::{
    self, Fields, HEADER, MAX_WALK, NOFID, NOTAG, RATTACH, RCLUNK, READ_OVERHEAD, RFLUSH, RLERROR,
    RLOPEN, RREAD, RVERSION, RWALK, TATTACH, TCLUNK, TFLUSH, TLOPEN, TREAD, TVERSION, TWALK,
};
use crate::records::{Attr, Errno, Qid, QidKind};

// Message types the server core answers but this client's shared codec does
// not: `Client` alone reads directory entries and attributes (numbering from
// diod protocol.md at de51d1ee1bd5, pinned in docs/sophia-9p-control-bus.md).
const TGETATTR: u8 = 24;
const RGETATTR: u8 = 25;
const TREADDIR: u8 = 40;
const RREADDIR: u8 = 41;

const NAME_MAX: usize = 255;
const O_RDONLY: u32 = 0;
const O_DIRECTORY: u32 = 0o200000;
/// Every basic `Tgetattr` field.
const GETATTR_ALL: u64 = 0x3fff;
/// valid[8] qid[13] mode[4] uid[4] gid[4] nlink[8] rdev[8] size[8]
/// blksize[8] blocks[8], four timestamps, gen[8] data_version[8].
const RGETATTR_BODY: usize = 153;
/// Linux `d_type` values for a directory and a regular file.
const DT_DIR: u8 = 4;
const DT_REG: u8 = 8;

/// Bounds a client holds itself to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientLimits {
    /// Offered at version; the server may answer smaller.
    pub msize: u32,
    /// From a request's first byte written to its reply's last byte read,
    /// and for connecting.
    pub request_deadline: Duration,
    /// For a flush to settle a read that outlasted its deadline.
    pub flush_deadline: Duration,
    /// Fids alive at once, counting any whose [`File`] was dropped unclunked.
    pub max_fids: u32,
}

impl ClientLimits {
    pub const MIN_MSIZE: u32 = 4096;
    pub const MAX_MSIZE: u32 = 16 << 20;

    fn validate(&self) -> Result<(), ClientError> {
        if !(Self::MIN_MSIZE..=Self::MAX_MSIZE).contains(&self.msize) {
            return Err(ClientError::Limit("msize outside 4096..=16 MiB"));
        }
        if self.request_deadline.is_zero() || self.flush_deadline.is_zero() {
            return Err(ClientError::Limit("zero deadline"));
        }
        if self.max_fids == 0 {
            return Err(ClientError::Limit("no fids"));
        }
        Ok(())
    }
}

impl Default for ClientLimits {
    fn default() -> Self {
        Self {
            msize: 65536,
            request_deadline: Duration::from_secs(5),
            flush_deadline: Duration::from_secs(1),
            max_fids: 16,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientError {
    /// The socket failed; the client is poisoned.
    Io(io::ErrorKind),
    /// The server answered `Rlerror`; the client is still usable.
    Remote(Errno),
    /// The server broke the protocol; the client is poisoned.
    Protocol(&'static str),
    /// A deadline passed without a settled answer; the client is poisoned.
    Timeout,
    /// An earlier failure poisoned the client.
    Poisoned,
    /// A local bound or rule refused the call before anything was sent.
    Limit(&'static str),
    /// The file belongs to another client.
    ForeignFile,
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(kind) => write!(formatter, "9P socket: {kind}"),
            Self::Remote(errno) => write!(formatter, "9P server error {}", errno.0),
            Self::Protocol(what) => write!(formatter, "9P protocol violation: {what}"),
            Self::Timeout => formatter.write_str("9P deadline passed"),
            Self::Poisoned => formatter.write_str("9P client poisoned by an earlier failure"),
            Self::Limit(what) => write!(formatter, "9P client limit: {what}"),
            Self::ForeignFile => formatter.write_str("9P file of another client"),
        }
    }
}

impl std::error::Error for ClientError {}

fn io_error(error: &io::Error) -> ClientError {
    match error.kind() {
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => ClientError::Timeout,
        kind => ClientError::Io(kind),
    }
}

/// A fid this client holds. It cannot be copied; [`Client::clunk`] consumes
/// it. One dropped without a clunk keeps its fid, and its place under
/// [`ClientLimits::max_fids`], for the life of the client.
#[derive(Debug)]
pub struct File {
    client: u64,
    fid: u32,
    qid: Qid,
    iounit: Option<u32>,
}

impl File {
    pub const fn qid(&self) -> Qid {
        self.qid
    }

    pub const fn is_open(&self) -> bool {
        self.iounit.is_some()
    }
}

/// One directory entry. `next` is the cookie that resumes after it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Listed {
    pub name: Vec<u8>,
    pub qid: Qid,
    pub next: u64,
    /// The Linux `d_type`, 4 for a directory or 8 for a regular file.
    pub dtype: u8,
}

static NEXT_CLIENT: AtomicU64 = AtomicU64::new(1);

pub struct Client {
    id: u64,
    stream: UnixStream,
    limits: ClientLimits,
    msize: u32,
    next_tag: u16,
    next_fid: u32,
    fids: BTreeSet<u32>,
    /// Bytes of a reply frame not yet complete. They are kept across a
    /// timed-out read so a flush can finish the same frame.
    partial: Vec<u8>,
    poisoned: bool,
}

impl Client {
    /// Connects, within the request deadline, and negotiates the version.
    pub fn connect(path: &Path, limits: ClientLimits) -> Result<Self, ClientError> {
        limits.validate()?;
        let deadline = deadline_after(limits.request_deadline)?;
        let stream = connect_by(path, deadline)?;
        Self::over(stream, limits)
    }

    /// Negotiates the version over an already connected stream.
    pub fn over(stream: UnixStream, limits: ClientLimits) -> Result<Self, ClientError> {
        limits.validate()?;
        stream.set_nonblocking(false).map_err(|e| io_error(&e))?;
        let mut client = Self {
            id: NEXT_CLIENT.fetch_add(1, Ordering::Relaxed),
            stream,
            limits,
            msize: limits.msize,
            next_tag: 0,
            next_fid: 0,
            fids: BTreeSet::new(),
            partial: Vec::new(),
            poisoned: false,
        };
        client.version()?;
        Ok(client)
    }

    pub const fn msize(&self) -> u32 {
        self.msize
    }

    pub const fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    fn version(&mut self) -> Result<(), ClientError> {
        let body = client_codec::version_body(self.limits.msize);
        let deadline = deadline_after(self.limits.request_deadline)?;
        let reply = self.exchange(TVERSION, NOTAG, &body, deadline)?;
        let body = self.expect(reply, NOTAG, RVERSION)?;
        let Some((msize, version)) = client_codec::decode_rversion(&body) else {
            return Err(self.poison(ClientError::Protocol("Rversion shape")));
        };
        let accepted = client_codec::accept_rversion(self.limits.msize, msize, version);
        match accepted {
            Ok(msize) => {
                self.msize = msize;
                Ok(())
            }
            Err(reason) => Err(self.poison(ClientError::Protocol(reason))),
        }
    }

    /// Attaches to the root the server gives this connection.
    pub fn attach(&mut self, uname: &[u8], aname: &[u8]) -> Result<File, ClientError> {
        let fid = self.allocate_fid()?;
        let body =
            client_codec::attach_body(fid, NOFID, uname, aname).map_err(ClientError::Limit)?;
        let result = self.call(TATTACH, &body, RATTACH).and_then(|reply| {
            client_codec::decode_rattach(&reply)
                .ok_or_else(|| self.poison(ClientError::Protocol("Rattach shape")))
        });
        self.settle_new_fid(fid, result)
    }

    /// Walks every name from `from` to a new fid. A walk that stops short
    /// creates no fid and is `ENOENT`.
    pub fn walk(&mut self, from: &File, names: &[&[u8]]) -> Result<File, ClientError> {
        self.owns(from)?;
        if names.len() > MAX_WALK {
            return Err(ClientError::Limit("more than sixteen names"));
        }
        if from.is_open() {
            return Err(ClientError::Limit("walk from an open file"));
        }
        let fid = self.allocate_fid()?;
        let body = client_codec::walk_body(from.fid, fid, names).map_err(ClientError::Limit)?;
        let result = self.call(TWALK, &body, RWALK).and_then(|reply| {
            let mut fields = Fields(&reply);
            let count = fields.u16().map(usize::from);
            let Some(count) = count.filter(|count| *count <= names.len()) else {
                return Err(self.poison(ClientError::Protocol("Rwalk count")));
            };
            let mut last = from.qid;
            for _ in 0..count {
                match fields.qid() {
                    Some(Ok(qid)) => last = qid,
                    _ => return Err(self.poison(ClientError::Protocol("Rwalk qid"))),
                }
            }
            if !fields.0.is_empty() {
                return Err(self.poison(ClientError::Protocol("Rwalk length")));
            }
            if count < names.len() {
                return Err(ClientError::Remote(Errno::ENOENT));
            }
            Ok(last)
        });
        self.settle_new_fid(fid, result)
    }

    /// Opens for reading, as a directory when asked.
    pub fn open(&mut self, file: &mut File, directory: bool) -> Result<(), ClientError> {
        self.owns(file)?;
        if file.is_open() {
            return Err(ClientError::Limit("already open"));
        }
        let flags = O_RDONLY | if directory { O_DIRECTORY } else { 0 };
        let body = client_codec::lopen_body(file.fid, flags);
        let reply = self.call(TLOPEN, &body, RLOPEN)?;
        match client_codec::decode_rlopen(&reply) {
            Some((qid, iounit)) => {
                file.qid = qid;
                file.iounit = Some(iounit);
                Ok(())
            }
            None => Err(self.poison(ClientError::Protocol("Rlopen shape"))),
        }
    }

    pub fn getattr(&mut self, file: &File) -> Result<Attr, ClientError> {
        self.owns(file)?;
        let mut body = file.fid.to_le_bytes().to_vec();
        body.extend_from_slice(&GETATTR_ALL.to_le_bytes());
        let reply = self.call(TGETATTR, &body, RGETATTR)?;
        if reply.len() != RGETATTR_BODY {
            return Err(self.poison(ClientError::Protocol("Rgetattr length")));
        }
        let mut fields = Fields(&reply);
        let valid = fields.u64();
        let qid = fields.qid();
        let mode = fields.u32();
        let _ids = fields.take(8);
        let nlink = fields.u64();
        let _rdev = fields.take(8);
        let size = fields.u64();
        match (valid, qid, mode, nlink, size) {
            (Some(valid), Some(Ok(qid)), Some(mode), Some(nlink), Some(size)) => Ok(Attr {
                valid,
                qid,
                mode,
                nlink,
                size,
            }),
            _ => Err(self.poison(ClientError::Protocol("Rgetattr qid"))),
        }
    }

    /// At most `count` bytes, and no more than one reply can carry.
    pub fn read(&mut self, file: &File, offset: u64, count: u32) -> Result<Vec<u8>, ClientError> {
        let deadline = deadline_after(self.limits.request_deadline)?;
        let tag = self.next_tag();
        let count = self.io_count(file, count)?;
        let body = client_codec::io_body(file.fid, offset, count);
        let reply = self.exchange(TREAD, tag, &body, deadline)?;
        let body = self.expect(reply, tag, RREAD)?;
        self.read_data(body, count)
    }

    /// A read that may wait, for a file whose reads block until data exists.
    /// At `deadline` the read is flushed: `None` means it was cancelled and
    /// nothing was read. A reply the server had already sent is discarded, so
    /// the caller reads again from the same offset.
    pub fn read_until(
        &mut self,
        file: &File,
        offset: u64,
        count: u32,
        deadline: Instant,
    ) -> Result<Option<Vec<u8>>, ClientError> {
        let count = self.io_count(file, count)?;
        if remaining(deadline).is_err() {
            // Nothing was sent, so nothing needs cancelling.
            return Ok(None);
        }
        let tag = self.next_tag();
        let body = client_codec::io_body(file.fid, offset, count);
        self.send(TREAD, tag, &body, deadline)?;
        match self.receive(deadline) {
            Ok(reply) => {
                let body = self.expect(reply, tag, RREAD)?;
                self.read_data(body, count).map(Some)
            }
            Err(ClientError::Timeout) => self.flush(tag).map(|()| None),
            Err(error) => Err(self.poison(error)),
        }
    }

    /// Directory entries from `cookie` on, within `count` bytes.
    pub fn readdir(
        &mut self,
        directory: &File,
        cookie: u64,
        count: u32,
    ) -> Result<Vec<Listed>, ClientError> {
        let deadline = deadline_after(self.limits.request_deadline)?;
        let tag = self.next_tag();
        let count = self.io_count(directory, count)?;
        let body = client_codec::io_body(directory.fid, cookie, count);
        let reply = self.exchange(TREADDIR, tag, &body, deadline)?;
        let body = self.expect(reply, tag, RREADDIR)?;
        let mut fields = Fields(&body);
        let length = fields.u32().map(|length| length as usize);
        if length != Some(fields.0.len()) || fields.0.len() > count as usize {
            return Err(self.poison(ClientError::Protocol("Rreaddir count")));
        }
        let mut entries = Vec::new();
        let mut previous = cookie;
        while !fields.0.is_empty() {
            let qid = fields.qid();
            let next = fields.u64();
            let dtype = fields.u8();
            let name = fields.string();
            let (Some(Ok(qid)), Some(next), Some(dtype), Some(name)) = (qid, next, dtype, name)
            else {
                return Err(self.poison(ClientError::Protocol("Rreaddir entry")));
            };
            let expected = match qid.kind {
                QidKind::Directory => DT_DIR,
                QidKind::File => DT_REG,
            };
            if dtype != expected || next <= previous || !listable(name) {
                return Err(self.poison(ClientError::Protocol("Rreaddir entry")));
            }
            previous = next;
            entries.push(Listed {
                name: name.to_vec(),
                qid,
                next,
                dtype,
            });
        }
        Ok(entries)
    }

    /// Reads from offset zero until an empty reply; more than `max_bytes` is
    /// a limit, not a truncation.
    pub fn read_to_end(&mut self, file: &File, max_bytes: usize) -> Result<Vec<u8>, ClientError> {
        let mut data = Vec::new();
        loop {
            let offset =
                u64::try_from(data.len()).map_err(|_| ClientError::Limit("offset overflow"))?;
            let chunk = self.read(file, offset, self.msize)?;
            if chunk.is_empty() {
                return Ok(data);
            }
            let total = data
                .len()
                .checked_add(chunk.len())
                .ok_or(ClientError::Limit("length overflow"))?;
            if total > max_bytes {
                return Err(ClientError::Limit("file larger than max_bytes"));
            }
            data.extend(chunk);
        }
    }

    /// Lists until an empty reply; more than `max_entries` is a limit.
    pub fn list_all(
        &mut self,
        directory: &File,
        max_entries: usize,
    ) -> Result<Vec<Listed>, ClientError> {
        let mut entries: Vec<Listed> = Vec::new();
        loop {
            let cookie = entries.last().map_or(0, |entry| entry.next);
            let page = self.readdir(directory, cookie, self.msize)?;
            if page.is_empty() {
                return Ok(entries);
            }
            let total = entries
                .len()
                .checked_add(page.len())
                .ok_or(ClientError::Limit("count overflow"))?;
            if total > max_entries {
                return Err(ClientError::Limit("directory larger than max_entries"));
            }
            entries.extend(page);
        }
    }

    /// Ends the fid. It is gone locally whatever the server answers, as a
    /// clunk always frees the fid on the server too.
    pub fn clunk(&mut self, file: File) -> Result<(), ClientError> {
        self.owns(&file)?;
        let result = self.call(TCLUNK, &client_codec::clunk_body(file.fid), RCLUNK);
        self.fids.remove(&file.fid);
        match result {
            Ok(body) => match client_codec::decode_rclunk(&body) {
                Some(()) => Ok(()),
                None => Err(self.poison(ClientError::Protocol("Rclunk length"))),
            },
            Err(error) => Err(error),
        }
    }

    fn owns(&self, file: &File) -> Result<(), ClientError> {
        if file.client != self.id || !self.fids.contains(&file.fid) {
            return Err(ClientError::ForeignFile);
        }
        Ok(())
    }

    fn allocate_fid(&mut self) -> Result<u32, ClientError> {
        if self.poisoned {
            return Err(ClientError::Poisoned);
        }
        if self.fids.len() >= self.limits.max_fids as usize {
            return Err(ClientError::Limit("max_fids reached"));
        }
        loop {
            let fid = self.next_fid;
            self.next_fid = self.next_fid.wrapping_add(1);
            if fid != NOFID && self.fids.insert(fid) {
                return Ok(fid);
            }
        }
    }

    /// A new fid exists only if its request succeeded.
    fn settle_new_fid(
        &mut self,
        fid: u32,
        qid: Result<Qid, ClientError>,
    ) -> Result<File, ClientError> {
        match qid {
            Ok(qid) => Ok(File {
                client: self.id,
                fid,
                qid,
                iounit: None,
            }),
            Err(error) => {
                self.fids.remove(&fid);
                Err(error)
            }
        }
    }

    /// The count a read or listing may ask for: open for reading, and within
    /// what one reply can carry.
    fn io_count(&self, file: &File, count: u32) -> Result<u32, ClientError> {
        self.owns(file)?;
        let Some(iounit) = file.iounit else {
            return Err(ClientError::Limit("not open"));
        };
        let mut limit = self.msize - READ_OVERHEAD;
        if iounit != 0 {
            limit = limit.min(iounit);
        }
        Ok(count.min(limit))
    }

    fn read_data(&mut self, body: Vec<u8>, count: u32) -> Result<Vec<u8>, ClientError> {
        match client_codec::decode_rread(&body) {
            Some(data) if data.len() <= count as usize => Ok(data),
            _ => Err(self.poison(ClientError::Protocol("Rread count"))),
        }
    }

    fn next_tag(&mut self) -> u16 {
        let tag = self.next_tag;
        self.next_tag = if self.next_tag >= NOTAG - 1 {
            0
        } else {
            self.next_tag + 1
        };
        tag
    }

    /// One request and its reply's body, under the request deadline.
    fn call(&mut self, kind: u8, body: &[u8], reply_kind: u8) -> Result<Vec<u8>, ClientError> {
        let deadline = deadline_after(self.limits.request_deadline)?;
        let tag = self.next_tag();
        let reply = self.exchange(kind, tag, body, deadline)?;
        self.expect(reply, tag, reply_kind)
    }

    fn exchange(
        &mut self,
        kind: u8,
        tag: u16,
        body: &[u8],
        deadline: Instant,
    ) -> Result<Frame, ClientError> {
        self.send(kind, tag, body, deadline)?;
        self.receive(deadline).map_err(|error| self.poison(error))
    }

    /// The reply body if it is the answer to `tag` of the expected type;
    /// `Rlerror` is the server's refusal and leaves the client usable.
    fn expect(&mut self, reply: Frame, tag: u16, kind: u8) -> Result<Vec<u8>, ClientError> {
        if reply.tag != tag {
            return Err(self.poison(ClientError::Protocol("reply to another tag")));
        }
        if reply.kind == RLERROR {
            return match client_codec::decode_rlerror(&reply.body) {
                Some(errno) => Err(ClientError::Remote(errno)),
                None => Err(self.poison(ClientError::Protocol("Rlerror length"))),
            };
        }
        if reply.kind != kind {
            return Err(self.poison(ClientError::Protocol("unexpected reply type")));
        }
        Ok(reply.body)
    }

    /// Settles a read that outlasted its deadline. The flush and the frames
    /// still owed get the flush deadline; the one reply the server may have
    /// sent to the read before the flush is discarded.
    fn flush(&mut self, old: u16) -> Result<(), ClientError> {
        let deadline = deadline_after(self.limits.flush_deadline)?;
        let tag = self.next_tag();
        self.send(TFLUSH, tag, &client_codec::flush_body(old), deadline)?;
        let mut late = false;
        loop {
            let reply = self.receive(deadline).map_err(|error| self.poison(error))?;
            match (reply.tag, reply.kind) {
                (t, RFLUSH) if t == tag && reply.body.is_empty() => return Ok(()),
                (t, RREAD | RLERROR) if t == old && !late => late = true,
                _ => return Err(self.poison(ClientError::Protocol("reply during flush"))),
            }
        }
    }

    fn send(
        &mut self,
        kind: u8,
        tag: u16,
        body: &[u8],
        deadline: Instant,
    ) -> Result<(), ClientError> {
        if self.poisoned {
            return Err(ClientError::Poisoned);
        }
        let size = HEADER
            .checked_add(body.len())
            .filter(|size| *size <= self.msize as usize)
            .ok_or(ClientError::Limit("request larger than msize"))?;
        let mut frame = Vec::with_capacity(size);
        frame.extend_from_slice(&(size as u32).to_le_bytes());
        frame.push(kind);
        frame.extend_from_slice(&tag.to_le_bytes());
        frame.extend_from_slice(body);
        let mut written = 0;
        while written < frame.len() {
            let result = remaining(deadline).and_then(|left| {
                self.stream
                    .set_write_timeout(Some(left))
                    .map_err(|e| io_error(&e))?;
                match self.stream.write(&frame[written..]) {
                    Ok(0) => Err(ClientError::Io(io::ErrorKind::WriteZero)),
                    Ok(count) => Ok(count),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => Ok(0),
                    Err(e) => Err(io_error(&e)),
                }
            });
            match result {
                Ok(count) => written += count,
                // A request cut short leaves the stream unusable.
                Err(error) => return Err(self.poison(error)),
            }
        }
        Ok(())
    }

    /// The next whole frame. Bytes are read only up to the end of the frame
    /// being assembled, and kept if the deadline passes first, so a flush can
    /// finish that same frame; they are never reread as a new header.
    fn receive(&mut self, deadline: Instant) -> Result<Frame, ClientError> {
        if self.poisoned {
            return Err(ClientError::Poisoned);
        }
        loop {
            let wanted = if self.partial.len() < 4 {
                4
            } else {
                let size = u32::from_le_bytes(self.partial[..4].try_into().unwrap()) as usize;
                if size < HEADER || size > self.msize as usize {
                    return Err(ClientError::Protocol("reply frame size"));
                }
                size
            };
            if self.partial.len() == wanted && wanted >= HEADER {
                let frame = std::mem::take(&mut self.partial);
                return Ok(Frame {
                    kind: frame[4],
                    tag: u16::from_le_bytes([frame[5], frame[6]]),
                    body: frame[HEADER..].to_vec(),
                });
            }
            let left = remaining(deadline)?;
            self.stream
                .set_read_timeout(Some(left))
                .map_err(|e| io_error(&e))?;
            let mut buffer = vec![0; wanted - self.partial.len()];
            match self.stream.read(&mut buffer) {
                Ok(0) => return Err(ClientError::Io(io::ErrorKind::UnexpectedEof)),
                Ok(count) => self.partial.extend_from_slice(&buffer[..count]),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(io_error(&e)),
            }
        }
    }

    /// Marks the client unusable and closes its socket.
    fn poison(&mut self, error: ClientError) -> ClientError {
        if !self.poisoned {
            self.poisoned = true;
            let _ = self.stream.shutdown(std::net::Shutdown::Both);
        }
        error
    }
}

struct Frame {
    kind: u8,
    tag: u16,
    body: Vec<u8>,
}

fn deadline_after(duration: Duration) -> Result<Instant, ClientError> {
    Instant::now()
        .checked_add(duration)
        .ok_or(ClientError::Limit("deadline overflow"))
}

/// The time left before `deadline`, or `Timeout` when none is.
fn remaining(deadline: Instant) -> Result<Duration, ClientError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
        .ok_or(ClientError::Timeout)
}

/// Connects without blocking past `deadline`, sharing the dance with
/// [`crate::pipeline::Pipeline`].
fn connect_by(path: &Path, deadline: Instant) -> Result<UnixStream, ClientError> {
    client_codec::connect_by(path, deadline).map_err(|error| match error {
        client_codec::ConnectError::Io(kind) => ClientError::Io(kind),
        client_codec::ConnectError::Timeout => ClientError::Timeout,
        client_codec::ConnectError::Limit(what) => ClientError::Limit(what),
    })
}

fn listable(name: &[u8]) -> bool {
    !name.is_empty()
        && name.len() <= NAME_MAX
        && name != b"."
        && name != b".."
        && !name.iter().any(|byte| matches!(byte, b'/' | 0))
}
