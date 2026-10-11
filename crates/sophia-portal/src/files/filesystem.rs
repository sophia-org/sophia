use super::*;
use sophia_9p::export::*;
use sophia_9p::journal::{Staging, StagingBounds};
use sophia_9p::records::{OpenAccess, OpenFlags};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortalFileNode {
    Root,
    Api,
    Limits,
    Events,
    Transaction,
    Submit,
    Ack,
    Upload,
    Slot,
}
use PortalFileNode as Node;
const ENTRIES: [(&[u8], Node); 7] = [
    (b"api", Node::Api),
    (b"limits", Node::Limits),
    (b"events", Node::Events),
    (b"transaction", Node::Transaction),
    (b"submit", Node::Submit),
    (b"ack", Node::Ack),
    (b"upload", Node::Upload),
];

pub struct PortalFileHandle {
    qid: u64,
    kind: Handle,
}
enum Handle {
    Plain,
    Transaction,
    Upload,
    Snapshot(Vec<u8>),
}

fn slice(bytes: &[u8], offset: u64, count: u32) -> Vec<u8> {
    let start = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    bytes[start..start.saturating_add(count as usize).min(bytes.len())].to_vec()
}

impl<A: PortalFileAuthority> Export for PortalFileExport<A> {
    type Node = Node;
    type Handle = PortalFileHandle;

    fn attach(&mut self, context: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        let c = self.live()?;
        if c.attached
            || c.admission.connection != context.connection
            || match c.admission.custody {
                PortalRequesterCustody::ProtectedPeer(peer) => context.peer != Some(peer),
                PortalRequesterCustody::SessionAuthority { .. } => false,
            }
        {
            return Err(Errno::EACCES);
        }
        c.attached = true;
        Ok(Attachment {
            root: Node::Root,
            epoch: Epoch(c.admission.epoch),
        })
    }
    fn check(&mut self, access: &Access<'_, Node>) -> Result<(), Errno> {
        let c = self.live()?;
        if !c.attached
            || c.admission.connection != access.connection
            || Epoch(c.admission.epoch) != access.epoch
        {
            return Err(Errno::ESTALE);
        }
        Ok(())
    }
    fn lookup(&mut self, directory: &Node, name: WalkName<'_>) -> Result<Node, Errno> {
        match (directory, name) {
            (Node::Root, WalkName::Parent) | (Node::Upload, WalkName::Parent) => Ok(Node::Root),
            (Node::Root, WalkName::Child(name)) => ENTRIES
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, node)| *node)
                .ok_or(Errno::ENOENT),
            (Node::Upload, WalkName::Child(b"0")) => Ok(Node::Slot),
            (Node::Upload, _) => Err(Errno::ENOENT),
            _ => Err(Errno::ENOTDIR),
        }
    }
    fn describe(&self, node: &Node, handle: Option<&PortalFileHandle>) -> Entry {
        let base = self.connection.as_ref().map_or(0, |c| c.base);
        let qid_path = handle.map_or(base + *node as u64, |h| h.qid);
        let size = match (node, handle.map(|h| &h.kind)) {
            (_, Some(Handle::Snapshot(bytes))) => bytes.len() as u64,
            (Node::Api, _) => {
                portal_file_api_line(self.connection.as_ref().map_or(0, |c| c.admission.epoch))
                    .len() as u64
            }
            (Node::Limits, _) => PORTAL_FILE_MAX_OBJECT_BYTES as u64,
            (Node::Events, _) => self.connection.as_ref().map_or(0, |c| c.journal.size()),
            (Node::Transaction, _) => self
                .connection
                .as_ref()
                .and_then(|c| c.staging.as_ref())
                .filter(|s| s.handle == qid_path)
                .map_or(0, |s| s.bytes.len() as u64),
            (Node::Slot, _) => self
                .upload
                .as_ref()
                .filter(|u| u.qid == qid_path)
                .map_or(0, |u| u.bytes.len() as u64),
            _ => 0,
        };
        Entry {
            kind: if matches!(node, Node::Root | Node::Upload) {
                NodeKind::Directory
            } else {
                NodeKind::File
            },
            qid_path,
            qid_version: 0,
            size,
            permissions: match node {
                Node::Root | Node::Upload => 0o500,
                Node::Transaction => 0o600,
                Node::Submit | Node::Ack | Node::Slot => 0o200,
                _ => 0o400,
            },
        }
    }
    fn open(&mut self, node: &Node, flags: OpenFlags) -> Result<PortalFileHandle, Errno> {
        self.live()?;
        let access = flags.access().ok_or(Errno::EINVAL)?;
        let required = match node {
            Node::Transaction => OpenAccess::ReadWrite,
            Node::Submit | Node::Ack | Node::Slot => OpenAccess::Write,
            _ => OpenAccess::Read,
        };
        if access != required || flags.truncate() || flags.append() {
            return Err(Errno::EACCES);
        }
        let c = self.connection.as_ref().unwrap();
        let mut qid = c.base + *node as u64;
        let kind = match node {
            Node::Api => Handle::Snapshot(portal_file_api_line(c.admission.epoch).into_bytes()),
            Node::Limits => Handle::Snapshot(
                encode_portal_file_limits(c.admission.epoch, &self.limits)
                    .map_err(|_| Errno::EINVAL)?,
            ),
            Node::Transaction => {
                if c.refused {
                    return Err(Errno::EACCES);
                }
                if c.staging.is_some()
                    || c.accepted
                        .as_ref()
                        .is_some_and(|a| c.acknowledged < a.receipt)
                {
                    return Err(EBUSY);
                }
                qid = self.allocate_qids(1)?;
                let c = self.connection.as_mut().unwrap();
                c.staging = Some(Staging::new(
                    qid,
                    StagingBounds {
                        header_bytes: PORTAL_FILE_MIN_CANDIDATE_BYTES,
                        max_bytes: PORTAL_FILE_MAX_CANDIDATE_BYTES,
                        assembly: Duration::from_millis(self.limits.assembly_timeout_ms.into()),
                    },
                ));
                c.submitted = false;
                Handle::Transaction
            }
            Node::Slot => {
                let upload = self.upload.as_mut().ok_or(Errno::ESTALE)?;
                if upload.writer {
                    return Err(EBUSY);
                }
                upload.writer = true;
                qid = upload.qid;
                Handle::Upload
            }
            _ => Handle::Plain,
        };
        Ok(PortalFileHandle { qid, kind })
    }
    fn read(
        &mut self,
        node: &Node,
        handle: &mut PortalFileHandle,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        let c = self.live()?;
        let bytes = match (node, &handle.kind) {
            (Node::Api | Node::Limits, Handle::Snapshot(bytes)) => slice(bytes, offset, count),
            (Node::Events, _) => return c.journal.read(offset, count),
            (Node::Transaction, Handle::Transaction) => slice(
                &c.staging
                    .as_ref()
                    .filter(|s| s.handle == handle.qid)
                    .ok_or(Errno::ESTALE)?
                    .bytes,
                offset,
                count,
            ),
            _ => return Err(Errno::EACCES),
        };
        Ok(ReadOutcome::Ready(bytes))
    }
    fn write(
        &mut self,
        node: &Node,
        handle: &mut PortalFileHandle,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, Errno> {
        self.live()?;
        match (node, &handle.kind) {
            (Node::Transaction, Handle::Transaction) => {
                let c = self.connection.as_mut().unwrap();
                if c.submitted {
                    return Err(Errno::EACCES);
                }
                c.staging
                    .as_mut()
                    .filter(|s| s.handle == handle.qid)
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
            (Node::Slot, Handle::Upload) => {
                let upload = self
                    .upload
                    .as_mut()
                    .filter(|u| u.qid == handle.qid)
                    .ok_or(Errno::ESTALE)?;
                let start = usize::try_from(offset).map_err(|_| Errno::EINVAL)?;
                let end = start
                    .checked_add(data.len())
                    .filter(|end| *end <= upload.total)
                    .ok_or(Errno::EINVAL)?;
                if start > upload.bytes.len() {
                    return Err(Errno::EINVAL);
                }
                if start < upload.bytes.len() {
                    if end > upload.bytes.len() || upload.bytes[start..end] != *data {
                        return Err(Errno::EINVAL);
                    }
                } else {
                    upload.bytes.extend_from_slice(data);
                }
                Ok(data.len() as u32)
            }
            _ => Err(Errno::EINVAL),
        }
    }
    fn readdir(
        &mut self,
        directory: &Node,
        _handle: &mut PortalFileHandle,
        cookie: u64,
        max: usize,
    ) -> Result<Vec<DirEntry>, Errno> {
        let entries: &[(&[u8], Node)] = match directory {
            Node::Root => &ENTRIES,
            Node::Upload => &[(b"0", Node::Slot)],
            _ => return Err(Errno::ENOTDIR),
        };
        let start = usize::try_from(cookie)
            .unwrap_or(usize::MAX)
            .min(entries.len());
        Ok(entries[start..]
            .iter()
            .take(max)
            .enumerate()
            .map(|(i, (name, node))| DirEntry {
                name: name.to_vec(),
                entry: self.describe(node, None),
                next: (start + i + 1) as u64,
            })
            .collect())
    }
    fn release(&mut self, _node: Node, handle: Option<PortalFileHandle>) {
        let Some(handle) = handle else {
            return;
        };
        match handle.kind {
            Handle::Transaction => {
                if let Some(c) = &mut self.connection
                    && c.staging.as_ref().is_some_and(|s| s.handle == handle.qid)
                {
                    c.staging = None;
                    c.submitted = false;
                }
            }
            Handle::Upload => {
                if let Some(id) = self
                    .upload
                    .as_ref()
                    .filter(|u| u.qid == handle.qid)
                    .map(|u| u.transfer)
                {
                    self.finish_reserved(id, PortalGrantState::Revoked);
                }
            }
            _ => {}
        }
    }
}
