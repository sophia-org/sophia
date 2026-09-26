//! One component's `sophia_shell_fs_v1` export. It owns file custody only:
//! the journal, the attach's candidate buffer and the typed inbound queue.
//! Admission, negotiation and every content decision stay with the existing
//! shell owners, which the transport feeds from `take_inbound`.
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use sophia_9p::connection::ConnectionId;
use sophia_9p::{
    Access, AttachContext, Attachment, DirEntry, Entry, Epoch, Errno, Export, NodeKind, OpenAccess,
    OpenFlags, ReadOutcome, WalkName,
};
use sophia_protocol::shell_files::*;
use sophia_protocol::{ShellContentRecord, ShellV1ClientHello, TransactionId};

use super::journal::{Journal, JournalBounds};
use super::staging::Staging;

/// Not in `sophia-9p`'s set; the WM file owner defines the same values.
const EBUSY: Errno = Errno(16);
const EALREADY: Errno = Errno(114);

/// At most this many accepted submissions wait for the owners, as the
/// socket transport's inbox does.
pub(in crate::shell_transport) const INBOUND_RECORDS: usize = 64;

const ROOT_ENTRIES: [(&[u8], Node); 7] = [
    (b"api", Node::Api),
    (b"limits", Node::Limits),
    (b"outputs", Node::Outputs),
    (b"events", Node::Events),
    (b"transaction", Node::Transaction),
    (b"submit", Node::Submit),
    (b"ack", Node::Ack),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::shell_transport) enum Node {
    Root,
    Api,
    Limits,
    Outputs,
    Events,
    Transaction,
    Submit,
    Ack,
}

pub(in crate::shell_transport) enum Handle {
    Plain,
    Transaction(u64),
    /// A pinned snapshot object: immutable, whatever is published later.
    Object(Arc<Object>),
}

/// One published snapshot object. A new publication never edits it; it
/// lives while it is current or pinned by an open fid.
pub(in crate::shell_transport) struct Object {
    qid: u64,
    bytes: Vec<u8>,
}

/// A submission whose custody the export has taken; the owners decide it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::shell_transport) enum Inbound {
    Negotiate(ShellV1ClientHello),
    Content(TransactionId, Box<ShellContentRecord>),
}

struct Accepted {
    submit: ShellFileSubmit,
    bytes: Vec<u8>,
    handle: u64,
    sequence: u64,
}

pub(in crate::shell_transport) struct ShellFiles {
    epoch: u64,
    api: Vec<u8>,
    connection: Option<ConnectionId>,
    attached: bool,
    revoked: bool,
    negotiate_accepted: bool,
    negotiated: bool,
    content: bool,
    qid_base: u64,
    next_qid: u64,
    limits: Option<Vec<u8>>,
    outputs: Option<Arc<Object>>,
    outputs_pinned: bool,
    staging: Option<Staging>,
    accepted: Option<Accepted>,
    submission_watermark: u64,
    journal: Journal,
    inbound: VecDeque<Inbound>,
}

fn slice(bytes: &[u8], offset: u64, count: u32) -> ReadOutcome {
    let start = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    let end = start.saturating_add(count as usize).min(bytes.len());
    ReadOutcome::Ready(bytes[start..end].to_vec())
}

impl ShellFiles {
    /// An export awaiting exactly one Negotiate record for `epoch`. Qids are
    /// logical per component and continue from `qid_base` across epochs.
    pub(in crate::shell_transport) fn awaiting_negotiation(
        epoch: u64,
        role: &str,
        bounds: JournalBounds,
        qid_base: u64,
        now: Instant,
    ) -> Self {
        Self {
            epoch,
            api: format!(
                "sophia-shell-files version={SHELL_FILE_API_VERSION} role={role} fd_transfer=none\n"
            )
            .into_bytes(),
            connection: None,
            attached: false,
            revoked: false,
            negotiate_accepted: false,
            negotiated: false,
            content: false,
            qid_base,
            next_qid: qid_base + 8,
            limits: None,
            outputs: None,
            outputs_pinned: false,
            staging: None,
            accepted: None,
            submission_watermark: 0,
            journal: Journal::new(epoch, bounds, now),
            inbound: VecDeque::with_capacity(INBOUND_RECORDS),
        }
    }

    pub(in crate::shell_transport) fn bind_connection(&mut self, connection: ConnectionId) {
        self.connection = Some(connection);
    }

    /// The next unused logical qid, for the component's following epoch.
    pub(in crate::shell_transport) fn next_qid(&self) -> u64 {
        self.next_qid
    }

    pub(in crate::shell_transport) fn revoke(&mut self) {
        self.revoked = true;
        self.staging = None;
        self.accepted = None;
        self.inbound.clear();
    }

    /// Records the owners' selection: the immutable limits object, if the
    /// profile has one, and whether content records may be submitted.
    pub(in crate::shell_transport) fn complete_negotiation(
        &mut self,
        limits: Option<Vec<u8>>,
    ) -> Result<(), Errno> {
        if self.revoked {
            return Err(Errno::ESTALE);
        }
        if self.negotiated {
            return Err(EALREADY);
        }
        self.content = limits.is_some();
        self.limits = limits;
        self.negotiated = true;
        Ok(())
    }

    /// Makes one snapshot object current and journals its publication, as one
    /// step: the event's room is checked before a qid is spent, and nothing
    /// changes on refusal. `Ok(false)` means the journal has no room yet.
    pub(in crate::shell_transport) fn publish_object(
        &mut self,
        kind: ShellFileKind,
        body: &[u8],
        credited: bool,
    ) -> Result<bool, Errno> {
        if self.revoked {
            return Err(Errno::ESTALE);
        }
        if kind != ShellFileKind::Outputs {
            return Err(Errno::EINVAL);
        }
        let bytes = encode_shell_file_record(
            ShellFileHeader {
                kind,
                connection_epoch: self.epoch,
                submission_id: 0,
                sequence: 0,
            },
            body,
        )
        .map_err(|_| Errno::EINVAL)?;
        let facts = decode_shell_file_outputs(&bytes).map_err(|_| Errno::EINVAL)?;
        let sophia_protocol::ShellContentRecord::OutputFacts(facts) = facts.record else {
            return Err(Errno::EINVAL);
        };
        let event = SHELL_FILE_HEADER_BYTES + 24;
        if !self.journal.fits(event, credited) {
            return Ok(false);
        }
        let qid = self.allocate_qid()?;
        let published = encode_shell_file_object_published_body(ShellFileObjectPublished {
            object: kind,
            generation: facts.facts_generation,
            qid,
        })
        .map_err(|_| Errno::EINVAL)?;
        self.journal
            .append(ShellFileKind::ObjectPublished, &published, credited)?;
        self.outputs = Some(Arc::new(Object { qid, bytes }));
        Ok(true)
    }

    pub(in crate::shell_transport) fn journal(&self) -> &Journal {
        &self.journal
    }

    pub(in crate::shell_transport) fn journal_mut(&mut self) -> &mut Journal {
        &mut self.journal
    }

    pub(in crate::shell_transport) fn take_inbound(&mut self) -> Option<Inbound> {
        self.inbound.pop_front()
    }

    /// Removes the first queued content record the predicate selects,
    /// preserving the order of everything else.
    pub(in crate::shell_transport) fn take_content(
        &mut self,
        select: impl Fn(&ShellContentRecord) -> bool,
    ) -> Option<(TransactionId, ShellContentRecord)> {
        let at = self
            .inbound
            .iter()
            .position(|item| matches!(item, Inbound::Content(_, record) if select(record)))?;
        match self.inbound.remove(at) {
            Some(Inbound::Content(transaction, record)) => Some((transaction, *record)),
            _ => None,
        }
    }

    pub(in crate::shell_transport) fn expire(&mut self) {
        if self
            .staging
            .as_ref()
            .is_some_and(|s| s.expired(Instant::now()))
        {
            self.staging = None;
        }
    }

    fn allocate_qid(&mut self) -> Result<u64, Errno> {
        let qid = self.next_qid;
        self.next_qid = qid.checked_add(1).ok_or(Errno::ENOSPC)?;
        Ok(qid)
    }

    /// Decodes a staged candidate into the value the owners receive. The
    /// phase decides which kinds exist: Negotiate once before negotiation,
    /// content records only after a content grant.
    fn decode(&self, bytes: &[u8], kind: ShellFileKind) -> Result<Inbound, Errno> {
        match kind {
            ShellFileKind::Negotiate => {
                if self.negotiate_accepted {
                    return Err(EALREADY);
                }
                decode_shell_file_negotiate(bytes)
                    .map(Inbound::Negotiate)
                    .map_err(|_| Errno::EINVAL)
            }
            ShellFileKind::AllocationRequest => {
                if !self.negotiated || !self.content {
                    return Err(Errno::EACCES);
                }
                let value =
                    decode_shell_file_allocation_request(bytes).map_err(|_| Errno::EINVAL)?;
                Ok(Inbound::Content(value.transaction, Box::new(value.record)))
            }
            _ => Err(Errno::EINVAL),
        }
    }

    fn submit(&mut self, bytes: &[u8]) -> Result<(), Errno> {
        let submit = decode_shell_file_submit(bytes).map_err(|_| Errno::EINVAL)?;
        if submit.connection_epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        // Accepted bytes cannot be edited. An identical retry names the same
        // retained object and never enqueues it again.
        if let Some(accepted) = &self.accepted {
            return if accepted.submit == submit {
                Ok(())
            } else {
                Err(EBUSY)
            };
        }
        if submit.submission_id <= self.submission_watermark {
            return Err(EALREADY);
        }
        // Owner backpressure precedes any decoding work.
        if self.inbound.len() >= INBOUND_RECORDS {
            return Err(Errno::EAGAIN);
        }
        let staging = self.staging.as_ref().ok_or(Errno::ESTALE)?;
        if staging.bytes.len() != submit.candidate_bytes as usize {
            return Err(Errno::EINVAL);
        }
        let record = decode_shell_file_record(&staging.bytes, ShellFileClass::Candidate)
            .map_err(|_| Errno::EINVAL)?;
        if record.header.connection_epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        if record.header.submission_id != submit.submission_id {
            return Err(Errno::EINVAL);
        }
        let kind = record.header.kind;
        let inbound = self.decode(&staging.bytes, kind)?;
        let body = encode_shell_file_submitted_body(ShellFileSubmitted {
            submission_id: submit.submission_id,
            candidate_kind: kind,
        })
        .map_err(|_| Errno::EINVAL)?;
        // Every fallible check precedes custody transfer, including journal
        // capacity. Custody records never use the terminal reserve.
        let sequence = self
            .journal
            .append(ShellFileKind::Submitted, &body, false)?;
        let staging = self.staging.take().expect("candidate checked");
        self.accepted = Some(Accepted {
            submit,
            bytes: staging.bytes,
            handle: staging.handle,
            sequence,
        });
        self.submission_watermark = submit.submission_id;
        if kind == ShellFileKind::Negotiate {
            self.negotiate_accepted = true;
        }
        self.inbound.push_back(inbound);
        Ok(())
    }
}

impl Export for ShellFiles {
    type Node = Node;
    type Handle = Handle;

    fn attach(&mut self, context: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        // One attach per admitted epoch; a replacement needs a fresh stream.
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
            WalkName::Child(name) => ROOT_ENTRIES
                .iter()
                .find(|(entry, _)| *entry == name)
                .map(|&(_, node)| node)
                .ok_or(Errno::ENOENT),
        }
    }

    fn describe(&self, node: &Node, handle: Option<&Handle>) -> Entry {
        let mut qid_path = self.qid_base + *node as u64;
        match handle {
            Some(Handle::Transaction(id)) => qid_path = *id,
            Some(Handle::Object(object)) => qid_path = object.qid,
            _ => {
                if *node == Node::Outputs
                    && let Some(object) = &self.outputs
                {
                    qid_path = object.qid;
                }
            }
        }
        let size = match (node, handle) {
            (Node::Api, _) => self.api.len() as u64,
            (Node::Limits, _) => self.limits.as_ref().map_or(0, |l| l.len() as u64),
            (Node::Outputs, Some(Handle::Object(object))) => object.bytes.len() as u64,
            (Node::Outputs, _) => self.outputs.as_ref().map_or(0, |o| o.bytes.len() as u64),
            (Node::Events, _) => self.journal.size(),
            (Node::Transaction, Some(Handle::Transaction(id))) => self
                .staging
                .as_ref()
                .filter(|s| s.handle == *id)
                .map(|s| s.bytes.len() as u64)
                .or_else(|| {
                    self.accepted
                        .as_ref()
                        .filter(|a| a.handle == *id)
                        .map(|a| a.bytes.len() as u64)
                })
                .unwrap_or(0),
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
            Node::Limits if self.limits.is_none() => Err(Errno::EAGAIN),
            Node::Outputs => {
                // One pin per feed per attach; publication continues meanwhile.
                if self.outputs_pinned {
                    return Err(EBUSY);
                }
                let object = self.outputs.clone().ok_or(Errno::EAGAIN)?;
                self.outputs_pinned = true;
                Ok(Handle::Object(object))
            }
            Node::Transaction => {
                self.expire();
                if self.staging.is_some() || self.accepted.is_some() {
                    return Err(EBUSY);
                }
                let id = self.allocate_qid()?;
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
            (Node::Api, _) => Ok(slice(&self.api, offset, count)),
            (Node::Limits, _) => self
                .limits
                .as_ref()
                .map(|limits| slice(limits, offset, count))
                .ok_or(Errno::EAGAIN),
            (Node::Outputs, Handle::Object(object)) => Ok(slice(&object.bytes, offset, count)),
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
                            .filter(|a| a.handle == *id)
                            .map(|a| &a.bytes)
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
                let ack = decode_shell_file_ack(data).map_err(|_| Errno::EINVAL)?;
                self.journal.ack(ack, Instant::now())?;
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

    /// Lists the fixed root through describe alone, with no open, qid
    /// allocation, journal read or acknowledgement.
    fn readdir(
        &mut self,
        directory: &Node,
        _handle: &mut Handle,
        cookie: u64,
        max_entries: usize,
    ) -> Result<Vec<DirEntry>, Errno> {
        if *directory != Node::Root {
            return Err(Errno::ENOTDIR);
        }
        let start = usize::try_from(cookie)
            .unwrap_or(usize::MAX)
            .min(ROOT_ENTRIES.len());
        Ok(ROOT_ENTRIES[start..]
            .iter()
            .take(max_entries)
            .zip(start as u64 + 1..)
            .map(|(&(name, node), next)| DirEntry {
                name: name.to_vec(),
                entry: self.describe(&node, None),
                next,
            })
            .collect())
    }

    fn release(&mut self, _node: Node, handle: Option<Handle>) {
        match handle {
            Some(Handle::Transaction(id))
                if self.staging.as_ref().is_some_and(|s| s.handle == id) =>
            {
                self.staging = None
            }
            Some(Handle::Object(_)) => self.outputs_pinned = false,
            _ => {}
        }
    }
}
