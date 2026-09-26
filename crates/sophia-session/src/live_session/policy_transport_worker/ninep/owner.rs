use super::*;
use sophia_9p::connection::ConnectionId;

const API: &[u8] = b"sophia-wm-files version=1 output_transport=current_ipc\n";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum Node {
    Root,
    Api,
    Limits,
    Snapshot,
    Events,
    Transaction,
    Submit,
    Ack,
}
pub(in super::super) struct Snapshot {
    qid: u64,
    bytes: Vec<u8>,
}
pub(in super::super) enum Handle {
    Plain,
    Snapshot(Arc<Snapshot>),
    Transaction(u64),
}

struct Accepted {
    submit: WmFileSubmit,
    bytes: Vec<u8>,
    handle: u64,
    sequence: u64,
}

pub(in super::super) struct WmFiles<C> {
    epoch: u64,
    capabilities: u64,
    connection: Option<ConnectionId>,
    attached: bool,
    revoked: bool,
    qids: WmQids,
    base: u64,
    codec: C,
    limits: Vec<u8>,
    snapshot: Option<Arc<Snapshot>>,
    snapshot_open: bool,
    staging: Option<Staging>,
    accepted: Option<Accepted>,
    watermark: u64,
    journal: Journal,
    permit: Option<PolicyReceivePermit>,
    delivery: Option<PolicyAdapterEvent>,
}

fn object(bytes: &[u8], epoch: u64, kind: WmFileKind) -> Result<(), Errno> {
    let record = decode_wm_file_record(bytes, WmFileClass::Object).map_err(|_| Errno::EINVAL)?;
    if record.header.connection_epoch != epoch {
        return Err(Errno::ESTALE);
    }
    if record.header.kind != kind {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

fn slice(bytes: &[u8], offset: u64, count: u32) -> ReadOutcome {
    let start = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    let end = start.saturating_add(count as usize).min(bytes.len());
    ReadOutcome::Ready(bytes[start..end].to_vec())
}

impl<C: PolicyFileCodec> WmFiles<C> {
    /// Admission and capability negotiation are supplied by their existing
    /// owner. This custody fixture does not turn attach names into authority.
    pub(super) fn new(
        epoch: u64,
        capabilities: u64,
        limits: Vec<u8>,
        qids: WmQids,
        codec: C,
    ) -> Result<Self, Errno> {
        object(&limits, epoch, WmFileKind::Limits)?;
        let base = qids.allocate(8)?;
        Ok(Self {
            epoch,
            capabilities,
            connection: None,
            attached: false,
            revoked: false,
            qids,
            base,
            codec,
            limits,
            snapshot: None,
            snapshot_open: false,
            staging: None,
            accepted: None,
            watermark: 0,
            journal: Journal::new(epoch),
            permit: None,
            delivery: None,
        })
    }

    pub(super) fn bind_connection(&mut self, connection: ConnectionId) {
        self.connection = Some(connection);
    }
    pub(super) fn revoke(&mut self) {
        self.revoked = true;
        self.permit = None;
        self.delivery = None;
        self.staging = None;
        self.accepted = None;
    }
    pub(super) fn publish_snapshot(&mut self, bytes: Vec<u8>) -> Result<u64, Errno> {
        if self.revoked {
            return Err(Errno::ESTALE);
        }
        object(&bytes, self.epoch, WmFileKind::Snapshot)?;
        let qid = self.qids.allocate(1)?;
        self.snapshot = Some(Arc::new(Snapshot { qid, bytes }));
        Ok(qid)
    }
    pub(super) fn offer(&mut self, permit: PolicyReceivePermit) -> Result<(), Errno> {
        if self.revoked {
            return Err(Errno::ESTALE);
        }
        if self.permit.is_some() || self.delivery.is_some() {
            return Err(EBUSY);
        }
        self.permit = Some(permit);
        Ok(())
    }
    pub(super) fn withdraw_permit(&mut self) {
        self.permit = None;
    }
    pub(super) fn take_delivery(&mut self) -> Option<PolicyAdapterEvent> {
        self.delivery.take()
    }
    pub(super) fn append_event(&mut self, kind: WmFileKind, body: &[u8]) -> Result<u64, Errno> {
        if self.revoked {
            return Err(Errno::ESTALE);
        }
        self.journal.append(kind, body)
    }
    pub(super) fn expire(&mut self) {
        if self
            .staging
            .as_ref()
            .is_some_and(|s| s.expired(Instant::now()))
        {
            self.staging = None;
        }
    }
    pub(super) fn next_wait(&mut self, maximum: Duration) -> Duration {
        self.expire();
        self.staging
            .as_ref()
            .map_or(maximum, |s| s.wait(Instant::now(), maximum))
    }

    fn submit(&mut self, bytes: &[u8]) -> Result<(), Errno> {
        let submit = decode_wm_file_submit(bytes).map_err(|_| Errno::EINVAL)?;
        if submit.connection_epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        // Accepted bytes cannot be edited. A retry only names that same
        // retained object and never spends the next driver-issued permit.
        if let Some(accepted) = &self.accepted {
            return if accepted.submit == submit {
                Ok(())
            } else {
                Err(EBUSY)
            };
        }
        if submit.submission_id <= self.watermark {
            return Err(EALREADY);
        }
        // Absence of driver admission must not spend row-decoding work. The
        // accepted replay above remains available independently of a permit.
        if self.permit.is_none() || self.delivery.is_some() {
            return Err(Errno::EAGAIN);
        }
        let staging = self.staging.as_ref().ok_or(Errno::ESTALE)?;
        if staging.bytes.len() != submit.candidate_bytes as usize {
            return Err(Errno::EINVAL);
        }
        let record = decode_wm_file_record(&staging.bytes, WmFileClass::Candidate)
            .map_err(|_| Errno::EINVAL)?;
        if record.header.connection_epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        if record.header.submission_id != submit.submission_id {
            return Err(Errno::EINVAL);
        }
        let decoded = self
            .codec
            .decode_candidate(&staging.bytes, self.capabilities)?;
        if decoded.required_capabilities & !self.capabilities != 0 {
            return Err(Errno::EACCES);
        }
        if self.delivery.is_some()
            || !self
                .permit
                .as_ref()
                .is_some_and(|p| p.allows(&decoded.event))
        {
            return Err(Errno::EAGAIN);
        }
        let body = self
            .codec
            .submitted_body(submit.submission_id, record.header.kind)?;
        // Every fallible check precedes custody transfer, including complete
        // journal capacity and sequence/offset exhaustion. No prefix is spent.
        let sequence = self.journal.append(WmFileKind::Submitted, &body)?;
        let staging = self.staging.take().expect("candidate checked");
        self.accepted = Some(Accepted {
            submit,
            bytes: staging.bytes,
            handle: staging.handle,
            sequence,
        });
        self.watermark = submit.submission_id;
        self.permit = None;
        self.delivery = Some(decoded.event);
        Ok(())
    }
}

impl<C: PolicyFileCodec> Export for WmFiles<C> {
    type Node = Node;
    type Handle = Handle;
    fn attach(&mut self, context: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        // One attach per admitted epoch. Version reset releases fids but does
        // not renew role authority; reattachment needs a fresh admitted stream.
        if self.revoked || self.attached || self.connection != Some(context.connection) {
            return Err(Errno::EACCES);
        }
        self.attached = true;
        Ok(Attachment {
            root: Node::Root,
            epoch: Epoch(self.epoch),
        })
    }
    fn check(&mut self, access: &Access<'_, Node>) -> Result<(), Errno> {
        self.expire();
        if self.revoked
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
            WalkName::Child(name) => match name {
                b"api" => Ok(Node::Api),
                b"limits" => Ok(Node::Limits),
                b"snapshot" => Ok(Node::Snapshot),
                b"events" => Ok(Node::Events),
                b"transaction" => Ok(Node::Transaction),
                b"submit" => Ok(Node::Submit),
                b"ack" => Ok(Node::Ack),
                _ => Err(Errno::ENOENT),
            },
        }
    }
    fn describe(&self, node: &Node, handle: Option<&Handle>) -> Entry {
        let mut qid_path = self.base + *node as u64;
        if let Some(Handle::Transaction(id)) = handle {
            qid_path = *id;
        }
        let size = match (node, handle) {
            (Node::Api, _) => API.len() as u64,
            (Node::Limits, _) => self.limits.len() as u64,
            (Node::Events, _) => self.journal.size(),
            (Node::Snapshot, Some(Handle::Snapshot(snapshot))) => {
                qid_path = snapshot.qid;
                snapshot.bytes.len() as u64
            }
            (Node::Snapshot, _) => self.snapshot.as_ref().map_or(0, |s| {
                qid_path = s.qid;
                s.bytes.len() as u64
            }),
            (Node::Transaction, Some(Handle::Transaction(id))) => self
                .staging
                .as_ref()
                .filter(|s| s.handle == *id)
                .map_or_else(
                    || {
                        self.accepted
                            .as_ref()
                            .filter(|a| a.handle == *id)
                            .map_or(0, |a| a.bytes.len() as u64)
                    },
                    |s| s.bytes.len() as u64,
                ),
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
            permissions: match node {
                Node::Root => 0o500,
                Node::Transaction => 0o600,
                Node::Submit | Node::Ack => 0o200,
                _ => 0o400,
            },
            size,
        }
    }
    fn open(&mut self, node: &Node, flags: OpenFlags) -> Result<Handle, Errno> {
        let access = flags.access().ok_or(Errno::EINVAL)?;
        let valid = match node {
            Node::Transaction => access == OpenAccess::ReadWrite,
            Node::Submit | Node::Ack => access == OpenAccess::Write,
            _ => access == OpenAccess::Read,
        };
        if !valid || flags.truncate() || flags.append() {
            return Err(Errno::EACCES);
        }
        match node {
            Node::Snapshot => {
                if self.snapshot_open {
                    return Err(EBUSY);
                }
                let snapshot = self.snapshot.clone().ok_or(Errno::EAGAIN)?;
                self.snapshot_open = true;
                Ok(Handle::Snapshot(snapshot))
            }
            Node::Transaction => {
                self.expire();
                if self.staging.is_some() || self.accepted.is_some() {
                    return Err(EBUSY);
                }
                let id = self.qids.allocate(1)?;
                self.staging = Some(Staging::new(id));
                Ok(Handle::Transaction(id))
            }
            _ => Ok(Handle::Plain),
        }
    }
    fn read(
        &mut self,
        node: &Node,
        handle: &mut Handle,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        match (node, handle) {
            (Node::Api, _) => Ok(slice(API, offset, count)),
            (Node::Limits, _) => Ok(slice(&self.limits, offset, count)),
            (Node::Snapshot, Handle::Snapshot(snapshot)) => {
                Ok(slice(&snapshot.bytes, offset, count))
            }
            (Node::Events, _) => self.journal.read(offset, count),
            (Node::Transaction, Handle::Transaction(id)) => {
                self.expire();
                let bytes = self
                    .staging
                    .as_ref()
                    .filter(|s| s.handle == *id)
                    .map(|s| &s.bytes)
                    .or_else(|| {
                        self.accepted
                            .as_ref()
                            .filter(|s| s.handle == *id)
                            .map(|s| &s.bytes)
                    })
                    .ok_or(Errno::ESTALE)?;
                Ok(slice(bytes, offset, count))
            }
            _ => Err(Errno::EACCES),
        }
    }
    fn write(
        &mut self,
        node: &Node,
        handle: &mut Handle,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, Errno> {
        self.expire();
        match (node, handle) {
            (Node::Transaction, Handle::Transaction(id)) => self
                .staging
                .as_mut()
                .filter(|s| s.handle == *id)
                .ok_or(Errno::ESTALE)?
                .write(offset, data, Instant::now()),
            (Node::Submit, _) if offset == 0 => {
                self.submit(data)?;
                Ok(data.len() as u32)
            }
            (Node::Ack, _) if offset == 0 => {
                let ack = decode_wm_file_ack(data).map_err(|_| Errno::EINVAL)?;
                self.journal.ack(ack)?;
                if self
                    .accepted
                    .as_ref()
                    .is_some_and(|a| a.sequence <= ack.sequence)
                {
                    self.accepted = None;
                }
                Ok(data.len() as u32)
            }
            _ => Err(Errno::EINVAL),
        }
    }
    fn release(&mut self, _node: Node, handle: Option<Handle>) {
        match handle {
            Some(Handle::Snapshot(_)) => self.snapshot_open = false,
            Some(Handle::Transaction(id))
                if self.staging.as_ref().is_some_and(|s| s.handle == id) =>
            {
                self.staging = None
            }
            _ => {}
        }
    }
}
