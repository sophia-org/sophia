use super::*;
use sophia_9p::{
    Access, AttachContext, Attachment, ConnectionId, DirEntry, Entry, Epoch, Export, NodeKind,
    OpenAccess, OpenFlags, ReadOutcome, WalkName,
};
use sophia_protocol::admin_files::{
    AdminInfo, AdminOutcome, AdminStatus, decode_admin_invocation, encode_admin_catalog,
    encode_admin_info, encode_admin_status,
};
use std::rc::Weak;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Node {
    Root,
    Info,
    Commands,
    Status,
    Ctl,
}

pub(super) struct Snapshot {
    qid: u64,
    bytes: Vec<u8>,
    generation: u64,
}

pub(super) enum Handle {
    Directory,
    Bytes(Rc<Snapshot>),
    Ctl,
}

pub(super) struct AdminExport {
    shared: Rc<RefCell<Shared>>,
    peer: Peer,
    id: u64,
    base_qid: u64,
    attached: Option<ConnectionId>,
    ever_attached: bool,
    last_activity: Instant,
    last_request: u64,
    discovered: u64,
    catalog: Weak<Snapshot>,
    pending: Option<ControlTicket>,
    status: AdminStatus,
}

impl AdminExport {
    pub fn new(shared: Rc<RefCell<Shared>>, peer: Peer, id: u64) -> Result<Self, Errno> {
        let base_qid = shared.borrow_mut().qids(5)?;
        Ok(Self {
            shared,
            peer,
            id,
            base_qid,
            attached: None,
            ever_attached: false,
            last_activity: Instant::now(),
            last_request: 0,
            discovered: 0,
            catalog: Weak::new(),
            pending: None,
            status: AdminStatus::Ready { connection: id },
        })
    }

    pub fn advance(&mut self, now: Instant) -> bool {
        let shared = self.shared.borrow();
        let authorized = shared.authorized(&self.peer);
        if let Some(ticket) = &self.pending {
            if !authorized {
                if ticket.cancel_queued() {
                    ticket.finish(ControlOutcome::Denied);
                }
                shared.owner_wake.notify();
            } else if ticket.state.phase.load(Ordering::Acquire) == 3 {
                let _ =
                    ticket
                        .state
                        .phase
                        .compare_exchange(3, 4, Ordering::AcqRel, Ordering::Acquire);
                shared.owner_wake.notify();
            }
            if now.duration_since(ticket.received) >= COMMAND_TIMEOUT {
                ticket.expire();
            }
            let outcome = ticket.state.outcome.load(Ordering::Acquire);
            if let Ok(outcome) = ControlOutcome::from_wire(outcome) {
                self.status = AdminStatus::Outcome(AdminOutcome {
                    connection: self.id,
                    request: ticket.request,
                    generation: ticket.generation,
                    outcome,
                    detail: String::new(),
                });
                self.pending = None;
            }
        }
        authorized
            && now.duration_since(self.last_activity) < IDLE_TIMEOUT
            && (self.ever_attached || now.duration_since(self.last_activity) < FRAME_TIMEOUT)
    }

    fn invoke(&mut self, data: &[u8]) -> Result<u32, Errno> {
        let invocation = decode_admin_invocation(data).map_err(|_| Errno::EINVAL)?;
        if invocation.connection != self.id || invocation.request <= self.last_request {
            return Err(Errno::EINVAL);
        }
        if self.pending.is_some() {
            return Err(Errno(16));
        }
        let mut shared = self.shared.borrow_mut();
        // Keep publication excluded while admitting the ticket. The owner
        // still checks its current generation and claims immediately at dispatch.
        let view_handle = shared.view.clone();
        let view = view_handle.lock().map_err(|_| Errno::EIO)?;
        shared
            .domain
            .check(&self.peer, &view.excluded)
            .map_err(|_| Errno::EACCES)?;
        shared
            .active
            .retain(|t| !t.state.settled.load(Ordering::Acquire));
        if shared.active.len() >= CONTROL_MAX_PENDING {
            return Err(Errno::EAGAIN);
        }
        let refusal = if invocation.generation != self.discovered
            || invocation.generation != view.catalog.generation
        {
            Some(ControlOutcome::Stale)
        } else if !view.catalog.commands.contains(&invocation.command) {
            Some(ControlOutcome::Rejected)
        } else {
            None
        };
        let status = if let Some(outcome) = refusal {
            AdminStatus::Outcome(AdminOutcome {
                connection: self.id,
                request: invocation.request,
                generation: invocation.generation,
                outcome,
                detail: String::new(),
            })
        } else {
            let ticket = ControlTicket {
                connection: self.id,
                request: invocation.request,
                generation: invocation.generation,
                command: invocation.command,
                received: Instant::now(),
                state: Arc::new(TicketState {
                    phase: AtomicU8::new(0),
                    outcome: AtomicU16::new(0),
                    settled: AtomicBool::new(false),
                    wake: shared.wake.clone(),
                }),
            };
            // Queue refusal changes neither the watermark nor the visible result.
            shared
                .requests
                .try_send(ticket.clone())
                .map_err(|_| Errno::EAGAIN)?;
            shared.active.push(ticket.clone());
            self.pending = Some(ticket);
            AdminStatus::Pending {
                connection: self.id,
                request: invocation.request,
                generation: invocation.generation,
            }
        };
        self.last_request = invocation.request;
        self.status = status;
        Ok(data.len() as u32)
    }
}

impl Drop for AdminExport {
    fn drop(&mut self) {
        // Also cover an I/O failure that ends the driver before its normal
        // connection_closed callback. Dispatched work stays with its owner.
        if let Some(ticket) = &self.pending {
            ticket.disconnect();
        }
    }
}

impl Export for AdminExport {
    type Node = Node;
    type Handle = Handle;

    fn attach(&mut self, context: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        if self.attached.is_some()
            || !self.shared.borrow().authorized(&self.peer)
            || !context
                .peer
                .is_some_and(|p| p.pid > 0 && p.pid as u32 == self.peer.pid)
        {
            return Err(Errno::EACCES);
        }
        // Textual attach claims cannot choose an administrative identity.
        self.attached = Some(context.connection);
        self.ever_attached = true;
        self.last_activity = Instant::now();
        Ok(Attachment {
            root: Node::Root,
            epoch: Epoch(self.id),
        })
    }

    fn check(&mut self, access: &Access<'_, Node>) -> Result<(), Errno> {
        if self.attached != Some(access.connection)
            || access.epoch != Epoch(self.id)
            || !self.shared.borrow().authorized(&self.peer)
        {
            return Err(Errno::ESTALE);
        }
        self.last_activity = Instant::now();
        Ok(())
    }

    fn guard_read_delivery(&self, _: &Node) -> bool {
        true
    }

    fn with_delivery<T>(
        &mut self,
        access: &Access<'_, Node>,
        deliver: impl FnOnce() -> T,
    ) -> Result<T, Errno> {
        self.check(access)?;
        let shared = self.shared.borrow();
        let view = shared.view.lock().map_err(|_| Errno::EIO)?;
        shared
            .domain
            .check(&self.peer, &view.excluded)
            .map_err(|_| Errno::ESTALE)?;
        Ok(deliver())
    }

    fn lookup(&mut self, directory: &Node, name: WalkName<'_>) -> Result<Node, Errno> {
        if *directory != Node::Root {
            return Err(Errno::ENOTDIR);
        }
        match name {
            WalkName::Parent => Ok(Node::Root),
            WalkName::Child(b"info") => Ok(Node::Info),
            WalkName::Child(b"commands") => Ok(Node::Commands),
            WalkName::Child(b"status") => Ok(Node::Status),
            WalkName::Child(b"ctl") => Ok(Node::Ctl),
            WalkName::Child(_) => Err(Errno::ENOENT),
        }
    }

    fn describe(&self, node: &Node, handle: Option<&Handle>) -> Entry {
        let (qid_path, size) = match handle {
            Some(Handle::Bytes(s)) => (s.qid, s.bytes.len() as u64),
            _ => (self.base_qid + *node as u64, 0),
        };
        Entry {
            kind: if *node == Node::Root {
                NodeKind::Directory
            } else {
                NodeKind::File
            },
            qid_path,
            qid_version: 0,
            permissions: match node {
                Node::Root => 0o500,
                Node::Ctl => 0o200,
                _ => 0o400,
            },
            size,
        }
    }

    fn open(&mut self, node: &Node, flags: OpenFlags) -> Result<Handle, Errno> {
        let expected = if *node == Node::Ctl {
            OpenAccess::Write
        } else {
            OpenAccess::Read
        };
        if flags.access() != Some(expected) || flags.truncate() || flags.append() {
            return Err(Errno::EACCES);
        }
        if *node == Node::Root {
            return Ok(Handle::Directory);
        }
        if *node == Node::Ctl {
            return Ok(Handle::Ctl);
        }
        if *node == Node::Commands
            && let Some(snapshot) = self.catalog.upgrade()
        {
            self.discovered = snapshot.generation;
            return Ok(Handle::Bytes(snapshot));
        }
        let mut shared = self.shared.borrow_mut();
        let (bytes, generation) = match node {
            Node::Info => (
                encode_admin_info(&AdminInfo {
                    session_id: shared.session_id,
                    connection: self.id,
                    command_timeout_ms: 10000,
                    frame_timeout_ms: 2000,
                    idle_timeout_ms: 60000,
                }),
                0,
            ),
            Node::Status => (encode_admin_status(&self.status), 0),
            Node::Commands => {
                let view = shared.view.lock().map_err(|_| Errno::EIO)?;
                (encode_admin_catalog(&view.catalog), view.catalog.generation)
            }
            _ => unreachable!(),
        };
        let snapshot = Rc::new(Snapshot {
            bytes: bytes.map_err(|_| Errno::EIO)?,
            qid: shared.qids(1)?,
            generation,
        });
        if *node == Node::Commands {
            self.discovered = generation;
            self.catalog = Rc::downgrade(&snapshot);
        }
        Ok(Handle::Bytes(snapshot))
    }

    fn read(
        &mut self,
        _: &Node,
        handle: &mut Handle,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        let Handle::Bytes(snapshot) = handle else {
            return Err(Errno::EACCES);
        };
        let offset = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(snapshot.bytes.len());
        let end = offset
            .saturating_add(count as usize)
            .min(snapshot.bytes.len());
        Ok(ReadOutcome::Ready(snapshot.bytes[offset..end].to_vec()))
    }

    fn write(
        &mut self,
        node: &Node,
        handle: &mut Handle,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, Errno> {
        if *node != Node::Ctl || !matches!(handle, Handle::Ctl) {
            return Err(Errno::EACCES);
        }
        if offset != 0 {
            return Err(Errno::EINVAL);
        }
        self.invoke(data)
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
            (b"info", Node::Info),
            (b"commands", Node::Commands),
            (b"status", Node::Status),
            (b"ctl", Node::Ctl),
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

    fn release(&mut self, _: Node, _: Option<Handle>) {}

    fn wire_reset(&mut self, _: ConnectionId) {
        // Tversion discards fids, not the transport's identity, replay watermark
        // or accepted command. Reattaching cannot make an old write new again.
        self.attached = None;
    }

    fn connection_closed(&mut self, _: ConnectionId) {
        if let Some(ticket) = self.pending.take() {
            ticket.disconnect();
        }
        self.attached = None;
    }
}
