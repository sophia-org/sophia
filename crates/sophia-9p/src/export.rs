//! The seam between the protocol core and the owner whose nodes it serves.
//!
//! The owner keeps every semantic decision: which nodes exist, who may reach
//! them, what reading and writing them means, and when an attachment's
//! authority ends. The core keeps only the protocol's own rules and calls
//! [`Export::check`] before each operation, so a revoked epoch stops at once,
//! through fids that are already open as well as new ones.

use crate::connection::ConnectionId;
use crate::records::{Errno, OpenFlags, Qid, QidKind};

/// One grant of authority, fixed when a client attaches. The owner ends it by
/// answering [`Export::check`] with an error for that epoch.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Epoch(pub u64);

/// What a node is, for the protocol's directory rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeKind {
    Directory,
    File,
}

/// What the protocol needs to know about a node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Entry {
    pub kind: NodeKind,
    /// Never reused for another node, across epochs as well.
    pub qid_path: u64,
    pub qid_version: u32,
    /// Permission bits only; the core adds the file type.
    pub permissions: u32,
    pub size: u64,
}

impl Entry {
    pub const fn qid(&self) -> Qid {
        Qid {
            kind: match self.kind {
                NodeKind::Directory => QidKind::Directory,
                NodeKind::File => QidKind::File,
            },
            version: self.qid_version,
            path: self.qid_path,
        }
    }
}

/// The credentials the kernel recorded for a socket's peer when it connected.
/// They describe the connecting process only, and confer nothing by
/// themselves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerCredentials {
    pub pid: i32,
    pub uid: u32,
    pub gid: u32,
}

/// Everything a client said when attaching. None of it is authority: the
/// owner decides what, if anything, this connection may attach to.
#[derive(Clone, Copy, Debug)]
pub struct AttachContext<'request> {
    pub connection: ConnectionId,
    pub peer: Option<PeerCredentials>,
    pub uname: &'request [u8],
    pub aname: &'request [u8],
    pub n_uname: u32,
}

/// A successful attach: the root the client's fid names, and the epoch of
/// authority every fid walked from it carries.
#[derive(Clone, Debug)]
pub struct Attachment<Node> {
    pub root: Node,
    pub epoch: Epoch,
}

/// The operation a check is asked about.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    /// Walking from this node, or arriving at it during a walk.
    Walk,
    Open(OpenFlags),
    Read,
    Write,
    Getattr,
}

/// One operation put to the owner before the core performs it.
#[derive(Clone, Copy, Debug)]
pub struct Access<'node, Node> {
    pub connection: ConnectionId,
    pub epoch: Epoch,
    pub node: &'node Node,
    pub operation: Operation,
}

/// A walk step. `..` from the attach root never reaches the owner: the core
/// keeps the client at its root, as the protocol requires.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WalkName<'request> {
    Child(&'request [u8]),
    Parent,
}

/// One child a directory lists: its name, the metadata a walk to it would
/// report, and the cookie that resumes the listing after it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirEntry {
    pub name: Vec<u8>,
    pub entry: Entry,
    pub next: u64,
}

/// A read either has its data now, or waits. A waiting read has consumed
/// nothing: it is asked again, and only the answer that is sent consumes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReadOutcome {
    Ready(Vec<u8>),
    Pending,
}

/// An owner's nodes, served through the protocol core.
///
/// Nodes and handles are the owner's opaque values. The core holds a node for
/// every fid and a handle for every open fid, and hands each back through
/// [`Export::release`] exactly once, whatever ended it: clunk, remove, a
/// replaced fid, a new version negotiation or disconnect. Release cannot be
/// refused.
pub trait Export {
    type Node: Clone + Eq;
    type Handle;

    fn attach(&mut self, context: &AttachContext<'_>) -> Result<Attachment<Self::Node>, Errno>;

    /// Whether this operation may proceed under its epoch. Asked before every
    /// operation on a fid except release, for every node a walk reaches, and
    /// again whenever a waiting read is retried.
    fn check(&mut self, access: &Access<'_, Self::Node>) -> Result<(), Errno>;

    fn lookup(&mut self, directory: &Self::Node, name: WalkName<'_>) -> Result<Self::Node, Errno>;

    /// A node's metadata, or, given the handle of an open fid, that opened
    /// version's: an owner that pins a snapshot at open reports the pinned
    /// qid and size through the handle. Reply to open and getattr use the
    /// handle when there is one; walks and attach use the node.
    fn describe(&self, node: &Self::Node, handle: Option<&Self::Handle>) -> Entry;

    fn open(&mut self, node: &Self::Node, flags: OpenFlags) -> Result<Self::Handle, Errno>;

    /// At most `count` bytes. Returning more is an owner fault, answered with
    /// `EIO` and never truncated.
    fn read(
        &mut self,
        node: &Self::Node,
        handle: &mut Self::Handle,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno>;

    /// The number of bytes accepted, at most `data.len()`.
    fn write(
        &mut self,
        node: &Self::Node,
        handle: &mut Self::Handle,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, Errno>;

    /// The entries of an open directory from `cookie` on, at most
    /// `max_entries` and at least one unless the listing has ended: an empty
    /// list is its end. Cookie zero is the start, and each entry's `next`
    /// resumes after it, so `next` rises strictly from `cookie`. The owner
    /// chooses cookies that stay valid while its children change.
    ///
    /// Names are exactly the ones [`Export::lookup`] resolves and
    /// [`Export::check`] admits, never `.` or `..`, and each entry is the
    /// child's [`Export::describe`] without a handle: listing opens, pins and
    /// allocates nothing. The core has already checked the directory with
    /// [`Operation::Read`]. It answers a listing that breaks these rules with
    /// `EIO`, and keeps entries that do not fit its reply for the next
    /// request. An owner that lists nothing keeps this refusal.
    fn readdir(
        &mut self,
        directory: &Self::Node,
        handle: &mut Self::Handle,
        cookie: u64,
        max_entries: usize,
    ) -> Result<Vec<DirEntry>, Errno> {
        let _ = (directory, handle, cookie, max_entries);
        Err(Errno::EOPNOTSUPP)
    }

    fn release(&mut self, node: Self::Node, handle: Option<Self::Handle>);
}
