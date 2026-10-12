use super::*;
use crate::host_domain::Peer;
use sophia_9p::{
    connection::ConnectionId,
    export::*,
    records::{Errno, OpenAccess, OpenFlags},
};
use sophia_protocol::developer_launch::*;
use std::sync::mpsc::SyncSender;
use std::time::Instant;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Node {
    Root,
    Api,
    Request,
    Submit,
    Result,
}
pub(super) enum Handle {
    Directory,
    Bytes(Vec<u8>),
    Request,
    Submit,
}
pub(super) struct LaunchExport {
    domain: Arc<HostDomain>,
    peer: Peer,
    shared: Arc<Shared>,
    sender: SyncSender<DeveloperLaunchTicket>,
    attached: Option<ConnectionId>,
    base: u64,
    accepted: Instant,
    writer: bool,
    candidate: Option<DeveloperLaunchRequest>,
    ticket: Option<DeveloperLaunchTicket>,
}
impl LaunchExport {
    pub(super) fn new(
        domain: Arc<HostDomain>,
        peer: Peer,
        shared: Arc<Shared>,
        sender: SyncSender<DeveloperLaunchTicket>,
        base: u64,
    ) -> Self {
        Self {
            domain,
            peer,
            shared,
            sender,
            attached: None,
            base,
            accepted: Instant::now(),
            writer: false,
            candidate: None,
            ticket: None,
        }
    }
    fn authorized(&self) -> bool {
        !self.shared.stopped.load(Ordering::Acquire) && self.domain.check(&self.peer, &[]).is_ok()
    }
    pub(super) fn service(&mut self) -> bool {
        if !self.authorized() {
            return false;
        }
        let Some(ticket) = &self.ticket else {
            return self.accepted.elapsed() < DEVELOPER_LAUNCH_ADMISSION_TIMEOUT;
        };
        if ticket.received.elapsed() >= DEVELOPER_LAUNCH_ADMISSION_TIMEOUT && !ticket.dispatched() {
            ticket.cancel();
            ticket.finish(
                if ticket.dispatched() {
                    DeveloperLaunchStatus::Indeterminate
                } else {
                    DeveloperLaunchStatus::Unavailable
                },
                None,
            );
        } else if ticket.received.elapsed() >= DEVELOPER_LAUNCH_RESULT_TIMEOUT {
            ticket.finish(DeveloperLaunchStatus::Indeterminate, None);
        } else if ticket.check_requested() {
            // The worker has just inspected the live kernel peer; Session never
            // does /proc IO while servicing this claim in its owner turn.
            ticket.verified();
        }
        ticket.received.elapsed() < DEVELOPER_LAUNCH_RESULT_TIMEOUT + Duration::from_secs(5)
    }
    fn qid(&self, node: Node) -> u64 {
        self.base
            + match node {
                Node::Root => 0,
                Node::Api => 1,
                Node::Request => 2,
                Node::Submit => 3,
                Node::Result => 4,
            }
    }
}
impl Drop for LaunchExport {
    fn drop(&mut self) {
        if let Some(ticket) = &self.ticket {
            ticket.cancel();
        }
    }
}
impl Export for LaunchExport {
    type Node = Node;
    type Handle = Handle;
    fn attach(&mut self, context: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        if self.attached.is_some()
            || !self.authorized()
            || !context
                .peer
                .is_some_and(|p| p.pid > 0 && p.pid as u32 == self.peer.pid)
            || !context.uname.is_empty()
            || !context.aname.is_empty()
        {
            return Err(Errno::EACCES);
        }
        self.attached = Some(context.connection);
        Ok(Attachment {
            root: Node::Root,
            epoch: Epoch(self.base),
        })
    }
    fn wire_reset(&mut self, _: ConnectionId) {
        self.attached = None;
    }
    fn connection_closed(&mut self, _: ConnectionId) {
        if let Some(ticket) = &self.ticket {
            ticket.cancel();
        }
    }
    fn check(&mut self, access: &Access<'_, Node>) -> Result<(), Errno> {
        if self.attached != Some(access.connection)
            || access.epoch != Epoch(self.base)
            || !self.authorized()
        {
            return Err(Errno::ESTALE);
        }
        let writable = matches!(access.node, Node::Request | Node::Submit);
        if matches!(access.operation, Operation::Write) && !writable {
            return Err(Errno::EACCES);
        }
        if let Operation::Open(flags) = access.operation {
            let expected = if writable {
                OpenAccess::Write
            } else {
                OpenAccess::Read
            };
            if flags.access() != Some(expected) || flags.truncate() || flags.append() {
                return Err(Errno::EACCES);
            }
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
            WalkName::Child(b"request") => Ok(Node::Request),
            WalkName::Child(b"submit") => Ok(Node::Submit),
            WalkName::Child(b"result") => Ok(Node::Result),
            _ => Err(Errno::ENOENT),
        }
    }
    fn describe(&self, node: &Node, handle: Option<&Handle>) -> Entry {
        Entry {
            kind: if *node == Node::Root {
                NodeKind::Directory
            } else {
                NodeKind::File
            },
            qid_path: self.qid(*node),
            qid_version: 0,
            permissions: match node {
                Node::Root => 0o500,
                Node::Request | Node::Submit => 0o200,
                _ => 0o400,
            },
            size: match handle {
                Some(Handle::Bytes(bytes)) => bytes.len() as u64,
                _ if *node == Node::Api => DEVELOPER_LAUNCH_API.len() as u64,
                _ => 0,
            },
        }
    }
    fn open(&mut self, node: &Node, flags: OpenFlags) -> Result<Handle, Errno> {
        let write = matches!(node, Node::Request | Node::Submit);
        if flags.access()
            != Some(if write {
                OpenAccess::Write
            } else {
                OpenAccess::Read
            })
            || flags.truncate()
            || flags.append()
        {
            return Err(Errno::EACCES);
        }
        match node {
            Node::Root => Ok(Handle::Directory),
            Node::Api => Ok(Handle::Bytes(DEVELOPER_LAUNCH_API.as_bytes().to_vec())),
            Node::Request => {
                if self.writer || self.ticket.is_some() {
                    return Err(Errno(16));
                }
                self.writer = true;
                Ok(Handle::Request)
            }
            Node::Submit => Ok(Handle::Submit),
            Node::Result => {
                let result = self
                    .ticket
                    .as_ref()
                    .and_then(DeveloperLaunchTicket::result)
                    .ok_or(Errno::EAGAIN)?;
                Ok(Handle::Bytes(
                    encode_developer_launch_result(&result).map_err(|_| Errno::EIO)?,
                ))
            }
        }
    }
    fn read(
        &mut self,
        _: &Node,
        handle: &mut Handle,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        let Handle::Bytes(bytes) = handle else {
            return Err(Errno::EACCES);
        };
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        Ok(ReadOutcome::Ready(
            bytes[start..start.saturating_add(count as usize).min(bytes.len())].to_vec(),
        ))
    }
    fn write(
        &mut self,
        node: &Node,
        handle: &mut Handle,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, Errno> {
        if offset != 0 {
            return Err(Errno::EINVAL);
        }
        match (node, handle) {
            (Node::Request, Handle::Request) => {
                if self.ticket.is_some() {
                    return Err(Errno(114));
                }
                let candidate = decode_developer_launch_request(data).map_err(|_| Errno::EINVAL)?;
                self.candidate = Some(candidate);
            }
            (Node::Submit, Handle::Submit) => {
                let id = u32::from_le_bytes(data.try_into().map_err(|_| Errno::EINVAL)?);
                if id == 0 {
                    return Err(Errno::EINVAL);
                }
                if let Some(ticket) = &self.ticket {
                    return if ticket.request.request == id {
                        Ok(data.len() as u32)
                    } else {
                        Err(Errno(114))
                    };
                }
                let candidate = self
                    .candidate
                    .as_ref()
                    .filter(|c| c.request == id)
                    .ok_or(Errno::EINVAL)?;
                let ticket = DeveloperLaunchTicket::new(candidate.clone(), self.shared.clone());
                self.sender
                    .try_send(ticket.clone())
                    .map_err(|_| Errno::EAGAIN)?;
                self.ticket = Some(ticket);
                self.candidate = None;
                self.shared.owner_wake.notify();
            }
            _ => return Err(Errno::EACCES),
        }
        Ok(data.len() as u32)
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
        let entries: [(&[u8], Node); 4] = [
            (b"api", Node::Api),
            (b"request", Node::Request),
            (b"submit", Node::Submit),
            (b"result", Node::Result),
        ];
        let start = usize::try_from(cookie).map_err(|_| Errno::EINVAL)?;
        if start > entries.len() {
            return Err(Errno::EINVAL);
        }
        Ok(entries
            .iter()
            .enumerate()
            .skip(start)
            .take(max)
            .map(|(i, (name, node))| DirEntry {
                name: name.to_vec(),
                entry: self.describe(node, None),
                next: i as u64 + 1,
            })
            .collect())
    }
    fn release(&mut self, _: Node, handle: Option<Handle>) {
        if matches!(handle, Some(Handle::Request)) {
            self.writer = false;
            self.candidate = None;
        }
    }
}
