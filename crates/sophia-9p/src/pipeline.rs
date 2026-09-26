//! A nonblocking, pipelined, write-capable 9P2000.L client over a Unix
//! stream socket, for Sophia's own role clients (the shell client's file
//! wire first; the admin CLI later).
//!
//! WHAT IT DOES. Blocking version negotiation within a deadline (the same
//! acceptance rules as [`crate::client::Client::over`]), then a stream
//! switched to nonblocking for the rest of its life. Requests are queued
//! without ever blocking -- attach, walk, lopen, read, write, clunk and
//! flush -- each returning the [`Tag`] it was sent under; [`Pipeline::poll`]
//! makes bounded nonblocking progress writing queued bytes and reading
//! replies, and [`Pipeline::take_reply`] drains completed replies in arrival
//! order. Every reply is checked against the request it answers: its tag
//! must be outstanding, its type must match (or be `Rlerror`), and its shape
//! exact. [`Pipeline::wait`] is a blocking convenience, via `poll(2)` on the
//! socket, for handshake-style call sequences that want one tag's answer
//! without hand-rolling a loop. It shares its request encoders and reply
//! decoders with [`crate::client::Client`] through the private
//! `crate::client_codec` module; it shares only the protocol's value records
//! with the server core, exactly as `Client` does.
//!
//! WHAT IT DOES NOT. It never resynchronises a stream it cannot trust: a
//! reply with an unmatched tag, a type that does not fit the request it
//! answers, a malformed body, an I/O error, or a `Tflush` answered with
//! anything but `Rflush`, poisons the pipeline, which shuts its socket and
//! refuses every later call. `Rlerror` is not such a failure: it is
//! delivered as [`Reply::Error`] like any other reply, and the pipeline
//! stays usable. It performs no admission or interpretation of its own on
//! fid or tag values; callers reuse [`crate::records::Fid`] and
//! [`crate::records::Tag`] however their own bookkeeping wants, subject only
//! to the bounds in [`PipelineLimits`].

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};

use crate::client_codec::{self, HEADER};
use crate::records::{Errno, Fid, Qid, Tag};

/// Nonblocking progress reads and writes at most this many bytes...
const IO_BYTE_BUDGET: usize = 256 * 1024;
/// ...in at most this many syscalls, per direction, per [`Pipeline::poll`]
/// call. The same bound the shell client's file wire uses.
const IO_SYSCALL_BUDGET: usize = 64;
/// One `read(2)`'s buffer.
const READ_CHUNK: usize = 4096;

/// Bounds a pipeline holds itself to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PipelineLimits {
    /// Offered at version; the server may answer smaller.
    pub msize: u32,
    /// Tags in flight at once: queued or sent but not yet matched to a
    /// reply. A tag is freed the moment its reply is matched -- whether or
    /// not [`Pipeline::take_reply`] has drained it yet -- except a flushed
    /// tag, kept reserved until the `Rflush` that settles it (see
    /// [`Pipeline::flush`]).
    pub max_outstanding: u16,
    /// Fids reserved at once. An attach or walk reserves its new fid the
    /// moment the request is queued, before any reply arrives, and frees it
    /// again if the reply turns out to be a partial walk or an error.
    pub max_fids: u32,
    /// Request bytes queued but not yet written to the socket.
    pub max_buffered_output: usize,
    /// Reply payload bytes matched but not yet taken by
    /// [`Pipeline::take_reply`]. Past this, [`Pipeline::poll`] stops reading
    /// further replies off the socket until some are taken; nothing is lost,
    /// progress just waits for the caller to drain.
    pub max_buffered_input: usize,
}

impl PipelineLimits {
    pub const MIN_MSIZE: u32 = client_codec::MIN_MSIZE;
    pub const MAX_MSIZE: u32 = client_codec::MAX_MSIZE;
    /// One less than the tag space: `NOTAG` can never be a real tag, so no
    /// more than this many can be outstanding at once.
    pub const MAX_OUTSTANDING: u16 = u16::MAX - 1;

    fn validate(&self) -> Result<(), PipelineError> {
        if !(Self::MIN_MSIZE..=Self::MAX_MSIZE).contains(&self.msize) {
            return Err(PipelineError::Limit("msize outside 4096..=16 MiB"));
        }
        if self.max_outstanding == 0 {
            return Err(PipelineError::Limit("no outstanding requests"));
        }
        if self.max_outstanding > Self::MAX_OUTSTANDING {
            return Err(PipelineError::Limit("max_outstanding too large"));
        }
        if self.max_fids == 0 {
            return Err(PipelineError::Limit("no fids"));
        }
        if (self.max_buffered_output as u64) < u64::from(self.msize) {
            return Err(PipelineError::Limit(
                "max_buffered_output smaller than msize",
            ));
        }
        if (self.max_buffered_input as u64) < u64::from(self.msize) {
            return Err(PipelineError::Limit(
                "max_buffered_input smaller than msize",
            ));
        }
        Ok(())
    }
}

impl Default for PipelineLimits {
    fn default() -> Self {
        Self {
            msize: 65536,
            max_outstanding: 128,
            max_fids: 128,
            max_buffered_output: 4 * 65536,
            max_buffered_input: 4 * 65536,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PipelineError {
    /// The socket failed; the pipeline is poisoned.
    Io(io::ErrorKind),
    /// The server broke the protocol; the pipeline is poisoned.
    Protocol(&'static str),
    /// An earlier failure poisoned the pipeline.
    Poisoned,
    /// A local bound or rule refused the call before anything was queued.
    Limit(&'static str),
    /// A [`Pipeline::wait`] deadline passed before its tag completed. The
    /// pipeline is still usable.
    Timeout,
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(kind) => write!(formatter, "9P socket: {kind}"),
            Self::Protocol(what) => write!(formatter, "9P protocol violation: {what}"),
            Self::Poisoned => formatter.write_str("9P pipeline poisoned by an earlier failure"),
            Self::Limit(what) => write!(formatter, "9P pipeline limit: {what}"),
            Self::Timeout => formatter.write_str("9P wait deadline passed"),
        }
    }
}

impl std::error::Error for PipelineError {}

/// One completed reply. `Error` is the server's `Rlerror`: a normal outcome,
/// not a pipeline failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Reply {
    Attach(Qid),
    Walk(Vec<Qid>),
    Lopen { qid: Qid, iounit: u32 },
    Read(Vec<u8>),
    Write(u32),
    Clunk,
    Flush,
    Error(Errno),
}

/// What a still-unanswered tag is waiting for, and enough of its request to
/// validate the reply's shape and settle fid bookkeeping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Outstanding {
    Attach {
        fid: u32,
    },
    Walk {
        newfid: u32,
        names: u16,
    },
    Lopen,
    Read {
        count: u32,
    },
    Write {
        sent: u32,
    },
    Clunk,
    Flush {
        old: u16,
    },
    /// `old`'s own answer already arrived and was delivered through the
    /// ordinary path; the tag stays reserved -- unusable by
    /// [`Pipeline::candidate_tag`] -- until the `Rflush` that settles it
    /// frees it. flush(5): a client must not reuse a flushed tag before it
    /// is answered.
    Settled,
}

pub struct Pipeline {
    stream: UnixStream,
    limits: PipelineLimits,
    msize: u32,
    next_tag: u16,
    next_fid: u32,
    fids: BTreeSet<u32>,
    in_flight: BTreeMap<u16, Outstanding>,
    /// Old tags a `Tflush` has been sent for and not yet settled.
    flushed: BTreeSet<u16>,
    completed: VecDeque<(Tag, Reply)>,
    /// Bytes of `Reply::Read` payloads sitting in `completed`.
    buffered_reply_bytes: usize,
    out_buf: Vec<u8>,
    in_buf: Vec<u8>,
    poisoned: bool,
}

impl Pipeline {
    /// Connects, then negotiates the version as [`Self::over`] does, both
    /// within `handshake_deadline`.
    pub fn connect(
        path: &Path,
        limits: PipelineLimits,
        handshake_deadline: Duration,
    ) -> Result<Self, PipelineError> {
        limits.validate()?;
        let deadline = deadline_after(handshake_deadline)?;
        let stream = client_codec::connect_by(path, deadline).map_err(connect_error)?;
        Self::over(stream, limits, handshake_deadline)
    }

    /// Negotiates the version over an already connected stream, blocking
    /// within `handshake_deadline`, then switches it to nonblocking for
    /// every later call.
    pub fn over(
        mut stream: UnixStream,
        limits: PipelineLimits,
        handshake_deadline: Duration,
    ) -> Result<Self, PipelineError> {
        limits.validate()?;
        stream.set_nonblocking(false).map_err(io_err)?;
        let deadline = deadline_after(handshake_deadline)?;
        let msize = negotiate(&mut stream, limits.msize, deadline)?;
        stream.set_read_timeout(None).map_err(io_err)?;
        stream.set_write_timeout(None).map_err(io_err)?;
        stream.set_nonblocking(true).map_err(io_err)?;
        Ok(Self {
            stream,
            limits,
            msize,
            next_tag: 0,
            next_fid: 0,
            fids: BTreeSet::new(),
            in_flight: BTreeMap::new(),
            flushed: BTreeSet::new(),
            completed: VecDeque::new(),
            buffered_reply_bytes: 0,
            out_buf: Vec::new(),
            in_buf: Vec::new(),
            poisoned: false,
        })
    }

    pub const fn msize(&self) -> u32 {
        self.msize
    }

    pub const fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    /// Tags currently outstanding (queued, sent, or reserved by a flush not
    /// yet settled).
    pub fn outstanding(&self) -> usize {
        self.in_flight.len()
    }

    /// Fids currently reserved (attached or walked to, not yet clunked or
    /// freed by a partial walk or an error).
    pub fn fid_count(&self) -> usize {
        self.fids.len()
    }

    /// Queues an attach to the root the server gives this connection (`afid`
    /// is always `NOFID`: neither side authenticates through the protocol).
    pub fn attach(&mut self, uname: &[u8], aname: &[u8]) -> Result<(Tag, Fid), PipelineError> {
        self.check_capacity(true)?;
        let fid = self.candidate_fid();
        let tag = self.candidate_tag();
        let body = client_codec::attach_body(fid, client_codec::NOFID, uname, aname)
            .map_err(PipelineError::Limit)?;
        self.submit(
            client_codec::TATTACH,
            tag,
            body,
            Outstanding::Attach { fid },
        )?;
        self.fids.insert(fid);
        self.commit_fid(fid);
        self.commit_tag(tag);
        Ok((Tag(tag), Fid(fid)))
    }

    /// Queues a walk from `from` to a newly allocated fid. At most sixteen
    /// names. The reply's qid count decides the new fid's fate: see the
    /// module doc and [`Reply::Walk`].
    pub fn walk(&mut self, from: Fid, names: &[&[u8]]) -> Result<(Tag, Fid), PipelineError> {
        if names.len() > client_codec::MAX_WALK {
            return Err(PipelineError::Limit("more than sixteen names"));
        }
        self.check_capacity(true)?;
        let newfid = self.candidate_fid();
        let tag = self.candidate_tag();
        let body = client_codec::walk_body(from.0, newfid, names).map_err(PipelineError::Limit)?;
        let names = names.len() as u16;
        self.submit(
            client_codec::TWALK,
            tag,
            body,
            Outstanding::Walk { newfid, names },
        )?;
        self.fids.insert(newfid);
        self.commit_fid(newfid);
        self.commit_tag(tag);
        Ok((Tag(tag), Fid(newfid)))
    }

    /// Queues an open of `fid`.
    pub fn lopen(&mut self, fid: Fid, flags: u32) -> Result<Tag, PipelineError> {
        self.check_capacity(false)?;
        let tag = self.candidate_tag();
        let body = client_codec::lopen_body(fid.0, flags);
        self.submit(client_codec::TLOPEN, tag, body, Outstanding::Lopen)?;
        self.commit_tag(tag);
        Ok(Tag(tag))
    }

    /// Queues a read, clamped to what one reply can carry (`msize -
    /// READ_OVERHEAD`), like [`crate::client::Client::read`]'s own clamp.
    pub fn read(&mut self, fid: Fid, offset: u64, count: u32) -> Result<Tag, PipelineError> {
        self.check_capacity(false)?;
        let count = count.min(self.msize - client_codec::READ_OVERHEAD);
        let tag = self.candidate_tag();
        let body = client_codec::io_body(fid.0, offset, count);
        self.submit(client_codec::TREAD, tag, body, Outstanding::Read { count })?;
        self.commit_tag(tag);
        Ok(Tag(tag))
    }

    /// Queues a write. `data` must fit `msize`; a `data` that would not is
    /// refused here, before anything is queued.
    pub fn write(&mut self, fid: Fid, offset: u64, data: &[u8]) -> Result<Tag, PipelineError> {
        let sent = u32::try_from(data.len())
            .map_err(|_| PipelineError::Limit("write larger than msize"))?;
        self.check_capacity(false)?;
        let tag = self.candidate_tag();
        let body = client_codec::write_body(fid.0, offset, sent, data);
        self.submit(client_codec::TWRITE, tag, body, Outstanding::Write { sent })?;
        self.commit_tag(tag);
        Ok(Tag(tag))
    }

    /// Queues a clunk. 9P requires a clunk to release the fid whatever the
    /// server later answers (clunk(5)); this pipeline frees the fid the
    /// moment the request is queued, not when the reply arrives, matching
    /// [`crate::client::Client::clunk`]'s own choice.
    pub fn clunk(&mut self, fid: Fid) -> Result<Tag, PipelineError> {
        self.check_capacity(false)?;
        let tag = self.candidate_tag();
        let body = client_codec::clunk_body(fid.0);
        self.submit(client_codec::TCLUNK, tag, body, Outstanding::Clunk)?;
        self.fids.remove(&fid.0);
        self.commit_tag(tag);
        Ok(Tag(tag))
    }

    /// Queues a flush of `old`, which must currently be outstanding (queued
    /// or sent, not itself a flush, not already settled, and not already
    /// being flushed). `old`'s own reply, if it arrives before this
    /// flush's `Rflush`, is still delivered through [`Self::take_reply`];
    /// if it never arrives, `old` is freed, unanswered, when `Rflush`
    /// arrives. Either way `old` cannot be reused until then. See flush(5).
    pub fn flush(&mut self, old: Tag) -> Result<Tag, PipelineError> {
        self.check_capacity(false)?;
        match self.in_flight.get(&old.0) {
            None | Some(Outstanding::Flush { .. }) | Some(Outstanding::Settled) => {
                return Err(PipelineError::Limit("tag not outstanding"));
            }
            Some(_) => {}
        }
        if self.flushed.contains(&old.0) {
            return Err(PipelineError::Limit("tag already being flushed"));
        }
        let tag = self.candidate_tag();
        let body = client_codec::flush_body(old.0);
        self.submit(
            client_codec::TFLUSH,
            tag,
            body,
            Outstanding::Flush { old: old.0 },
        )?;
        self.flushed.insert(old.0);
        self.commit_tag(tag);
        Ok(Tag(tag))
    }

    /// Bounded nonblocking progress: writes queued request bytes and reads
    /// replies, at most [`IO_BYTE_BUDGET`] bytes in at most
    /// [`IO_SYSCALL_BUDGET`] syscalls per direction.
    pub fn poll(&mut self) -> Result<(), PipelineError> {
        if self.poisoned {
            return Err(PipelineError::Poisoned);
        }
        self.write_output()?;
        self.read_input()?;
        Ok(())
    }

    /// The next completed reply, in the order its frame arrived.
    pub fn take_reply(&mut self) -> Option<(Tag, Reply)> {
        let entry = self.completed.pop_front()?;
        if let (_, Reply::Read(data)) = &entry {
            self.buffered_reply_bytes = self.buffered_reply_bytes.saturating_sub(data.len());
        }
        Some(entry)
    }

    /// Polls until `tag` completes or `deadline` passes, blocking on the
    /// socket via `poll(2)` in between; replies for other tags are buffered
    /// for [`Self::take_reply`], in arrival order, exactly as `poll` would
    /// leave them.
    pub fn wait(&mut self, tag: Tag, deadline: Instant) -> Result<Reply, PipelineError> {
        loop {
            if let Some(index) = self.completed.iter().position(|(found, _)| *found == tag) {
                let (_, reply) = self.completed.remove(index).unwrap();
                if let Reply::Read(data) = &reply {
                    self.buffered_reply_bytes =
                        self.buffered_reply_bytes.saturating_sub(data.len());
                }
                return Ok(reply);
            }
            if self.poisoned {
                return Err(PipelineError::Poisoned);
            }
            let left = remaining(deadline)?;
            let timeout =
                Timespec::try_from(left).map_err(|_| PipelineError::Limit("deadline overflow"))?;
            let mut flags = PollFlags::IN;
            if !self.out_buf.is_empty() {
                flags |= PollFlags::OUT;
            }
            let mut fds = [PollFd::new(&self.stream, flags)];
            match poll(&mut fds, Some(&timeout)) {
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => continue,
                Err(errno) => return Err(self.poison(io_err(io::Error::from(errno)))),
            }
            self.poll()?;
        }
    }

    fn check_capacity(&self, needs_fid: bool) -> Result<(), PipelineError> {
        if self.poisoned {
            return Err(PipelineError::Poisoned);
        }
        if self.in_flight.len() >= self.limits.max_outstanding as usize {
            return Err(PipelineError::Limit("max outstanding requests reached"));
        }
        if needs_fid && self.fids.len() >= self.limits.max_fids as usize {
            return Err(PipelineError::Limit("max fids reached"));
        }
        Ok(())
    }

    /// The tag [`Self::commit_tag`] would make live, without reserving it:
    /// nothing changes until the request that uses it is actually queued.
    fn candidate_tag(&self) -> u16 {
        let mut tag = self.next_tag;
        loop {
            if tag != client_codec::NOTAG && !self.in_flight.contains_key(&tag) {
                return tag;
            }
            tag = if tag >= client_codec::NOTAG - 1 {
                0
            } else {
                tag + 1
            };
        }
    }

    fn commit_tag(&mut self, tag: u16) {
        self.next_tag = if tag >= client_codec::NOTAG - 1 {
            0
        } else {
            tag + 1
        };
    }

    /// The fid [`Self::commit_fid`] would make live, without reserving it.
    fn candidate_fid(&self) -> u32 {
        let mut fid = self.next_fid;
        while fid == client_codec::NOFID || self.fids.contains(&fid) {
            fid = fid.wrapping_add(1);
        }
        fid
    }

    fn commit_fid(&mut self, fid: u32) {
        self.next_fid = fid.wrapping_add(1);
    }

    /// Checks the frame fits `msize` and the output buffer, then queues it
    /// and records `outstanding`. Nothing here is undone on failure because
    /// nothing before it mutated anything: a caller reserves a fid slot only
    /// by inserting into `self.fids`, which it does after this succeeds.
    fn submit(
        &mut self,
        kind: u8,
        tag: u16,
        body: Vec<u8>,
        outstanding: Outstanding,
    ) -> Result<(), PipelineError> {
        let frame_len = HEADER + body.len();
        if frame_len > self.msize as usize {
            return Err(PipelineError::Limit("request larger than msize"));
        }
        if self.out_buf.len() + frame_len > self.limits.max_buffered_output {
            return Err(PipelineError::Limit("output buffer full"));
        }
        self.out_buf.reserve(frame_len);
        self.out_buf
            .extend_from_slice(&(frame_len as u32).to_le_bytes());
        self.out_buf.push(kind);
        self.out_buf.extend_from_slice(&tag.to_le_bytes());
        self.out_buf.extend_from_slice(&body);
        self.in_flight.insert(tag, outstanding);
        Ok(())
    }

    fn write_output(&mut self) -> Result<(), PipelineError> {
        let mut budget = IO_BYTE_BUDGET;
        for _ in 0..IO_SYSCALL_BUDGET {
            if self.out_buf.is_empty() || budget == 0 {
                break;
            }
            let take = self.out_buf.len().min(budget);
            match self.stream.write(&self.out_buf[..take]) {
                Ok(0) => return Err(self.poison(PipelineError::Io(io::ErrorKind::WriteZero))),
                Ok(count) => {
                    self.out_buf.drain(..count);
                    budget -= count;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(self.poison(PipelineError::Io(error.kind()))),
            }
        }
        Ok(())
    }

    fn read_input(&mut self) -> Result<(), PipelineError> {
        let mut budget = IO_BYTE_BUDGET;
        for _ in 0..IO_SYSCALL_BUDGET {
            self.drain_frames()?;
            if budget == 0 || self.buffered_reply_bytes >= self.limits.max_buffered_input {
                break;
            }
            let mut chunk = [0u8; READ_CHUNK];
            let want = chunk.len().min(budget);
            match self.stream.read(&mut chunk[..want]) {
                Ok(0) => return Err(self.poison(PipelineError::Io(io::ErrorKind::UnexpectedEof))),
                Ok(count) => {
                    self.in_buf.extend_from_slice(&chunk[..count]);
                    budget -= count;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(self.poison(PipelineError::Io(error.kind()))),
            }
        }
        self.drain_frames()
    }

    /// Parses as many complete frames as `in_buf` holds; an incomplete tail
    /// is kept for the next read.
    fn drain_frames(&mut self) -> Result<(), PipelineError> {
        loop {
            if self.in_buf.len() < 4 {
                return Ok(());
            }
            let size = u32::from_le_bytes(self.in_buf[..4].try_into().unwrap()) as usize;
            if size < HEADER || size > self.msize as usize {
                return Err(self.poison(PipelineError::Protocol("reply frame size")));
            }
            if self.in_buf.len() < size {
                return Ok(());
            }
            let frame: Vec<u8> = self.in_buf.drain(..size).collect();
            let kind = frame[4];
            let tag = u16::from_le_bytes([frame[5], frame[6]]);
            let body = frame[HEADER..].to_vec();
            self.on_frame(kind, tag, body)?;
        }
    }

    /// Matches one decoded frame to its outstanding request, validates its
    /// shape and type exactly, and either delivers it or poisons the
    /// pipeline.
    fn on_frame(&mut self, kind: u8, tag: u16, body: Vec<u8>) -> Result<(), PipelineError> {
        let Some(outstanding) = self.in_flight.get(&tag).copied() else {
            return Err(self.poison(PipelineError::Protocol("reply to a tag not outstanding")));
        };
        if kind == client_codec::RLERROR {
            let Some(errno) = client_codec::decode_rlerror(&body) else {
                return Err(self.poison(PipelineError::Protocol("Rlerror shape")));
            };
            match outstanding {
                Outstanding::Settled => {
                    return Err(
                        self.poison(PipelineError::Protocol("reply to an already-settled tag"))
                    );
                }
                // Tflush can never fail (flush(5)): an Rlerror answering one
                // is itself a protocol violation.
                Outstanding::Flush { .. } => {
                    return Err(self.poison(PipelineError::Protocol("Rlerror answering Tflush")));
                }
                Outstanding::Attach { fid } => {
                    self.fids.remove(&fid);
                }
                Outstanding::Walk { newfid, .. } => {
                    self.fids.remove(&newfid);
                }
                Outstanding::Lopen
                | Outstanding::Read { .. }
                | Outstanding::Write { .. }
                | Outstanding::Clunk => {}
            }
            self.resolve(tag, Reply::Error(errno));
            return Ok(());
        }
        match outstanding {
            Outstanding::Settled => {
                Err(self.poison(PipelineError::Protocol("reply to an already-settled tag")))
            }
            Outstanding::Attach { .. } => {
                if kind != client_codec::RATTACH {
                    return Err(self.poison(PipelineError::Protocol("unexpected reply type")));
                }
                let Some(qid) = client_codec::decode_rattach(&body) else {
                    return Err(self.poison(PipelineError::Protocol("Rattach shape")));
                };
                self.resolve(tag, Reply::Attach(qid));
                Ok(())
            }
            Outstanding::Walk { newfid, names } => {
                if kind != client_codec::RWALK {
                    return Err(self.poison(PipelineError::Protocol("unexpected reply type")));
                }
                let Some(qids) = client_codec::decode_rwalk(&body) else {
                    return Err(self.poison(PipelineError::Protocol("Rwalk shape")));
                };
                if qids.len() > names as usize {
                    return Err(self.poison(PipelineError::Protocol("Rwalk count")));
                }
                if qids.len() < names as usize {
                    self.fids.remove(&newfid);
                }
                self.resolve(tag, Reply::Walk(qids));
                Ok(())
            }
            Outstanding::Lopen => {
                if kind != client_codec::RLOPEN {
                    return Err(self.poison(PipelineError::Protocol("unexpected reply type")));
                }
                let Some((qid, iounit)) = client_codec::decode_rlopen(&body) else {
                    return Err(self.poison(PipelineError::Protocol("Rlopen shape")));
                };
                self.resolve(tag, Reply::Lopen { qid, iounit });
                Ok(())
            }
            Outstanding::Read { count } => {
                if kind != client_codec::RREAD {
                    return Err(self.poison(PipelineError::Protocol("unexpected reply type")));
                }
                let Some(data) = client_codec::decode_rread(&body) else {
                    return Err(self.poison(PipelineError::Protocol("Rread shape")));
                };
                if data.len() > count as usize {
                    return Err(self.poison(PipelineError::Protocol("Rread count")));
                }
                self.resolve(tag, Reply::Read(data));
                Ok(())
            }
            Outstanding::Write { sent } => {
                if kind != client_codec::RWRITE {
                    return Err(self.poison(PipelineError::Protocol("unexpected reply type")));
                }
                let Some(count) = client_codec::decode_rwrite(&body) else {
                    return Err(self.poison(PipelineError::Protocol("Rwrite shape")));
                };
                if count > sent {
                    return Err(self.poison(PipelineError::Protocol("Rwrite count")));
                }
                self.resolve(tag, Reply::Write(count));
                Ok(())
            }
            Outstanding::Clunk => {
                if kind != client_codec::RCLUNK {
                    return Err(self.poison(PipelineError::Protocol("unexpected reply type")));
                }
                let Some(()) = client_codec::decode_rclunk(&body) else {
                    return Err(self.poison(PipelineError::Protocol("Rclunk shape")));
                };
                self.resolve(tag, Reply::Clunk);
                Ok(())
            }
            Outstanding::Flush { old } => {
                if kind != client_codec::RFLUSH {
                    return Err(self.poison(PipelineError::Protocol("unexpected reply type")));
                }
                let Some(()) = client_codec::decode_rflush(&body) else {
                    return Err(self.poison(PipelineError::Protocol("Rflush shape")));
                };
                // Whether `old` was already `Settled` (its own reply arrived
                // first) or is still its original entry (never answered),
                // this `Rflush` is what frees it either way.
                self.flushed.remove(&old);
                self.in_flight.remove(&old);
                self.resolve(tag, Reply::Flush);
                Ok(())
            }
        }
    }

    /// Delivers `reply` for `tag`, in arrival order. A tag under an
    /// outstanding flush is not freed here: it is marked [`Outstanding::Settled`]
    /// instead, so it stays unusable until that flush's `Rflush` frees it.
    fn resolve(&mut self, tag: u16, reply: Reply) {
        if self.flushed.contains(&tag) {
            self.in_flight.insert(tag, Outstanding::Settled);
        } else {
            self.in_flight.remove(&tag);
        }
        if let Reply::Read(data) = &reply {
            self.buffered_reply_bytes += data.len();
        }
        self.completed.push_back((Tag(tag), reply));
    }

    /// Marks the pipeline unusable and closes its socket.
    fn poison(&mut self, error: PipelineError) -> PipelineError {
        if !self.poisoned {
            self.poisoned = true;
            let _ = self.stream.shutdown(std::net::Shutdown::Both);
        }
        error
    }
}

impl AsFd for Pipeline {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.stream.as_fd()
    }
}

fn connect_error(error: client_codec::ConnectError) -> PipelineError {
    match error {
        client_codec::ConnectError::Io(kind) => PipelineError::Io(kind),
        client_codec::ConnectError::Timeout => PipelineError::Timeout,
        client_codec::ConnectError::Limit(what) => PipelineError::Limit(what),
    }
}

fn io_err(error: io::Error) -> PipelineError {
    PipelineError::Io(error.kind())
}

fn deadline_after(duration: Duration) -> Result<Instant, PipelineError> {
    Instant::now()
        .checked_add(duration)
        .ok_or(PipelineError::Limit("deadline overflow"))
}

/// The time left before `deadline`, or `Timeout` when none is.
fn remaining(deadline: Instant) -> Result<Duration, PipelineError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
        .ok_or(PipelineError::Timeout)
}

/// The blocking version negotiation `connect`/`over` do before switching the
/// stream to nonblocking, applying [`client_codec::accept_rversion`]'s rules
/// -- the same ones [`crate::client::Client::over`] applies.
fn negotiate(
    stream: &mut UnixStream,
    offered_msize: u32,
    deadline: Instant,
) -> Result<u32, PipelineError> {
    let body = client_codec::version_body(offered_msize);
    send_blocking(
        stream,
        client_codec::TVERSION,
        client_codec::NOTAG,
        &body,
        deadline,
        offered_msize,
    )?;
    let (kind, tag, body) = receive_blocking(stream, deadline, offered_msize)?;
    if tag != client_codec::NOTAG || kind != client_codec::RVERSION {
        return Err(PipelineError::Protocol("Rversion shape"));
    }
    let Some((msize, version)) = client_codec::decode_rversion(&body) else {
        return Err(PipelineError::Protocol("Rversion shape"));
    };
    client_codec::accept_rversion(offered_msize, msize, version).map_err(PipelineError::Protocol)
}

fn send_blocking(
    stream: &mut UnixStream,
    kind: u8,
    tag: u16,
    body: &[u8],
    deadline: Instant,
    msize: u32,
) -> Result<(), PipelineError> {
    let size = HEADER
        .checked_add(body.len())
        .filter(|size| *size <= msize as usize)
        .ok_or(PipelineError::Limit("request larger than msize"))?;
    let mut frame = Vec::with_capacity(size);
    frame.extend_from_slice(&(size as u32).to_le_bytes());
    frame.push(kind);
    frame.extend_from_slice(&tag.to_le_bytes());
    frame.extend_from_slice(body);
    let mut written = 0;
    while written < frame.len() {
        let left = remaining(deadline)?;
        stream.set_write_timeout(Some(left)).map_err(io_err)?;
        match stream.write(&frame[written..]) {
            Ok(0) => return Err(PipelineError::Io(io::ErrorKind::WriteZero)),
            Ok(count) => written += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(PipelineError::Timeout);
            }
            Err(error) => return Err(io_err(error)),
        }
    }
    Ok(())
}

fn receive_blocking(
    stream: &mut UnixStream,
    deadline: Instant,
    msize: u32,
) -> Result<(u8, u16, Vec<u8>), PipelineError> {
    let mut buffer = Vec::new();
    loop {
        let wanted = if buffer.len() < 4 {
            4
        } else {
            let size = u32::from_le_bytes(buffer[..4].try_into().unwrap()) as usize;
            if size < HEADER || size > msize as usize {
                return Err(PipelineError::Protocol("reply frame size"));
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
        let left = remaining(deadline)?;
        stream.set_read_timeout(Some(left)).map_err(io_err)?;
        let mut chunk = vec![0u8; wanted - buffer.len()];
        match stream.read(&mut chunk) {
            Ok(0) => return Err(PipelineError::Io(io::ErrorKind::UnexpectedEof)),
            Ok(count) => buffer.extend_from_slice(&chunk[..count]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(PipelineError::Timeout);
            }
            Err(error) => return Err(io_err(error)),
        }
    }
}
