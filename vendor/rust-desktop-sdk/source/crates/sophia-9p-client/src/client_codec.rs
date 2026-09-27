//! Byte-level 9P2000.L codec shared by [`crate::client`]'s blocking,
//! read-only client and [`crate::pipeline`]'s nonblocking, pipelined client.
//!
//! WHAT IT DOES. The pure, mechanical half of each client's codec: message
//! kinds, frame limits, the field cursor, request body encoders and reply
//! body decoders whose shape does not depend on which client is asking (a
//! decoder here reports only whether a reply parses to its exact shape).
//! Independent of the server core's own wire module, by the same
//! reasoning [`crate::client`]'s doc comment already gives: neither client
//! judges its own codec against the core it talks to.
//!
//! WHAT IT DOES NOT. Nothing here decides what a shape violation means to its
//! caller. [`crate::client::Client`] tells apart, say, a truncated `Rwalk`
//! count from a walk that answered more qids than it asked for, with a
//! distinct poison message for each; that granularity is specific to one
//! call site and stays there. This module hands back `Option`/`Result`
//! values a caller narrates in its own words.

use std::io::{self, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::net::{AddressFamily, SocketAddrUnix, SocketFlags, SocketType};
use std::os::unix::net::UnixStream;

use crate::records::{Errno, Qid, QidKind};

// Message types, from the 9P2000.L specification (diod protocol.md at
// de51d1ee1bd5, pinned in docs/sophia-9p-control-bus.md), for the
// verbs both clients send or receive. `Tgetattr`/`Treaddir` and their replies
// are `Client`-only and stay defined there.
pub(crate) const RLERROR: u8 = 7;
pub(crate) const TLOPEN: u8 = 12;
pub(crate) const RLOPEN: u8 = 13;
pub(crate) const TVERSION: u8 = 100;
pub(crate) const RVERSION: u8 = 101;
pub(crate) const TATTACH: u8 = 104;
pub(crate) const RATTACH: u8 = 105;
pub(crate) const TFLUSH: u8 = 108;
pub(crate) const RFLUSH: u8 = 109;
pub(crate) const TWALK: u8 = 110;
pub(crate) const RWALK: u8 = 111;
pub(crate) const TREAD: u8 = 116;
pub(crate) const RREAD: u8 = 117;
pub(crate) const TWRITE: u8 = 118;
pub(crate) const RWRITE: u8 = 119;
pub(crate) const TCLUNK: u8 = 120;
pub(crate) const RCLUNK: u8 = 121;

pub(crate) const NOTAG: u16 = u16::MAX;
pub(crate) const NOFID: u32 = u32::MAX;
pub(crate) const DIALECT: &[u8] = b"9P2000.L";
/// size[4] type[1] tag[2].
pub(crate) const HEADER: usize = 7;
/// What `Rread` adds to its data: count[4].
pub(crate) const READ_OVERHEAD: u32 = 11;
pub(crate) const MAX_WALK: usize = 16;
/// The smallest and largest `msize` either client will offer or accept.
pub(crate) const MIN_MSIZE: u32 = 4096;
pub(crate) const MAX_MSIZE: u32 = 16 << 20;
/// How long a connect waits before retrying a listener whose queue is full.
const CONNECT_RETRY: Duration = Duration::from_millis(5);

/// A cursor over a reply or request body. Each read either consumes exactly
/// its field or leaves the cursor unchanged and reports `None`.
pub(crate) struct Fields<'body>(pub(crate) &'body [u8]);

impl<'body> Fields<'body> {
    pub(crate) fn take(&mut self, count: usize) -> Option<&'body [u8]> {
        if self.0.len() < count {
            self.0 = &[];
            return None;
        }
        let (head, rest) = self.0.split_at(count);
        self.0 = rest;
        Some(head)
    }
    pub(crate) fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|bytes| bytes[0])
    }
    pub(crate) fn u16(&mut self) -> Option<u16> {
        self.take(2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
    }
    pub(crate) fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
    }
    pub(crate) fn u64(&mut self) -> Option<u64> {
        self.take(8)
            .map(|bytes| u64::from_le_bytes(bytes.try_into().unwrap()))
    }
    pub(crate) fn string(&mut self) -> Option<&'body [u8]> {
        let length = usize::from(self.u16()?);
        self.take(length)
    }
    /// `None` when too short; `Some(Err)` for a qid type this codec does not
    /// know.
    pub(crate) fn qid(&mut self) -> Option<Result<Qid, ()>> {
        let kind = match self.u8()? {
            0x80 => Ok(QidKind::Directory),
            0x00 => Ok(QidKind::File),
            _ => Err(()),
        };
        let version = self.u32()?;
        let path = self.u64()?;
        Some(kind.map(|kind| Qid {
            kind,
            version,
            path,
        }))
    }
}

/// A string longer than a `u16` can hold.
pub(crate) fn put_string(out: &mut Vec<u8>, value: &[u8]) -> Result<(), &'static str> {
    let length = u16::try_from(value.len()).map_err(|_| "string too long")?;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(value);
    Ok(())
}

/// `Tversion msize[4] version[s]`, always offering [`DIALECT`].
pub(crate) fn version_body(msize: u32) -> Vec<u8> {
    let mut body = msize.to_le_bytes().to_vec();
    // Infallible: DIALECT's length fits a u16 many times over.
    body.extend_from_slice(&(DIALECT.len() as u16).to_le_bytes());
    body.extend_from_slice(DIALECT);
    body
}

/// `Tattach fid[4] afid[4] uname[s] aname[s] n_uname[4]`. `n_uname` is always
/// sent as [`NOFID`]: neither client authenticates by numeric uid.
pub(crate) fn attach_body(
    fid: u32,
    afid: u32,
    uname: &[u8],
    aname: &[u8],
) -> Result<Vec<u8>, &'static str> {
    let mut body = fid.to_le_bytes().to_vec();
    body.extend_from_slice(&afid.to_le_bytes());
    put_string(&mut body, uname)?;
    put_string(&mut body, aname)?;
    body.extend_from_slice(&NOFID.to_le_bytes());
    Ok(body)
}

/// `Twalk fid[4] newfid[4] nwname[2] nwname*(wname[s])`. Callers bound
/// `names.len()` against [`MAX_WALK`] themselves, before this is called, so a
/// request over the limit never reaches the wire.
pub(crate) fn walk_body(fid: u32, newfid: u32, names: &[&[u8]]) -> Result<Vec<u8>, &'static str> {
    let mut body = fid.to_le_bytes().to_vec();
    body.extend_from_slice(&newfid.to_le_bytes());
    body.extend_from_slice(&(names.len() as u16).to_le_bytes());
    for name in names {
        put_string(&mut body, name)?;
    }
    Ok(body)
}

/// `Tlopen fid[4] flags[4]`.
pub(crate) fn lopen_body(fid: u32, flags: u32) -> Vec<u8> {
    let mut body = fid.to_le_bytes().to_vec();
    body.extend_from_slice(&flags.to_le_bytes());
    body
}

/// `Tread`/`Treaddir fid[4] offset[8] count[4]`: the same shape either way.
pub(crate) fn io_body(fid: u32, offset: u64, count: u32) -> Vec<u8> {
    let mut body = fid.to_le_bytes().to_vec();
    body.extend_from_slice(&offset.to_le_bytes());
    body.extend_from_slice(&count.to_le_bytes());
    body
}

/// `Twrite fid[4] offset[8] count[4] data[count]`. `count` is taken from the
/// caller, already checked against `data.len()`, so a `data` too long for a
/// `u32` cannot silently produce a short count.
pub(crate) fn write_body(fid: u32, offset: u64, count: u32, data: &[u8]) -> Vec<u8> {
    let mut body = fid.to_le_bytes().to_vec();
    body.extend_from_slice(&offset.to_le_bytes());
    body.extend_from_slice(&count.to_le_bytes());
    body.extend_from_slice(data);
    body
}

/// `Tclunk fid[4]`.
pub(crate) fn clunk_body(fid: u32) -> Vec<u8> {
    fid.to_le_bytes().to_vec()
}

/// `Tflush oldtag[2]`.
pub(crate) fn flush_body(old: u16) -> Vec<u8> {
    old.to_le_bytes().to_vec()
}

/// `Rversion msize[4] version[s]`, parsed but not judged: see
/// [`accept_rversion`].
pub(crate) fn decode_rversion(body: &[u8]) -> Option<(u32, &[u8])> {
    let mut fields = Fields(body);
    let msize = fields.u32()?;
    let version = fields.string()?;
    fields.0.is_empty().then_some((msize, version))
}

/// The acceptance rule both clients apply to a decoded `Rversion`: the
/// dialect must be exactly [`DIALECT`] (no suffix, since neither client
/// offers one), and the negotiated `msize` must be no larger than offered and
/// no smaller than [`MIN_MSIZE`].
pub(crate) fn accept_rversion(
    offered: u32,
    msize: u32,
    version: &[u8],
) -> Result<u32, &'static str> {
    if version != DIALECT {
        return Err("dialect other than 9P2000.L");
    }
    if msize > offered || msize < MIN_MSIZE {
        return Err("negotiated msize out of range");
    }
    Ok(msize)
}

/// `Rattach qid[13]`.
pub(crate) fn decode_rattach(body: &[u8]) -> Option<Qid> {
    let mut fields = Fields(body);
    let qid = fields.qid()?.ok()?;
    fields.0.is_empty().then_some(qid)
}

/// `Rwalk nwqid[2] nwqid*(wqid[13])`, every qid decoded or none at all: a
/// short count, a count over [`MAX_WALK`], a body whose length is not
/// exactly `2 + 13*nwqid`, an unknown qid type, or trailing bytes are alike
/// `None` -- checked, and nothing allocated, before any qid is read, so a
/// bogus declared count cannot drive an allocation sized from it. A caller
/// that must tell these apart (their exact count against what it asked for,
/// in particular) does not use this and decodes the shape itself.
pub(crate) fn decode_rwalk(body: &[u8]) -> Option<Vec<Qid>> {
    let mut fields = Fields(body);
    let count = usize::from(fields.u16()?);
    if count > MAX_WALK || fields.0.len() != count * 13 {
        return None;
    }
    let mut qids = Vec::with_capacity(count);
    for _ in 0..count {
        qids.push(fields.qid()?.ok()?);
    }
    fields.0.is_empty().then_some(qids)
}

/// `Rlopen qid[13] iounit[4]`.
pub(crate) fn decode_rlopen(body: &[u8]) -> Option<(Qid, u32)> {
    let mut fields = Fields(body);
    let qid = fields.qid()?.ok()?;
    let iounit = fields.u32()?;
    fields.0.is_empty().then_some((qid, iounit))
}

/// `Rread count[4] data[count]`. The declared count must equal the bytes
/// that follow it exactly; whether that many bytes is itself acceptable
/// (within what the caller asked for) is for the caller to decide.
pub(crate) fn decode_rread(body: &[u8]) -> Option<Vec<u8>> {
    let mut fields = Fields(body);
    let length = usize::try_from(fields.u32()?).ok()?;
    let data = fields.take(length)?;
    fields.0.is_empty().then(|| data.to_vec())
}

/// `Rwrite count[4]`.
pub(crate) fn decode_rwrite(body: &[u8]) -> Option<u32> {
    let mut fields = Fields(body);
    let count = fields.u32()?;
    fields.0.is_empty().then_some(count)
}

/// `Rclunk`: no body.
pub(crate) fn decode_rclunk(body: &[u8]) -> Option<()> {
    body.is_empty().then_some(())
}

/// `Rflush`: no body.
pub(crate) fn decode_rflush(body: &[u8]) -> Option<()> {
    body.is_empty().then_some(())
}

/// `Rlerror ecode[4]`.
pub(crate) fn decode_rlerror(body: &[u8]) -> Option<Errno> {
    let mut fields = Fields(body);
    let errno = fields.u32()?;
    fields.0.is_empty().then_some(Errno(errno))
}

/// Why [`connect_by`], or the blocking handshake over its stream, failed.
pub(crate) enum ConnectError {
    Io(io::ErrorKind),
    Timeout,
    /// A local bound refused the call.
    Limit(&'static str),
    /// The peer broke the protocol.
    Protocol(&'static str),
}

fn time_left(deadline: Instant) -> Result<Duration, ConnectError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
        .ok_or(ConnectError::Timeout)
}

fn connect_errno(errno: rustix::io::Errno) -> ConnectError {
    match io::Error::from(errno).kind() {
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => ConnectError::Timeout,
        kind => ConnectError::Io(kind),
    }
}

/// Writes one whole frame within `deadline`, a `set_write_timeout` per
/// attempt. Used for the blocking version handshake, before a stream
/// switches to nonblocking (`Client`'s own request/reply loop, already
/// nonblocking-aware through timeouts, does not need this).
pub(crate) fn send_blocking(
    stream: &mut UnixStream,
    kind: u8,
    tag: u16,
    body: &[u8],
    deadline: Instant,
    msize: u32,
) -> Result<(), ConnectError> {
    let size = HEADER
        .checked_add(body.len())
        .filter(|size| *size <= msize as usize)
        .ok_or(ConnectError::Limit("request larger than msize"))?;
    let mut frame = Vec::with_capacity(size);
    frame.extend_from_slice(&(size as u32).to_le_bytes());
    frame.push(kind);
    frame.extend_from_slice(&tag.to_le_bytes());
    frame.extend_from_slice(body);
    let mut written = 0;
    while written < frame.len() {
        stream
            .set_write_timeout(Some(time_left(deadline)?))
            .map_err(|e| ConnectError::Io(e.kind()))?;
        match stream.write(&frame[written..]) {
            Ok(0) => return Err(ConnectError::Io(io::ErrorKind::WriteZero)),
            Ok(count) => written += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(ConnectError::Timeout);
            }
            Err(error) => return Err(ConnectError::Io(error.kind())),
        }
    }
    Ok(())
}

/// Reads one whole frame within `deadline`, a `set_read_timeout` per
/// attempt. See [`send_blocking`].
pub(crate) fn receive_blocking(
    stream: &mut UnixStream,
    deadline: Instant,
    msize: u32,
) -> Result<(u8, u16, Vec<u8>), ConnectError> {
    let mut buffer = Vec::new();
    loop {
        let wanted = if buffer.len() < 4 {
            4
        } else {
            let size = u32::from_le_bytes(buffer[..4].try_into().unwrap()) as usize;
            if size < HEADER || size > msize as usize {
                return Err(ConnectError::Protocol("reply frame size"));
            }
            size
        };
        if buffer.len() == wanted && wanted >= HEADER {
            return Ok((
                buffer[4],
                u16::from_le_bytes([buffer[5], buffer[6]]),
                buffer[HEADER..].to_vec(),
            ));
        }
        stream
            .set_read_timeout(Some(time_left(deadline)?))
            .map_err(|e| ConnectError::Io(e.kind()))?;
        let mut chunk = vec![0u8; wanted - buffer.len()];
        match stream.read(&mut chunk) {
            Ok(0) => return Err(ConnectError::Io(io::ErrorKind::UnexpectedEof)),
            Ok(count) => buffer.extend_from_slice(&chunk[..count]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(ConnectError::Timeout);
            }
            Err(error) => return Err(ConnectError::Io(error.kind())),
        }
    }
}

/// Connects without blocking past `deadline`: a nonblocking connect, a poll
/// for completion and the socket's pending error. A listener whose queue is
/// full is retried until the deadline. The returned stream is still
/// `O_NONBLOCK` at the file-descriptor level; a caller doing a blocking
/// handshake over it sets its own blocking mode and timeouts first.
pub(crate) fn connect_by(path: &Path, deadline: Instant) -> Result<UnixStream, ConnectError> {
    let socket = rustix::net::socket_with(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::NONBLOCK | SocketFlags::CLOEXEC,
        None,
    )
    .map_err(connect_errno)?;
    let address = SocketAddrUnix::new(path).map_err(connect_errno)?;
    loop {
        match rustix::net::connect(&socket, &address) {
            Ok(()) => break,
            Err(rustix::io::Errno::INTR) => {}
            Err(rustix::io::Errno::AGAIN) => {
                std::thread::sleep(time_left(deadline)?.min(CONNECT_RETRY));
            }
            Err(rustix::io::Errno::INPROGRESS) => {
                let timeout = Timespec::try_from(time_left(deadline)?)
                    .map_err(|_| ConnectError::Limit("deadline overflow"))?;
                let mut fds = [PollFd::new(&socket, PollFlags::OUT)];
                if poll(&mut fds, Some(&timeout)).map_err(connect_errno)? == 0 {
                    return Err(ConnectError::Timeout);
                }
                rustix::net::sockopt::socket_error(&socket)
                    .map_err(connect_errno)?
                    .map_err(connect_errno)?;
                break;
            }
            Err(errno) => return Err(connect_errno(errno)),
        }
    }
    Ok(UnixStream::from(socket))
}
