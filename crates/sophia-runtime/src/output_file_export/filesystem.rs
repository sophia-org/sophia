use super::*;
use sophia_9p::export::*;
use sophia_9p::records::{OpenAccess, OpenFlags};

const API: &[u8] = b"sophia-output-files version=1\n";
const EBUSY: Errno = Errno(16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFileNode {
    Root,
    Api,
    Limits,
    Topology,
    Events,
    Transaction,
    Submit,
    Ack,
}

use OutputFileNode as Node;

const ENTRIES: [(&[u8], Node); 7] = [
    (b"api", Node::Api),
    (b"limits", Node::Limits),
    (b"topology", Node::Topology),
    (b"events", Node::Events),
    (b"transaction", Node::Transaction),
    (b"submit", Node::Submit),
    (b"ack", Node::Ack),
];

pub struct OutputFileHandle(Handle);

enum Handle {
    Plain,
    Topology {
        object: Arc<Topology>,
        covered: Vec<bool>,
        unread: usize,
    },
    Transaction(u64),
}

fn slice(bytes: &[u8], offset: u64, count: u32) -> Vec<u8> {
    let start = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    bytes[start..start.saturating_add(count as usize).min(bytes.len())].to_vec()
}

impl Export for OutputFileExport {
    type Node = Node;
    type Handle = OutputFileHandle;

    fn attach(&mut self, context: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        self.live()?;
        if self.attached || self.connection != Some(context.connection) {
            return Err(Errno::EACCES);
        }
        self.attached = true;
        Ok(Attachment {
            root: Node::Root,
            epoch: Epoch(self.epoch),
        })
    }

    fn check(&mut self, access: &Access<'_, Node>) -> Result<(), Errno> {
        self.live()?;
        if !self.attached
            || access.epoch != Epoch(self.epoch)
            || self.connection != Some(access.connection)
        {
            return Err(Errno::ESTALE);
        }
        Ok(())
    }

    fn lookup(&mut self, directory: &Node, name: WalkName<'_>) -> Result<Node, Errno> {
        if *directory != Node::Root {
            return Err(Errno::ENOTDIR);
        }
        match name {
            WalkName::Parent => Ok(Node::Root),
            WalkName::Child(name) => ENTRIES
                .iter()
                .find(|(entry, _)| *entry == name)
                .map(|(_, node)| *node)
                .ok_or(Errno::ENOENT),
        }
    }

    fn describe(&self, node: &Node, handle: Option<&OutputFileHandle>) -> Entry {
        let mut qid_path = self.base + *node as u64;
        let size = match (node, handle.map(|handle| &handle.0)) {
            (Node::Api, _) => API.len() as u64,
            (Node::Limits, _) => self.limits_bytes.len() as u64,
            (Node::Events, _) => self.admission.journal().position().tail,
            (Node::Topology, Some(Handle::Topology { object, .. })) => {
                qid_path = object.qid;
                object.bytes.len() as u64
            }
            (Node::Topology, _) => {
                qid_path = self.topology.qid;
                self.topology.bytes.len() as u64
            }
            (Node::Transaction, Some(Handle::Transaction(id))) => {
                qid_path = *id;
                self.staging
                    .as_ref()
                    .filter(|s| s.handle == *id)
                    .map_or(0, |s| s.bytes.len() as u64)
            }
            _ => 0,
        };
        Entry {
            kind: if *node == Node::Root {
                NodeKind::Directory
            } else {
                NodeKind::File
            },
            qid_path,
            qid_version: 0,
            size,
            permissions: match node {
                Node::Root => 0o500,
                Node::Transaction => 0o600,
                Node::Submit | Node::Ack => 0o200,
                _ => 0o400,
            },
        }
    }

    fn open(&mut self, node: &Node, flags: OpenFlags) -> Result<OutputFileHandle, Errno> {
        self.live()?;
        let access = flags.access().ok_or(Errno::EINVAL)?;
        let allowed = match node {
            Node::Transaction => access == OpenAccess::ReadWrite,
            Node::Submit | Node::Ack => access == OpenAccess::Write,
            _ => access == OpenAccess::Read,
        };
        if !allowed || flags.truncate() || flags.append() {
            return Err(Errno::EACCES);
        }
        let handle = match node {
            Node::Topology => {
                self.admission
                    .connection()
                    .require_observe()
                    .map_err(|_| Errno::EAGAIN)?;
                if self.topology_open.is_some() {
                    return Err(EBUSY);
                }
                self.topology_open = Some(self.topology.qid);
                Handle::Topology {
                    object: self.topology.clone(),
                    covered: vec![false; self.topology.bytes.len()],
                    unread: self.topology.bytes.len(),
                }
            }
            Node::Transaction => {
                if self.refused {
                    return Err(Errno::EACCES);
                }
                if self.staging.is_some() || self.receipt.is_some() {
                    return Err(EBUSY);
                }
                let id = self.qids.allocate(1)?;
                self.staging = Some(Staging::new(
                    id,
                    StagingBounds {
                        header_bytes: 48,
                        max_bytes: self.limits.staging_bytes as usize,
                        assembly: Duration::from_millis(self.limits.assembly_timeout_millis.into()),
                    },
                ));
                self.staged_submitted = false;
                Handle::Transaction(id)
            }
            _ => Handle::Plain,
        };
        Ok(OutputFileHandle(handle))
    }

    fn read(
        &mut self,
        node: &Node,
        handle: &mut OutputFileHandle,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        self.live()?;
        let bytes = match (node, &mut handle.0) {
            (Node::Api, _) => slice(API, offset, count),
            (Node::Limits, _) => slice(&self.limits_bytes, offset, count),
            (Node::Events, _) => return self.reads.read(self.admission.journal(), offset, count),
            (
                Node::Topology,
                Handle::Topology {
                    object,
                    covered,
                    unread,
                },
            ) => {
                let bytes = slice(&object.bytes, offset, count);
                let start = usize::try_from(offset)
                    .unwrap_or(usize::MAX)
                    .min(covered.len());
                for read in &mut covered[start..start + bytes.len()] {
                    if !*read {
                        *read = true;
                        *unread -= 1;
                    }
                }
                if *unread == 0 {
                    if object.qid == self.topology.qid {
                        self.topology_read = true
                    }
                    if let Some(publication) = &mut self.publication
                        && publication.qid == object.qid
                    {
                        publication.read = true
                    }
                }
                bytes
            }
            (Node::Transaction, Handle::Transaction(id)) => {
                let staging = self
                    .staging
                    .as_ref()
                    .filter(|s| s.handle == *id)
                    .ok_or(Errno::ESTALE)?;
                slice(&staging.bytes, offset, count)
            }
            _ => return Err(Errno::EACCES),
        };
        Ok(ReadOutcome::Ready(bytes))
    }

    fn write(
        &mut self,
        node: &Node,
        handle: &mut OutputFileHandle,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, Errno> {
        self.live()?;
        match (node, &handle.0) {
            (Node::Transaction, Handle::Transaction(id)) => {
                if self.staged_submitted {
                    return Err(Errno::EACCES);
                }
                self.staging
                    .as_mut()
                    .filter(|s| s.handle == *id)
                    .ok_or(Errno::ESTALE)?
                    .write(offset, data, Instant::now())
            }
            (Node::Submit, _) if offset == 0 => {
                self.submit(data)?;
                Ok(data.len() as u32)
            }
            (Node::Ack, _) if offset == 0 => {
                self.acknowledge(data)?;
                Ok(data.len() as u32)
            }
            _ => Err(Errno::EINVAL),
        }
    }

    fn readdir(
        &mut self,
        directory: &Node,
        _handle: &mut OutputFileHandle,
        cookie: u64,
        max_entries: usize,
    ) -> Result<Vec<DirEntry>, Errno> {
        if *directory != Node::Root {
            return Err(Errno::ENOTDIR);
        }
        let start = usize::try_from(cookie)
            .unwrap_or(usize::MAX)
            .min(ENTRIES.len());
        Ok(ENTRIES[start..]
            .iter()
            .take(max_entries)
            .enumerate()
            .map(|(i, (name, node))| DirEntry {
                name: name.to_vec(),
                entry: self.describe(node, None),
                next: (start + i + 1) as u64,
            })
            .collect())
    }

    fn release(&mut self, _node: Node, handle: Option<OutputFileHandle>) {
        match handle.map(|h| h.0) {
            Some(Handle::Topology { object, .. }) if self.topology_open == Some(object.qid) => {
                self.topology_open = None;
            }
            Some(Handle::Transaction(id))
                if self.staging.as_ref().is_some_and(|s| s.handle == id) =>
            {
                self.staging = None;
                self.staged_submitted = false;
            }
            _ => {}
        }
    }
}
