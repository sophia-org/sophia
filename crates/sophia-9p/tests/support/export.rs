//! The C1 static test export. Its tree:
//!
//! ```text
//! /            directory
//!   info       read-only file, INFO_LEN bytes of a known pattern
//!   sink       write-only file, at most SINK_LIMIT bytes in total
//!   events     read-only; a read waits until an event is pushed, then takes
//!              exactly one whole event
//!   hidden     visible to lookup, refused by check
//!   dir/       directory
//!     leaf     read-only file
//! ```
//!
//! A listing names every child but `hidden`, in the order above, with the
//! cookie of each entry its position plus one.
//!
//! Every attach is a fresh epoch, and qid paths carry the epoch, so no path is
//! reused. The shared state lets a test push events, revoke epochs, and see
//! every call the core made.

use std::collections::{BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use sophia_9p::{
    Access, AttachContext, Attachment, ConnectionId, DirEntry, Entry, Epoch, Errno, Export,
    NodeKind, OpenFlags, Operation, PeerCredentials, ReadOutcome, WalkName,
};

pub const INFO_LEN: usize = 70_000;
pub const SINK_LIMIT: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Name {
    Root,
    Info,
    Sink,
    Events,
    Hidden,
    Dir,
    Leaf,
}

impl Name {
    const fn index(self) -> u64 {
        match self {
            Self::Root => 1,
            Self::Info => 2,
            Self::Sink => 3,
            Self::Events => 4,
            Self::Hidden => 5,
            Self::Dir => 6,
            Self::Leaf => 7,
        }
    }

    const fn kind(self) -> NodeKind {
        match self {
            Self::Root | Self::Dir => NodeKind::Directory,
            _ => NodeKind::File,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    pub name: Name,
    pub epoch: Epoch,
}

pub fn info_byte(offset: usize) -> u8 {
    (offset % 251) as u8
}

/// One attach as the export saw it: connection, peer, uname and aname.
pub type AttachRecord = (ConnectionId, Option<PeerCredentials>, Vec<u8>, Vec<u8>);

#[derive(Default)]
pub struct Shared {
    pub next_epoch: u64,
    pub revoked: BTreeSet<Epoch>,
    pub events: VecDeque<Vec<u8>>,
    pub sink: Vec<u8>,
    pub attaches: Vec<AttachRecord>,
    pub checks: Vec<(ConnectionId, Epoch, Name, Operation)>,
    /// Every read and write that reached the export, ready or not.
    pub reads: usize,
    pub listings: usize,
    pub writes: usize,
    pub opens: usize,
    /// Every release, with whether it carried an open handle.
    pub released: Vec<(Name, Epoch, bool)>,
}

#[derive(Clone, Default)]
pub struct StaticExport {
    shared: Arc<Mutex<Shared>>,
}

impl StaticExport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap()
    }

    pub fn push_event(&self, event: &[u8]) {
        self.state().events.push_back(event.to_vec());
    }

    pub fn revoke(&self, epoch: Epoch) {
        self.state().revoked.insert(epoch);
    }
}

/// An open fid's handle. Opening `info` pins a version, the number of opens
/// so far, as an owner that snapshots at open would; the opened fid reports
/// it while the node itself reports version 0.
pub struct Handle {
    pub version: u32,
}

impl Export for StaticExport {
    type Node = Node;
    type Handle = Handle;

    fn attach(&mut self, context: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        let mut state = self.state();
        state.attaches.push((
            context.connection,
            context.peer,
            context.uname.to_vec(),
            context.aname.to_vec(),
        ));
        if !context.aname.is_empty() {
            return Err(Errno::ENOENT);
        }
        state.next_epoch += 1;
        let epoch = Epoch(state.next_epoch);
        Ok(Attachment {
            root: Node {
                name: Name::Root,
                epoch,
            },
            epoch,
        })
    }

    fn check(&mut self, access: &Access<'_, Node>) -> Result<(), Errno> {
        let mut state = self.state();
        state.checks.push((
            access.connection,
            access.epoch,
            access.node.name,
            access.operation,
        ));
        if state.revoked.contains(&access.epoch) {
            return Err(Errno::ESTALE);
        }
        match (access.node.name, access.operation) {
            (Name::Hidden, _) => Err(Errno::EACCES),
            (Name::Info | Name::Leaf | Name::Events, Operation::Open(flags))
                if flags.access().is_some_and(|access| access.writes()) =>
            {
                Err(Errno::EACCES)
            }
            (Name::Sink, Operation::Open(flags))
                if flags.access().is_some_and(|access| access.reads()) =>
            {
                Err(Errno::EACCES)
            }
            _ => Ok(()),
        }
    }

    fn lookup(&mut self, directory: &Node, name: WalkName<'_>) -> Result<Node, Errno> {
        let next = match (directory.name, name) {
            (Name::Root, WalkName::Child(b"info")) => Name::Info,
            (Name::Root, WalkName::Child(b"sink")) => Name::Sink,
            (Name::Root, WalkName::Child(b"events")) => Name::Events,
            (Name::Root, WalkName::Child(b"hidden")) => Name::Hidden,
            (Name::Root, WalkName::Child(b"dir")) => Name::Dir,
            (Name::Dir, WalkName::Child(b"leaf")) => Name::Leaf,
            (Name::Dir, WalkName::Parent) => Name::Root,
            _ => return Err(Errno::ENOENT),
        };
        Ok(Node {
            name: next,
            epoch: directory.epoch,
        })
    }

    fn describe(&self, node: &Node, handle: Option<&Handle>) -> Entry {
        Entry {
            kind: node.name.kind(),
            qid_path: (node.epoch.0 << 16) | node.name.index(),
            qid_version: handle.map_or(0, |handle| handle.version),
            permissions: match node.name {
                Name::Sink => 0o200,
                Name::Root | Name::Dir => 0o555,
                _ => 0o444,
            },
            size: match node.name {
                Name::Info => INFO_LEN as u64,
                _ => 0,
            },
        }
    }

    fn open(&mut self, node: &Node, _flags: OpenFlags) -> Result<Handle, Errno> {
        let mut state = self.state();
        state.opens += 1;
        let version = match node.name {
            Name::Info => u32::try_from(state.opens).unwrap(),
            _ => 0,
        };
        Ok(Handle { version })
    }

    fn read(
        &mut self,
        node: &Node,
        _handle: &mut Handle,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        let mut state = self.state();
        state.reads += 1;
        match node.name {
            Name::Info | Name::Leaf => {
                let length = if node.name == Name::Info { INFO_LEN } else { 4 };
                let start = usize::try_from(offset).unwrap_or(usize::MAX).min(length);
                let end = start.saturating_add(count as usize).min(length);
                Ok(ReadOutcome::Ready((start..end).map(info_byte).collect()))
            }
            Name::Events => match state.events.front() {
                None => Ok(ReadOutcome::Pending),
                // An event is never split: one that does not fit is refused
                // and stays queued.
                Some(event) if event.len() > count as usize => Err(Errno::EINVAL),
                Some(_) => Ok(ReadOutcome::Ready(state.events.pop_front().unwrap())),
            },
            _ => Err(Errno::EBADF),
        }
    }

    fn write(
        &mut self,
        node: &Node,
        _handle: &mut Handle,
        _offset: u64,
        data: &[u8],
    ) -> Result<u32, Errno> {
        let mut state = self.state();
        state.writes += 1;
        if node.name != Name::Sink {
            return Err(Errno::EBADF);
        }
        if state.sink.len() + data.len() > SINK_LIMIT {
            return Err(Errno::ENOSPC);
        }
        state.sink.extend_from_slice(data);
        Ok(u32::try_from(data.len()).unwrap())
    }

    fn readdir(
        &mut self,
        directory: &Node,
        _handle: &mut Handle,
        cookie: u64,
        max_entries: usize,
    ) -> Result<Vec<DirEntry>, Errno> {
        self.state().listings += 1;
        let children: &[(&[u8], Name)] = match directory.name {
            Name::Root => &[
                (b"info", Name::Info),
                (b"sink", Name::Sink),
                (b"events", Name::Events),
                (b"dir", Name::Dir),
            ],
            Name::Dir => &[(b"leaf", Name::Leaf)],
            _ => return Err(Errno::ENOTDIR),
        };
        let start = usize::try_from(cookie)
            .unwrap_or(usize::MAX)
            .min(children.len());
        Ok(children[start..]
            .iter()
            .take(max_entries)
            .zip(start + 1..)
            .map(|(&(name, child), next)| DirEntry {
                name: name.to_vec(),
                entry: self.describe(
                    &Node {
                        name: child,
                        epoch: directory.epoch,
                    },
                    None,
                ),
                next: next as u64,
            })
            .collect())
    }

    fn release(&mut self, node: Node, handle: Option<Handle>) {
        self.state()
            .released
            .push((node.name, node.epoch, handle.is_some()));
    }
}
