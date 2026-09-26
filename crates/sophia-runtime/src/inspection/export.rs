use super::publication::{Shared, Snapshot};
use super::*;
use crate::host_domain::{HostDomain, Peer};
use sophia_9p::connection::ConnectionId;
use sophia_9p::export::{
    Access, AttachContext, Attachment, DirEntry, Entry, Epoch, Export, NodeKind, Operation,
    ReadOutcome, WalkName,
};
use sophia_9p::records::{Errno, OpenAccess, OpenFlags};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Node {
    Root,
    Api,
    Status,
    Snapshot,
    Events,
}

pub(super) enum Handle {
    Directory,
    Bytes { qid: u64, bytes: Vec<u8> },
    Snapshot(Arc<Snapshot>),
    Events { loss: u64, start: u64, stale: bool },
}

pub(super) struct InspectionExport {
    shared: Arc<Shared>,
    domain: Arc<HostDomain>,
    peer: Peer,
    generation: u64,
    base_qid: u64,
    attached: Option<ConnectionId>,
    pinned: bool,
    /// Opening a fresh snapshot is the explicit resynchronization boundary.
    /// Reading every byte of that snapshot remains the client's obligation.
    coherent: Option<(u64, u64)>,
}

impl InspectionExport {
    pub fn new(
        shared: Arc<Shared>,
        domain: Arc<HostDomain>,
        peer: Peer,
    ) -> Result<Self, InspectionError> {
        let generation = shared.generation.load(Ordering::SeqCst);
        if !shared.valid(generation) {
            return Err(InspectionError::Fenced);
        }
        let base_qid = shared.qids(5)?;
        Ok(Self {
            shared,
            domain,
            peer,
            generation,
            base_qid,
            attached: None,
            pinned: false,
            coherent: None,
        })
    }
    pub fn authorized(&self) -> bool {
        self.shared.valid(self.generation)
            && self
                .domain
                .check(&self.peer, &[self.shared.excluded.load(Ordering::SeqCst)])
                .is_ok()
    }
    fn current(&self) -> Result<Arc<Snapshot>, Errno> {
        let store = self.shared.view().map_err(|_| Errno::EIO)?;
        let loss = self.shared.loss.load(Ordering::SeqCst);
        store
            .snapshot
            .as_ref()
            .filter(|s| s.record.generation == self.generation && s.record.loss_generation == loss)
            .cloned()
            .ok_or(Errno::EAGAIN)
    }
    fn qid(&self, node: Node) -> u64 {
        self.base_qid
            + match node {
                Node::Root => 0,
                Node::Api => 1,
                Node::Status => 2,
                Node::Snapshot => 3,
                Node::Events => 4,
            }
    }
    fn status(&self) -> Result<Vec<u8>, Errno> {
        let store = self.shared.view().map_err(|_| Errno::EIO)?;
        let loss_generation = self.shared.loss.load(Ordering::SeqCst);
        let snapshot = store.snapshot.as_ref().filter(|s| {
            s.record.generation == self.generation && s.record.loss_generation == loss_generation
        });
        encode_inspection_status(&InspectionStatus {
            schema: INSPECTION_SCHEMA,
            generation: self.generation,
            sequence: store.sequence,
            session_generation: snapshot.map_or(0, |s| s.record.snapshot.session_generation),
            state: snapshot.map_or(InspectionState::Unavailable, |s| s.record.snapshot.state),
            wire: snapshot.map(|s| s.record.snapshot.wire),
            selected_capabilities: snapshot.map_or(0, |s| s.record.snapshot.selected_capabilities),
            wm_epoch: self.shared.wm_epoch.load(Ordering::SeqCst),
            event_floor: store.floor(),
            event_tail: store.tail,
            loss_generation,
            snapshot_available: snapshot.is_some(),
        })
        .map_err(|_| Errno::EIO)
    }
}

impl Export for InspectionExport {
    type Node = Node;
    type Handle = Handle;

    fn attach(&mut self, context: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        if self.attached.is_some()
            || !self.authorized()
            || !context
                .peer
                .is_some_and(|p| p.pid > 0 && p.pid as u32 == self.peer.pid)
        {
            return Err(Errno::EACCES);
        }
        // Neither aname nor uname can request a role, another root or epoch.
        self.attached = Some(context.connection);
        Ok(Attachment {
            root: Node::Root,
            epoch: Epoch(self.generation),
        })
    }
    fn check(&mut self, access: &Access<'_, Node>) -> Result<(), Errno> {
        if self.attached != Some(access.connection)
            || access.epoch != Epoch(self.generation)
            || !self.authorized()
        {
            return Err(Errno::ESTALE);
        }
        if matches!(access.operation, Operation::Write)
            || matches!(access.operation, Operation::Open(f) if f.access() != Some(OpenAccess::Read) || f.truncate() || f.append())
        {
            return Err(Errno::EACCES);
        }
        Ok(())
    }
    fn lookup(&mut self, directory: &Node, name: WalkName<'_>) -> Result<Node, Errno> {
        if *directory != Node::Root {
            return Err(Errno::ENOTDIR);
        }
        match name {
            WalkName::Parent => Ok(Node::Root),
            WalkName::Child(b"api") => Ok(Node::Api),
            WalkName::Child(b"status") => Ok(Node::Status),
            WalkName::Child(b"snapshot") => Ok(Node::Snapshot),
            WalkName::Child(b"events") => Ok(Node::Events),
            WalkName::Child(_) => Err(Errno::ENOENT),
        }
    }
    fn describe(&self, node: &Node, handle: Option<&Handle>) -> Entry {
        let (qid_path, size) = match handle {
            Some(Handle::Bytes { qid, bytes }) => (*qid, bytes.len() as u64),
            Some(Handle::Snapshot(s)) => (s.qid, s.bytes.len() as u64),
            _ => (self.qid(*node), 0),
        };
        Entry {
            kind: if *node == Node::Root {
                NodeKind::Directory
            } else {
                NodeKind::File
            },
            qid_path,
            qid_version: 0,
            permissions: if *node == Node::Root { 0o500 } else { 0o400 },
            size,
        }
    }
    fn open(&mut self, node: &Node, flags: OpenFlags) -> Result<Handle, Errno> {
        if flags.access() != Some(OpenAccess::Read) || flags.truncate() || flags.append() {
            return Err(Errno::EACCES);
        }
        match node {
            Node::Root => Ok(Handle::Directory),
            Node::Api | Node::Status => {
                let bytes = if *node == Node::Api {
                    inspection_api_text().into_bytes()
                } else {
                    self.status()?
                };
                let qid = self.shared.qids(1).map_err(|_| Errno::ENOSPC)?;
                Ok(Handle::Bytes { qid, bytes })
            }
            Node::Snapshot => {
                if self.pinned {
                    return Err(Errno(16));
                }
                let snapshot = self.current()?;
                self.coherent = Some((
                    snapshot.record.loss_generation,
                    snapshot.record.event_offset,
                ));
                self.pinned = true;
                Ok(Handle::Snapshot(snapshot))
            }
            Node::Events => {
                let loss = self.shared.loss.load(Ordering::SeqCst);
                let Some((observed, start)) = self.coherent else {
                    return Err(Errno::EAGAIN);
                };
                if observed != loss {
                    return Err(Errno::ESTALE);
                }
                if start < self.shared.view().map_err(|_| Errno::EIO)?.floor() {
                    self.coherent = None;
                    return Err(Errno::ESTALE);
                }
                Ok(Handle::Events {
                    loss,
                    start,
                    stale: false,
                })
            }
        }
    }
    fn read(
        &mut self,
        node: &Node,
        handle: &mut Handle,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        if !self.shared.valid(self.generation) {
            return Err(Errno::ESTALE);
        }
        let loss = self.shared.loss.load(Ordering::SeqCst);
        let bytes: &[u8] = match handle {
            Handle::Directory => return Err(Errno::EISDIR),
            Handle::Bytes { bytes, .. } => bytes,
            Handle::Snapshot(snapshot) => {
                if snapshot.record.loss_generation != loss {
                    return Err(Errno::ESTALE);
                }
                &snapshot.bytes
            }
            Handle::Events {
                loss: observed,
                start,
                stale,
            } => {
                if *stale || *observed != loss {
                    return Err(Errno::ESTALE);
                }
                let store = self.shared.view().map_err(|_| Errno::EIO)?;
                if offset < *start || offset < store.floor() {
                    *stale = true;
                    self.coherent = None;
                    return Err(Errno::ESTALE);
                }
                if offset > store.tail {
                    return Err(Errno::EINVAL);
                }
                if offset == store.tail {
                    return Ok(ReadOutcome::Pending);
                }
                let mut bytes = Vec::new();
                let mut cursor = offset;
                for event in &store.events {
                    let end = event.offset + event.bytes.len() as u64;
                    if cursor < event.offset || cursor >= end {
                        continue;
                    }
                    let local = (cursor - event.offset) as usize;
                    let take = (count as usize - bytes.len()).min(event.bytes.len() - local);
                    bytes.extend_from_slice(&event.bytes[local..local + take]);
                    cursor += take as u64;
                    if bytes.len() == count as usize {
                        break;
                    }
                }
                // Loss concurrent with assembling the response cannot authorize
                // another successful observation read. Already queued replies
                // before the check cannot be recalled by this export.
                if self.shared.loss.load(Ordering::SeqCst) != *observed {
                    return Err(Errno::ESTALE);
                }
                return Ok(ReadOutcome::Ready(bytes));
            }
        };
        let _ = node;
        let offset = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        let end = offset.saturating_add(count as usize).min(bytes.len());
        Ok(ReadOutcome::Ready(bytes[offset..end].to_vec()))
    }
    fn write(&mut self, _: &Node, _: &mut Handle, _: u64, _: &[u8]) -> Result<u32, Errno> {
        Err(Errno::EACCES)
    }
    fn readdir(
        &mut self,
        directory: &Node,
        handle: &mut Handle,
        cookie: u64,
        max_entries: usize,
    ) -> Result<Vec<DirEntry>, Errno> {
        if *directory != Node::Root || !matches!(handle, Handle::Directory) {
            return Err(Errno::ENOTDIR);
        }
        let children: [(&[u8], Node); 4] = [
            (b"api", Node::Api),
            (b"status", Node::Status),
            (b"snapshot", Node::Snapshot),
            (b"events", Node::Events),
        ];
        let start = usize::try_from(cookie).map_err(|_| Errno::EINVAL)?;
        if start > children.len() {
            return Err(Errno::EINVAL);
        }
        Ok(children
            .iter()
            .enumerate()
            .skip(start)
            .take(max_entries)
            .map(|(i, (name, node))| DirEntry {
                name: name.to_vec(),
                entry: self.describe(node, None),
                next: (i + 1) as u64,
            })
            .collect())
    }
    fn release(&mut self, _: Node, handle: Option<Handle>) {
        if matches!(handle, Some(Handle::Snapshot(_))) {
            self.pinned = false;
        }
    }
}
