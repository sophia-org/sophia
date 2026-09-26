//! One connection's protocol state, without I/O.
//!
//! Bytes go in through [`Connection::receive`]; reply frames come out of
//! [`Connection::output`]. Requests are answered in arrival order, one at a
//! time, except reads the export makes wait, which are answered when
//! [`Connection::retry_waiting`] finds them ready or they are flushed,
//! clunked, or cancelled by a new version or by [`Connection::close`].
//!
//! BOUNDED OUTPUT. Before a request reaches the export, room for its largest
//! possible reply is required in the unsent output. A request without room
//! stays in the input, unprocessed, until the peer reads, so no export
//! operation is ever performed, and no event consumed, whose reply could not
//! be kept. The same holds for every retried read.

use std::collections::{BTreeMap, VecDeque};

use crate::export::{
    Access, AttachContext, Epoch, Export, NodeKind, Operation, PeerCredentials, ReadOutcome,
    WalkName,
};
use crate::records::{Attr, Errno, Fid, Limits, OpenAccess, OpenFlags, Reply, Request, Tag};
use crate::wire::{self, FrameError, IO_HEADER, READ_OVERHEAD};

/// Identifies a connection to the export, for as long as it lives.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ConnectionId(pub u64);

/// A violation after which the stream cannot be trusted. The connection must
/// be closed; nothing more is answered on it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fatal {
    Frame(FrameError),
    /// A request other than a version negotiation before one succeeded.
    BeforeVersion {
        kind: u8,
    },
    /// A request reusing the tag of one still unanswered.
    DuplicateTag(Tag),
    /// `NOTAG` on a request other than a version negotiation.
    NoTag {
        kind: u8,
    },
    /// A reply this connection could not frame.
    Unframeable,
}

/// The only dialect this core speaks.
const DIALECT: &[u8] = b"9P2000.L";

/// The plain dialect, or it offered with Google's numbered extensions.
fn accepts_dialect(version: &[u8]) -> bool {
    match version.strip_prefix(DIALECT) {
        Some(b"") => true,
        Some(rest) => rest
            .strip_prefix(b".Google.")
            .is_some_and(|number| !number.is_empty() && number.iter().all(u8::is_ascii_digit)),
        None => false,
    }
}

/// The largest reply to any request except a read: `Rgetattr`, 160 bytes, is
/// the widest, with `Rwalk` of sixteen qids at 217.
const SMALL_REPLY: usize = 256;
/// An `Rlerror`: header and error number.
const ERROR_REPLY: usize = 11;

/// `st_mode` file types.
const S_IFDIR: u32 = 0o040000;
const S_IFREG: u32 = 0o100000;

struct FidState<Node, Handle> {
    node: Node,
    /// The attach root this fid descends from, which `..` cannot leave.
    root: Node,
    epoch: Epoch,
    open: Option<Opened<Handle>>,
}

struct Opened<Handle> {
    handle: Handle,
    access: OpenAccess,
    kind: NodeKind,
}

#[derive(Clone, Copy)]
struct Waiting {
    tag: Tag,
    fid: Fid,
    offset: u64,
    count: u32,
}

pub struct Connection<E: Export> {
    id: ConnectionId,
    peer: Option<PeerCredentials>,
    limits: Limits,
    msize: Option<u32>,
    fids: BTreeMap<Fid, FidState<E::Node, E::Handle>>,
    waiting: VecDeque<Waiting>,
    input: Vec<u8>,
    output: Vec<u8>,
    /// The tag and unwritten length of each reply in `output`, in order. A
    /// tag stays in use until its whole reply is written: before that the
    /// client cannot have it, so a request reusing the tag is a violation.
    unwritten: VecDeque<(Tag, usize)>,
    /// A complete request is buffered and waits for output room.
    stalled: bool,
    closed: bool,
}

impl<E: Export> Connection<E> {
    pub fn new(id: ConnectionId, peer: Option<PeerCredentials>, limits: Limits) -> Self {
        Self {
            id,
            peer,
            limits,
            msize: None,
            fids: BTreeMap::new(),
            waiting: VecDeque::new(),
            input: Vec::new(),
            output: Vec::new(),
            unwritten: VecDeque::new(),
            stalled: false,
            closed: false,
        }
    }

    pub const fn id(&self) -> ConnectionId {
        self.id
    }

    /// The negotiated message size, once a version negotiation succeeded.
    pub const fn msize(&self) -> Option<u32> {
        self.msize
    }

    pub fn fid_count(&self) -> usize {
        self.fids.len()
    }

    pub fn waiting_count(&self) -> usize {
        self.waiting.len()
    }

    /// Reply bytes not yet written.
    pub fn output(&self) -> &[u8] {
        &self.output
    }

    /// The peer took `count` bytes of [`Self::output`]. Call
    /// [`Self::resume`] and [`Self::retry_waiting`] afterwards: requests held
    /// for want of room may now proceed.
    pub fn sent(&mut self, count: usize) {
        let mut count = count.min(self.output.len());
        self.output.drain(..count);
        while let Some((_, remaining)) = self.unwritten.front_mut() {
            if count < *remaining {
                *remaining -= count;
                break;
            }
            count -= *remaining;
            self.unwritten.pop_front();
        }
    }

    /// How many more input bytes this connection will buffer now. Zero while
    /// a complete request waits for output room, so a driver stops reading.
    pub fn input_room(&self) -> usize {
        if self.closed || self.stalled {
            return 0;
        }
        (self.frame_limit() as usize).saturating_sub(self.input.len())
    }

    /// Takes bytes from the peer and answers every complete request that has
    /// room for its reply. Returns how many of `bytes` were taken: at most
    /// [`Self::input_room`]. The caller keeps the rest and offers it again
    /// once there is room; nothing offered is lost.
    pub fn receive(&mut self, export: &mut E, bytes: &[u8]) -> Result<usize, Fatal> {
        let accepted = bytes.len().min(self.input_room());
        self.input.extend_from_slice(&bytes[..accepted]);
        self.resume(export)?;
        Ok(accepted)
    }

    /// Answers buffered requests that now have room.
    pub fn resume(&mut self, export: &mut E) -> Result<(), Fatal> {
        if self.closed {
            return Ok(());
        }
        let input = std::mem::take(&mut self.input);
        let mut consumed = 0;
        let result = self.process(export, &input, &mut consumed);
        self.input = input;
        self.input.drain(..consumed);
        result
    }

    /// Asks the export again for each waiting read, in arrival order, while
    /// there is room for its reply. Each retry is checked against its epoch
    /// first, so revocation ends a waiting read.
    pub fn retry_waiting(&mut self, export: &mut E) -> Result<(), Fatal> {
        let mut index = 0;
        while index < self.waiting.len() {
            let waiting = self.waiting[index];
            if !self.has_room(READ_OVERHEAD as usize + waiting.count as usize) {
                break;
            }
            match self.read_now(export, waiting.fid, waiting.offset, waiting.count) {
                Some(reply) => {
                    self.waiting.remove(index);
                    self.send(waiting.tag, &reply)?;
                }
                None => index += 1,
            }
        }
        Ok(())
    }

    /// Ends the connection: waiting reads are dropped unanswered and every
    /// fid is released to the export.
    pub fn close(&mut self, export: &mut E) {
        self.reset(export);
        self.input.clear();
        self.output.clear();
        self.unwritten.clear();
        self.closed = true;
    }

    fn frame_limit(&self) -> u32 {
        self.msize.unwrap_or(self.limits.max_msize())
    }

    fn has_room(&self, reply: usize) -> bool {
        self.output.len() + reply <= self.limits.max_unsent()
    }

    fn process(&mut self, export: &mut E, input: &[u8], consumed: &mut usize) -> Result<(), Fatal> {
        self.stalled = false;
        loop {
            let rest = &input[*consumed..];
            let Some(prefix) = rest.first_chunk::<4>() else {
                return Ok(());
            };
            let length = wire::frame_length(*prefix, self.frame_limit()).map_err(Fatal::Frame)?;
            let Some(frame) = rest.get(..length) else {
                return Ok(());
            };
            let Some((kind, tag)) = wire::header(frame) else {
                return Err(Fatal::Frame(FrameError::Short(length as u32)));
            };
            if kind != wire::kind::TVERSION {
                if tag == Tag::NOTAG {
                    return Err(Fatal::NoTag { kind });
                }
                if self.msize.is_none() {
                    return Err(Fatal::BeforeVersion { kind });
                }
                if self.waiting.iter().any(|waiting| waiting.tag == tag)
                    || self
                        .unwritten
                        .iter()
                        .any(|(unwritten, _)| *unwritten == tag)
                {
                    return Err(Fatal::DuplicateTag(tag));
                }
            }
            let decoded = wire::decode(frame);
            if !self.has_room(self.reply_bound(&decoded)) {
                self.stalled = true;
                return Ok(());
            }
            *consumed += length;
            match decoded {
                Err(malformed) => self.send(tag, &Reply::Lerror(malformed.errno))?,
                Ok((_, request)) => {
                    if let Some(reply) = self.answer(export, tag, request)? {
                        self.send(tag, &reply)?;
                    }
                }
            }
        }
    }

    /// The largest reply a request can need, including the errors a clunk
    /// sends to reads waiting on its fid.
    fn reply_bound(&self, decoded: &Result<(Tag, Request<'_>), wire::Malformed>) -> usize {
        match decoded {
            Ok((_, Request::Read { count, .. })) => {
                READ_OVERHEAD as usize + self.read_count(*count) as usize
            }
            Ok((_, Request::Clunk { fid } | Request::Remove { fid })) => {
                SMALL_REPLY + ERROR_REPLY * self.waiting_on(*fid)
            }
            _ => SMALL_REPLY,
        }
    }

    fn waiting_on(&self, fid: Fid) -> usize {
        self.waiting
            .iter()
            .filter(|waiting| waiting.fid == fid)
            .count()
    }

    fn read_count(&self, count: u32) -> u32 {
        count.min(self.frame_limit() - READ_OVERHEAD)
    }

    fn send(&mut self, tag: Tag, reply: &Reply) -> Result<(), Fatal> {
        let start = self.output.len();
        wire::encode(tag, reply, &mut self.output).map_err(|_| Fatal::Unframeable)?;
        self.unwritten.push_back((tag, self.output.len() - start));
        Ok(())
    }

    /// `None` when the request is answered later, or never (a flushed read).
    fn answer(
        &mut self,
        export: &mut E,
        tag: Tag,
        request: Request<'_>,
    ) -> Result<Option<Reply>, Fatal> {
        let reply = match request {
            Request::Version { msize, version } => self.version(export, msize, version),
            Request::Attach {
                fid,
                afid,
                uname,
                aname,
                n_uname,
            } => self.attach(export, fid, afid, uname, aname, n_uname),
            Request::Flush { old } => {
                self.waiting.retain(|waiting| waiting.tag != old);
                Reply::Flush
            }
            Request::Walk { fid, newfid, names } => self.walk(export, fid, newfid, &names),
            Request::Lopen { fid, flags } => self.open(export, fid, flags),
            Request::Read { fid, offset, count } => {
                return Ok(self.read(export, tag, fid, offset, count));
            }
            Request::Write { fid, offset, data } => self.write(export, fid, offset, data),
            Request::Clunk { fid } => self.clunk(export, fid, Reply::Clunk)?,
            // Remove clunks its fid even though the removal itself is refused.
            Request::Remove { fid } => self.clunk(export, fid, Reply::Lerror(Errno::EOPNOTSUPP))?,
            Request::Getattr { fid, .. } => self.getattr(export, fid),
            Request::Refused { errno, .. } => Reply::Lerror(errno),
        };
        Ok(Some(reply))
    }

    /// A version negotiation ends the previous session whatever its outcome.
    /// The plain dialect is accepted. An offer of the dialect with Google's
    /// numbered extensions (`9P2000.L.Google.N`, which the pinned Go client
    /// sends) is answered with the plain dialect: the lower protocol, with no
    /// extension claimed. Anything else is `unknown`. No reply echoes a suffix.
    fn version(&mut self, export: &mut E, msize: u32, version: &[u8]) -> Reply {
        self.reset(export);
        let offered = msize.min(self.limits.max_msize());
        if !accepts_dialect(version) {
            return Reply::Version {
                msize: offered,
                version: b"unknown",
            };
        }
        if msize < self.limits.min_msize() {
            return Reply::Lerror(Errno::EINVAL);
        }
        self.msize = Some(offered);
        Reply::Version {
            msize: offered,
            version: DIALECT,
        }
    }

    fn reset(&mut self, export: &mut E) {
        self.waiting.clear();
        self.msize = None;
        for (_, state) in std::mem::take(&mut self.fids) {
            export.release(state.node, state.open.map(|opened| opened.handle));
        }
    }

    fn attach(
        &mut self,
        export: &mut E,
        fid: Fid,
        afid: Fid,
        uname: &[u8],
        aname: &[u8],
        n_uname: u32,
    ) -> Reply {
        if fid == Fid::NOFID || self.fids.contains_key(&fid) {
            return Reply::Lerror(Errno::EBADF);
        }
        if afid != Fid::NOFID {
            return Reply::Lerror(Errno::EINVAL);
        }
        if self.fids.len() >= self.limits.max_fids() {
            return Reply::Lerror(Errno::EMFILE);
        }
        let context = AttachContext {
            connection: self.id,
            peer: self.peer,
            uname,
            aname,
            n_uname,
        };
        match export.attach(&context) {
            Ok(attachment) => {
                let qid = export.describe(&attachment.root, None).qid();
                self.fids.insert(
                    fid,
                    FidState {
                        node: attachment.root.clone(),
                        root: attachment.root,
                        epoch: attachment.epoch,
                        open: None,
                    },
                );
                Reply::Attach(qid)
            }
            Err(errno) => Reply::Lerror(errno),
        }
    }

    fn check(
        &self,
        export: &mut E,
        epoch: Epoch,
        node: &E::Node,
        operation: Operation,
    ) -> Result<(), Errno> {
        export.check(&Access {
            connection: self.id,
            epoch,
            node,
            operation,
        })
    }

    /// A failure at the first name is an error and creates nothing. A failure
    /// later answers the qids walked so far and also creates nothing.
    fn walk(&mut self, export: &mut E, fid: Fid, newfid: Fid, names: &[&[u8]]) -> Reply {
        let Some(source) = self.fids.get(&fid) else {
            return Reply::Lerror(Errno::EBADF);
        };
        if source.open.is_some() {
            return Reply::Lerror(Errno::EBADF);
        }
        if newfid != fid {
            if newfid == Fid::NOFID || self.fids.contains_key(&newfid) {
                return Reply::Lerror(Errno::EBADF);
            }
            if self.fids.len() >= self.limits.max_fids() {
                return Reply::Lerror(Errno::EMFILE);
            }
        }
        let (epoch, root) = (source.epoch, source.root.clone());
        let mut node = source.node.clone();
        if let Err(errno) = self.check(export, epoch, &node, Operation::Walk) {
            return Reply::Lerror(errno);
        }
        let mut qids = Vec::with_capacity(names.len());
        for (index, name) in names.iter().enumerate() {
            match self.step(export, epoch, &root, &node, name) {
                Ok(next) => {
                    qids.push(export.describe(&next, None).qid());
                    node = next;
                }
                Err(errno) if index == 0 => return Reply::Lerror(errno),
                Err(_) => return Reply::Walk(qids),
            }
        }
        match self.fids.get_mut(&newfid) {
            Some(existing) => {
                let replaced = std::mem::replace(&mut existing.node, node);
                export.release(replaced, None);
            }
            None => {
                self.fids.insert(
                    newfid,
                    FidState {
                        node,
                        root,
                        epoch,
                        open: None,
                    },
                );
            }
        }
        Reply::Walk(qids)
    }

    fn step(
        &self,
        export: &mut E,
        epoch: Epoch,
        root: &E::Node,
        node: &E::Node,
        name: &[u8],
    ) -> Result<E::Node, Errno> {
        if export.describe(node, None).kind != NodeKind::Directory {
            return Err(Errno::ENOTDIR);
        }
        let next = match name {
            b".." if node == root => root.clone(),
            b".." => export.lookup(node, WalkName::Parent)?,
            b"" | b"." => return Err(Errno::ENOENT),
            _ if name.contains(&b'/') || name.contains(&0) => return Err(Errno::ENOENT),
            _ => export.lookup(node, WalkName::Child(name))?,
        };
        self.check(export, epoch, &next, Operation::Walk)?;
        Ok(next)
    }

    fn open(&mut self, export: &mut E, fid: Fid, flags: OpenFlags) -> Reply {
        let Some(state) = self.fids.get(&fid) else {
            return Reply::Lerror(Errno::EBADF);
        };
        if state.open.is_some() {
            return Reply::Lerror(Errno::EBADF);
        }
        let Some(access) = flags.access() else {
            return Reply::Lerror(Errno::EINVAL);
        };
        if flags.unknown() != 0 {
            return Reply::Lerror(Errno::EINVAL);
        }
        let entry = export.describe(&state.node, None);
        if entry.kind == NodeKind::Directory && access.writes() {
            return Reply::Lerror(Errno::EISDIR);
        }
        if entry.kind == NodeKind::File && flags.directory() {
            return Reply::Lerror(Errno::ENOTDIR);
        }
        let result = self
            .check(export, state.epoch, &state.node, Operation::Open(flags))
            .and_then(|()| export.open(&state.node, flags));
        match result {
            Ok(handle) => {
                // The opened version, which the owner may have pinned.
                let entry = export.describe(&state.node, Some(&handle));
                let iounit = self.frame_limit() - IO_HEADER;
                if let Some(state) = self.fids.get_mut(&fid) {
                    state.open = Some(Opened {
                        handle,
                        access,
                        kind: entry.kind,
                    });
                }
                Reply::Lopen {
                    qid: entry.qid(),
                    iounit,
                }
            }
            Err(errno) => Reply::Lerror(errno),
        }
    }

    fn read(
        &mut self,
        export: &mut E,
        tag: Tag,
        fid: Fid,
        offset: u64,
        count: u32,
    ) -> Option<Reply> {
        let count = self.read_count(count);
        let reply = self.read_now(export, fid, offset, count);
        if reply.is_some() {
            return reply;
        }
        if self.waiting.len() >= self.limits.max_pending() {
            // The export consumed nothing for a waiting read.
            return Some(Reply::Lerror(Errno::EAGAIN));
        }
        self.waiting.push_back(Waiting {
            tag,
            fid,
            offset,
            count,
        });
        None
    }

    /// `None` when the export makes the read wait.
    fn read_now(&mut self, export: &mut E, fid: Fid, offset: u64, count: u32) -> Option<Reply> {
        let id = self.id;
        let Some(state) = self.fids.get_mut(&fid) else {
            return Some(Reply::Lerror(Errno::EBADF));
        };
        let Some(opened) = state.open.as_mut() else {
            return Some(Reply::Lerror(Errno::EBADF));
        };
        if !opened.access.reads() {
            return Some(Reply::Lerror(Errno::EBADF));
        }
        if opened.kind == NodeKind::Directory {
            return Some(Reply::Lerror(Errno::EISDIR));
        }
        let access = Access {
            connection: id,
            epoch: state.epoch,
            node: &state.node,
            operation: Operation::Read,
        };
        // A zero-count read is answered at once: it neither waits nor
        // consumes, whatever the node.
        let result = export.check(&access).and_then(|()| match count {
            0 => Ok(ReadOutcome::Ready(Vec::new())),
            _ => export.read(&state.node, &mut opened.handle, offset, count),
        });
        match result {
            Ok(ReadOutcome::Ready(data)) if data.len() > count as usize => {
                Some(Reply::Lerror(Errno::EIO))
            }
            Ok(ReadOutcome::Ready(data)) => Some(Reply::Read(data)),
            Ok(ReadOutcome::Pending) => None,
            Err(errno) => Some(Reply::Lerror(errno)),
        }
    }

    fn write(&mut self, export: &mut E, fid: Fid, offset: u64, data: &[u8]) -> Reply {
        let id = self.id;
        let Some(state) = self.fids.get_mut(&fid) else {
            return Reply::Lerror(Errno::EBADF);
        };
        let Some(opened) = state.open.as_mut() else {
            return Reply::Lerror(Errno::EBADF);
        };
        if !opened.access.writes() {
            return Reply::Lerror(Errno::EBADF);
        }
        let access = Access {
            connection: id,
            epoch: state.epoch,
            node: &state.node,
            operation: Operation::Write,
        };
        let result = export
            .check(&access)
            .and_then(|()| export.write(&state.node, &mut opened.handle, offset, data));
        match result {
            Ok(count) if count as usize > data.len() => Reply::Lerror(Errno::EIO),
            Ok(count) => Reply::Write(count),
            Err(errno) => Reply::Lerror(errno),
        }
    }

    /// Reads waiting on the fid are answered `EBADF` first; the fid and its
    /// handle then go back to the export unconditionally.
    fn clunk(&mut self, export: &mut E, fid: Fid, reply: Reply) -> Result<Reply, Fatal> {
        let Some(state) = self.fids.remove(&fid) else {
            return Ok(Reply::Lerror(Errno::EBADF));
        };
        let (ended, kept): (VecDeque<_>, VecDeque<_>) = std::mem::take(&mut self.waiting)
            .into_iter()
            .partition(|waiting| waiting.fid == fid);
        self.waiting = kept;
        for waiting in ended {
            self.send(waiting.tag, &Reply::Lerror(Errno::EBADF))?;
        }
        export.release(state.node, state.open.map(|opened| opened.handle));
        Ok(reply)
    }

    fn getattr(&mut self, export: &mut E, fid: Fid) -> Reply {
        let Some(state) = self.fids.get(&fid) else {
            return Reply::Lerror(Errno::EBADF);
        };
        if let Err(errno) = self.check(export, state.epoch, &state.node, Operation::Getattr) {
            return Reply::Lerror(errno);
        }
        let handle = state.open.as_ref().map(|opened| &opened.handle);
        let entry = export.describe(&state.node, handle);
        let file_type = match entry.kind {
            NodeKind::Directory => S_IFDIR,
            NodeKind::File => S_IFREG,
        };
        Reply::Getattr(Attr {
            valid: Attr::MODE | Attr::NLINK | Attr::INO | Attr::SIZE,
            qid: entry.qid(),
            mode: file_type | (entry.permissions & 0o7777),
            nlink: 1,
            size: entry.size,
        })
    }
}
