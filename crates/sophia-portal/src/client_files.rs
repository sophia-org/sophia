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
    Capture,
    CaptureStatus,
    Transfer { object: u64, qid: u64 },
    FrameInfo { object: u64, qid: u64 },
    Frame { object: u64, qid: u64 },
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
            next_qid: 6,
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
        let binding = (object, self.qids(4)?);
        self.binding = Some(binding);
        Ok(binding)
    }
    fn checked(&self, node: &Node) -> Result<(), Errno> {
        if matches!(node, Node::Capture | Node::CaptureStatus)
            && self.objects.capture_requests().is_none()
        {
            return Err(Errno::ENOENT);
        }
        if let Node::Transfer { object, .. }
        | Node::Watch { object, .. }
        | Node::FrameInfo { object, .. }
        | Node::Frame { object, .. } = node
        {
            self.objects.read(*object, 0, 0)?;
        }
        if let Node::FrameInfo { object, .. } | Node::Frame { object, .. } = node {
            self.objects.frame_size(*object)?;
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
    fn guard_read_delivery(&self, node: &Node) -> bool {
        matches!(
            node,
            Node::Transfer { .. } | Node::FrameInfo { .. } | Node::Frame { .. }
        )
    }
    fn with_delivery<T>(
        &mut self,
        access: &Access<'_, Node>,
        deliver: impl FnOnce() -> T,
    ) -> Result<T, Errno> {
        self.check(access)?;
        match access.node {
            Node::Transfer { object, .. }
            | Node::FrameInfo { object, .. }
            | Node::Frame { object, .. } => self.objects.with_delivery(*object, deliver),
            _ => Ok(deliver()),
        }
    }
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
        if matches!(access.operation, Operation::Write) && *access.node != Node::Capture {
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
            WalkName::Child(name @ (b"capture" | b"capture-status")) => {
                self.objects.capture_requests().ok_or(Errno::ENOENT)?;
                Ok(if name == b"capture" {
                    Node::Capture
                } else {
                    Node::CaptureStatus
                })
            }
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
            WalkName::Child(name @ (b"frame-info" | b"frame")) => {
                let (object, qid) = self.binding()?;
                self.objects.frame_size(object)?;
                if name == b"frame-info" {
                    Ok(Node::FrameInfo {
                        object,
                        qid: qid + 2,
                    })
                } else {
                    Ok(Node::Frame {
                        object,
                        qid: qid + 3,
                    })
                }
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
                Node::Capture => (4, 0),
                Node::CaptureStatus => (5, 0),
                Node::Transfer { object, qid } | Node::FrameInfo { object, qid } => {
                    (*qid, self.objects.size(*object).unwrap_or(0))
                }
                Node::Frame { object, qid } => {
                    (*qid, self.objects.frame_size(*object).unwrap_or(0))
                }
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
            permissions: match node {
                Node::Root => 0o500,
                Node::Capture => 0o200,
                _ => 0o400,
            },
        }
    }
    fn open(&mut self, node: &Node, flags: OpenFlags) -> Result<Handle, Errno> {
        if flags.0 != if *node == Node::Capture { 1 } else { 0 } {
            return Err(Errno::EACCES);
        }
        self.checked(node)?;
        let bytes = match node {
            Node::Api => format!(
                "sophia-portal-client version=1 epoch={} request={} fd_transfer=none\n",
                self.epoch,
                if self.objects.capture_requests().is_some() {
                    "output-capture-v1"
                } else {
                    "unavailable"
                }
            )
            .into_bytes(),
            Node::Status => format!(
                "request={} transfer={}\n",
                if self.objects.capture_requests().is_some() {
                    "output-capture-v1"
                } else {
                    "unavailable"
                },
                match self.objects.current() {
                    Ok(_) => "ready",
                    Err(Errno::ENOENT) => "absent",
                    Err(_) => "revoked",
                }
            )
            .into_bytes(),
            Node::CaptureStatus => self
                .objects
                .capture_requests()
                .ok_or(Errno::ENOENT)?
                .status()
                .encode()
                .ok_or(Errno::EIO)?
                .to_vec(),
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
            (Node::Api | Node::Status | Node::CaptureStatus, Handle::Snapshot { bytes, .. }) => {
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
            (Node::Transfer { object, .. } | Node::FrameInfo { object, .. }, _) => self
                .objects
                .read(*object, offset, count)
                .map(ReadOutcome::Ready),
            (Node::Frame { object, .. }, _) => self
                .objects
                .read_frame(*object, offset, count)
                .map(ReadOutcome::Ready),
            (Node::Watch { .. }, _) if offset != 0 => Err(Errno::EINVAL),
            (Node::Watch { .. }, _) if count == 0 => Ok(ReadOutcome::Ready(Vec::new())),
            (Node::Watch { .. }, _) => Ok(ReadOutcome::Pending),
            _ => Err(Errno::EIO),
        }
    }
    fn write(
        &mut self,
        node: &Node,
        _: &mut Handle,
        offset: u64,
        bytes: &[u8],
    ) -> Result<u32, Errno> {
        if *node != Node::Capture {
            return Err(Errno::EACCES);
        }
        if offset != 0 {
            return Err(Errno::EINVAL);
        }
        let request = sophia_protocol::output_capture_request::OutputCaptureRequest::decode(bytes)
            .ok_or(Errno::EINVAL)?;
        self.objects
            .capture_requests()
            .ok_or(Errno::ENOENT)?
            .submit(request)
            .map_err(|error| {
                use crate::capture_requests::CaptureRequestError as Error;
                match error {
                    Error::Invalid => Errno::EINVAL,
                    Error::Busy => Errno::EAGAIN,
                    Error::Denied => Errno::EACCES,
                    Error::Stale => Errno::ESTALE,
                }
            })?;
        Ok(bytes.len() as u32)
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
        for (index, name) in [
            b"api".as_slice(),
            b"status",
            b"transfer",
            b"watch",
            b"frame-info",
            b"frame",
            b"capture",
            b"capture-status",
        ]
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
