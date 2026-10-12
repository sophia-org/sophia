//! Authenticated recipient view. This grants neither broker decisions nor
//! protocol-authority upload/execution custody. Payloads come from the common
//! portal coordinator's non-forgeable recipient lease.
use crate::recipient_object::PortalObjectRecipient;
use sophia_9p::{ConnectionId, Errno, OpenFlags, export::*};
use sophia_protocol::NamespaceId;
use sophia_runtime::authenticated_files::{PreparedFileRoot, ProtectedFiles};
use std::sync::Arc;

mod connection;
pub use connection::PortalClientConnection;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortalClientNode {
    Root,
    Api,
    Status,
    Transfer { object: u64, qid: u64 },
    Watch { object: u64, qid: u64 },
}
use PortalClientNode as Node;
pub enum PortalClientHandle {
    Plain,
    Snapshot { qid: u64, bytes: Vec<u8> },
}
use PortalClientHandle as Handle;

pub struct PortalClientFiles {
    objects: Arc<PortalObjectRecipient>,
    epoch: u64,
    connection: Option<ConnectionId>,
    attached: bool,
    next_qid: u64,
    binding: Option<(u64, u64)>,
}
impl PortalClientFiles {
    /// The caller supplies the recipient belonging to the authenticated
    /// endpoint scope. Equal namespace numbers do not select another owner.
    pub fn new(
        namespace: NamespaceId,
        objects: Arc<PortalObjectRecipient>,
        epoch: u64,
    ) -> Result<Self, Errno> {
        if !namespace.is_valid() || objects.namespace() != namespace || epoch == 0 {
            return Err(Errno::EACCES);
        }
        Ok(Self {
            objects,
            epoch,
            connection: None,
            attached: false,
            next_qid: 4,
            binding: None,
        })
    }
    pub fn bind_connection(&mut self, connection: ConnectionId) -> Result<(), Errno> {
        if self.connection.is_some() {
            return Err(Errno::EACCES);
        }
        self.connection = Some(connection);
        Ok(())
    }
    fn qids(&mut self, count: u64) -> Result<u64, Errno> {
        let start = self.next_qid;
        self.next_qid = start.checked_add(count).ok_or(Errno::ENOSPC)?;
        Ok(start)
    }
    fn binding(&mut self) -> Result<(u64, u64), Errno> {
        let object = self.objects.current()?;
        if let Some(binding) = self.binding.filter(|(current, _)| *current == object) {
            return Ok(binding);
        }
        let binding = (object, self.qids(2)?);
        self.binding = Some(binding);
        Ok(binding)
    }
    fn checked(&self, node: &Node) -> Result<(), Errno> {
        if let Node::Transfer { object, .. } | Node::Watch { object, .. } = node {
            self.objects.read(*object, 0, 0)?;
        }
        Ok(())
    }
}

pub struct PreparedPortalClientRoot<'a> {
    attached: &'a mut bool,
    epoch: u64,
}
impl PreparedFileRoot for PreparedPortalClientRoot<'_> {
    type Node = Node;
    fn publish(self) -> Attachment<Node> {
        *self.attached = true;
        Attachment {
            root: Node::Root,
            epoch: Epoch(self.epoch),
        }
    }
}
impl ProtectedFiles for PortalClientFiles {
    type Prepared<'a> = PreparedPortalClientRoot<'a>;
    fn prepare_authenticated_root(
        &mut self,
        context: &AttachContext<'_>,
    ) -> Result<Self::Prepared<'_>, Errno> {
        if self.connection != Some(context.connection) || self.attached {
            return Err(Errno::EACCES);
        }
        Ok(PreparedPortalClientRoot {
            attached: &mut self.attached,
            epoch: self.epoch,
        })
    }
    fn allocate_auth_qid(&mut self) -> Result<u64, Errno> {
        self.qids(1)
    }
    fn reset_authenticated_root(&mut self, connection: ConnectionId) {
        if self.connection == Some(connection) {
            self.attached = false;
        }
    }
}

impl Export for PortalClientFiles {
    type Node = Node;
    type Handle = Handle;
    fn attach(&mut self, _: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        Err(Errno::EACCES)
    }
    fn check(&mut self, access: &Access<'_, Node>) -> Result<(), Errno> {
        if !self.attached
            || self.connection != Some(access.connection)
            || access.epoch != Epoch(self.epoch)
        {
            return Err(Errno::ESTALE);
        }
        if matches!(access.operation, Operation::Write) {
            return Err(Errno::EACCES);
        }
        self.checked(access.node)
    }
    fn lookup(&mut self, directory: &Node, name: WalkName<'_>) -> Result<Node, Errno> {
        if *directory != Node::Root {
            return Err(Errno::ENOTDIR);
        }
        match name {
            WalkName::Parent => Ok(Node::Root),
            WalkName::Child(b"api") => Ok(Node::Api),
            WalkName::Child(b"status") => Ok(Node::Status),
            WalkName::Child(b"transfer") => {
                let (object, qid) = self.binding()?;
                Ok(Node::Transfer { object, qid })
            }
            WalkName::Child(b"watch") => {
                let (object, qid) = self.binding()?;
                Ok(Node::Watch {
                    object,
                    qid: qid + 1,
                })
            }
            _ => Err(Errno::ENOENT),
        }
    }
    fn describe(&self, node: &Node, handle: Option<&Handle>) -> Entry {
        let (qid, size) = match handle {
            Some(Handle::Snapshot { qid, bytes }) => (*qid, bytes.len() as u64),
            _ => match node {
                Node::Root => (1, 0),
                Node::Api => (2, 0),
                Node::Status => (3, 0),
                Node::Transfer { object, qid } => (*qid, self.objects.size(*object).unwrap_or(0)),
                Node::Watch { qid, .. } => (*qid, 0),
            },
        };
        Entry {
            kind: if *node == Node::Root {
                NodeKind::Directory
            } else {
                NodeKind::File
            },
            qid_path: qid,
            qid_version: 0,
            size,
            permissions: if *node == Node::Root { 0o500 } else { 0o400 },
        }
    }
    fn open(&mut self, node: &Node, flags: OpenFlags) -> Result<Handle, Errno> {
        if flags.0 != 0 {
            return Err(Errno::EACCES);
        }
        self.checked(node)?;
        let bytes = match node {
            Node::Api => format!(
                "sophia-portal-client version=1 epoch={} request=unavailable fd_transfer=none\n",
                self.epoch
            )
            .into_bytes(),
            Node::Status => format!(
                "request=unavailable transfer={}\n",
                match self.objects.current() {
                    Ok(_) => "ready",
                    Err(Errno::ENOENT) => "absent",
                    Err(_) => "revoked",
                }
            )
            .into_bytes(),
            _ => return Ok(Handle::Plain),
        };
        Ok(Handle::Snapshot {
            qid: self.qids(1)?,
            bytes,
        })
    }
    fn read(
        &mut self,
        node: &Node,
        handle: &mut Handle,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        self.checked(node)?;
        match (node, handle) {
            (Node::Root, _) => Err(Errno::EISDIR),
            (Node::Api | Node::Status, Handle::Snapshot { bytes, .. }) => {
                let start = usize::try_from(offset)
                    .unwrap_or(usize::MAX)
                    .min(bytes.len());
                Ok(ReadOutcome::Ready(
                    bytes[start..]
                        .iter()
                        .take(count as usize)
                        .copied()
                        .collect(),
                ))
            }
            (Node::Transfer { object, .. }, _) => self
                .objects
                .read(*object, offset, count)
                .map(ReadOutcome::Ready),
            (Node::Watch { .. }, _) if offset != 0 => Err(Errno::EINVAL),
            (Node::Watch { .. }, _) if count == 0 => Ok(ReadOutcome::Ready(Vec::new())),
            (Node::Watch { .. }, _) => Ok(ReadOutcome::Pending),
            _ => Err(Errno::EIO),
        }
    }
    fn write(&mut self, _: &Node, _: &mut Handle, _: u64, _: &[u8]) -> Result<u32, Errno> {
        Err(Errno::EACCES)
    }
    fn readdir(
        &mut self,
        directory: &Node,
        _: &mut Handle,
        cookie: u64,
        max: usize,
    ) -> Result<Vec<DirEntry>, Errno> {
        if *directory != Node::Root {
            return Err(Errno::ENOTDIR);
        }
        let mut entries = Vec::new();
        for (index, name) in [b"api".as_slice(), b"status", b"transfer", b"watch"]
            .into_iter()
            .enumerate()
        {
            if (index as u64) < cookie || entries.len() == max {
                continue;
            }
            match self.lookup(directory, WalkName::Child(name)) {
                Ok(node) => entries.push(DirEntry {
                    name: name.to_vec(),
                    entry: self.describe(&node, None),
                    next: index as u64 + 1,
                }),
                Err(Errno::ENOENT | Errno::ESTALE) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(entries)
    }
    fn release(&mut self, _: Node, _: Option<Handle>) {}
}
