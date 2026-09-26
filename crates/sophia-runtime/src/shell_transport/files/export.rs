//! One component's `sophia_shell_fs_v1` export. It owns file custody only:
//! the journal, the attach's candidate buffer and the typed inbound queue.
//! Admission, negotiation and every content decision stay with the existing
//! shell owners, which the transport feeds from `take_inbound`.
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sophia_9p::connection::ConnectionId;
use sophia_9p::{
    Access, AttachContext, Attachment, DirEntry, Entry, Epoch, Errno, Export, NodeKind, OpenAccess,
    OpenFlags, ReadOutcome, WalkName,
};
use sophia_protocol::shell_files::*;
use sophia_protocol::{
    ContentGrant, ContentLimits, ContentResourceCancel, ContentResourceChunk, ContentResourceId,
    ContentResourceLayout, ShellContentRecord, ShellV1ClientHello, TransactionId,
};

use super::journal::{Journal, JournalBounds};
use sophia_9p::journal::{Staging, StagingBounds};

mod submission;

/// Not in `sophia-9p`'s set; the WM file owner defines the same values.
const EBUSY: Errno = Errno(16);
const EALREADY: Errno = Errno(114);

/// One candidate record under assembly: at most one transaction, completed
/// within the WM file assembly deadline.
const STAGING: StagingBounds = StagingBounds {
    header_bytes: SHELL_FILE_HEADER_BYTES,
    max_bytes: SHELL_FILE_MAX_TRANSACTION_BYTES,
    assembly: Duration::from_millis(SHELL_FILE_ASSEMBLY_TIMEOUT_MILLIS as u64),
};

/// At most this many accepted submissions wait for the owners, as the
/// socket transport's inbox does.
pub(in crate::shell_transport) const INBOUND_RECORDS: usize = 64;

const ROOT_ENTRIES: [(&[u8], Node); 8] = [
    (b"api", Node::Api),
    (b"limits", Node::Limits),
    (b"outputs", Node::Outputs),
    (b"events", Node::Events),
    (b"transaction", Node::Transaction),
    (b"submit", Node::Submit),
    (b"ack", Node::Ack),
    (b"upload", Node::Uploads),
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
    /// The fixed `upload` directory of transfer slots.
    Uploads,
    Upload(u8),
}

impl Node {
    /// A stable logical offset for this node's qid within the component.
    fn index(self) -> u64 {
        match self {
            Self::Root => 0,
            Self::Api => 1,
            Self::Limits => 2,
            Self::Outputs => 3,
            Self::Events => 4,
            Self::Transaction => 5,
            Self::Submit => 6,
            Self::Ack => 7,
            Self::Uploads => 8,
            Self::Upload(slot) => 9 + u64::from(slot),
        }
    }
}

/// Logical qids reserved per epoch for the fixed nodes (root .. upload/3).
const NODE_QIDS: u64 = 16;

pub(in crate::shell_transport) enum Handle {
    Plain,
    Transaction(u64),
    /// A pinned snapshot object: immutable, whatever is published later.
    Object(Arc<Object>),
    /// A writer fid on one binding of an upload slot. Its `binding` qid fences
    /// it from any later binding of the same slot.
    Upload {
        binding: u64,
        writer: u64,
    },
}

/// One slot's binding to exactly (grant, resource). Pending from the accepted
/// Begin until the store reports the transfer admitted; it ends at a terminal
/// status. Bytes pass to the store only as canonical chunks.
struct Binding {
    qid: u64,
    transaction: TransactionId,
    grant: ContentGrant,
    resource: ContentResourceId,
    layout: ContentResourceLayout,
    admitted: bool,
    /// End or Cancel was submitted: no further writes, and a clunk no longer
    /// cancels (the store answers the terminal status itself).
    closing: bool,
    writer: Option<u64>,
    passed: u64,
    ordinal: u32,
    scratch: Vec<u8>,
}

impl Binding {
    fn cursor(&self) -> u64 {
        self.passed + self.scratch.len() as u64
    }

    /// The canonical size of the chunk now being assembled.
    fn chunk_len(&self) -> usize {
        let full = u64::from(self.layout.row_bytes) * u64::from(self.layout.rows_per_chunk);
        (self.layout.total_bytes - self.passed).min(full) as usize
    }
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
    /// One whole candidate; its parts reach the owner in wire order.
    Candidate(Box<ShellFileCandidate>),
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
    content_limits: Option<ContentLimits>,
    upload_slots: u8,
    uploads: [Option<Binding>; SHELL_FILE_MAX_UPLOAD_SLOTS as usize],
    outputs: Option<Arc<Object>>,
    outputs_pinned: bool,
    staging: Option<Staging>,
    accepted: Option<Accepted>,
    submission_watermark: u64,
    journal: Journal,
    inbound: VecDeque<Inbound>,
    /// The Begin, Chunk and End of the candidate being handed to the owner.
    candidate_parts: VecDeque<(TransactionId, ShellContentRecord)>,
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
            next_qid: qid_base + NODE_QIDS,
            limits: None,
            content_limits: None,
            upload_slots: 0,
            uploads: Default::default(),
            outputs: None,
            outputs_pinned: false,
            staging: None,
            accepted: None,
            submission_watermark: 0,
            journal: Journal::new(epoch, bounds, now),
            inbound: VecDeque::with_capacity(INBOUND_RECORDS),
            candidate_parts: VecDeque::with_capacity(3),
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
        limits: Option<(Vec<u8>, ContentLimits)>,
    ) -> Result<(), Errno> {
        if self.revoked {
            return Err(Errno::ESTALE);
        }
        if self.negotiated {
            return Err(EALREADY);
        }
        self.content = limits.is_some();
        if let Some((bytes, limits)) = limits {
            self.upload_slots = limits
                .max_open_transfers
                .min(u32::from(SHELL_FILE_MAX_UPLOAD_SLOTS)) as u8;
            self.limits = Some(bytes);
            self.content_limits = Some(limits);
        }
        self.negotiated = true;
        Ok(())
    }

    /// Journals one event and applies what it means for the upload slots: a
    /// resource status binds a pending slot (admitted) or ends its binding
    /// (accepted, rejected, cancelled). Nothing changes on refusal.
    pub(in crate::shell_transport) fn append_event(
        &mut self,
        kind: ShellFileKind,
        body: &[u8],
        credited: bool,
    ) -> Result<u64, Errno> {
        let status = if kind == ShellFileKind::ResourceStatus {
            let record = encode_shell_file_record(
                ShellFileHeader {
                    kind,
                    connection_epoch: self.epoch,
                    submission_id: 0,
                    sequence: 1,
                },
                body,
            )
            .map_err(|_| Errno::EINVAL)?;
            match decode_shell_file_resource_status(&record)
                .map_err(|_| Errno::EINVAL)?
                .record
            {
                ShellContentRecord::ResourceStatus(status) => Some(status),
                _ => return Err(Errno::EINVAL),
            }
        } else {
            None
        };
        let sequence = self.journal.append(kind, body, credited)?;
        if let Some(status) = status {
            self.observe_resource_status(status.resource, status.status);
        }
        Ok(sequence)
    }

    fn observe_resource_status(&mut self, resource: ContentResourceId, status: u16) {
        for slot in self.uploads.iter_mut() {
            if slot.as_ref().is_some_and(|b| b.resource == resource) {
                if status == 1 {
                    slot.as_mut().expect("bound slot").admitted = true;
                } else {
                    // A terminal status fences every fid of this binding.
                    *slot = None;
                }
            }
        }
    }

    fn binding_of(&self, slot: u8, qid: u64) -> Result<&Binding, Errno> {
        self.uploads[usize::from(slot)]
            .as_ref()
            .filter(|binding| binding.qid == qid)
            .ok_or(Errno::ESTALE)
    }

    /// Appends at the binding's exact cursor, accepting at most the prefix
    /// that completes the current canonical chunk. A completed chunk is queued
    /// for the owners once; incomplete bytes stay in the slot's one buffer.
    fn write_upload(&mut self, slot: u8, qid: u64, offset: u64, data: &[u8]) -> Result<u32, Errno> {
        let inbound_full = self.inbound.len() >= INBOUND_RECORDS;
        let binding = self.uploads[usize::from(slot)]
            .as_mut()
            .filter(|binding| binding.qid == qid)
            .ok_or(Errno::ESTALE)?;
        if binding.closing || !binding.admitted {
            return Err(Errno::ESTALE);
        }
        let end = offset.checked_add(data.len() as u64).ok_or(Errno::EINVAL)?;
        if offset != binding.cursor() || end > binding.layout.total_bytes {
            return Err(Errno::EINVAL);
        }
        if data.is_empty() {
            return Ok(0);
        }
        let chunk = binding.chunk_len();
        let take = data.len().min(chunk - binding.scratch.len());
        let completes = binding.scratch.len() + take == chunk;
        if completes && inbound_full {
            return Err(Errno::EAGAIN);
        }
        binding.scratch.extend_from_slice(&data[..take]);
        if completes {
            let bytes = std::mem::replace(&mut binding.scratch, Vec::with_capacity(chunk));
            let record = ShellContentRecord::ResourceChunk(ContentResourceChunk {
                grant: binding.grant,
                resource: binding.resource,
                ordinal: binding.ordinal,
                offset: binding.passed,
                bytes,
            });
            binding.passed += chunk as u64;
            binding.ordinal += 1;
            let transaction = binding.transaction;
            self.inbound
                .push_back(Inbound::Content(transaction, Box::new(record)));
        }
        Ok(take as u32)
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

    /// The first queued content record the predicate selects, left queued.
    pub(in crate::shell_transport) fn peek_content(
        &self,
        select: impl Fn(&ShellContentRecord) -> bool,
    ) -> Option<&ShellContentRecord> {
        self.inbound.iter().find_map(|item| match item {
            Inbound::Content(_, record) if select(record) => Some(record.as_ref()),
            _ => None,
        })
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

    /// The next candidate part: the rest of the candidate already begun, or
    /// the first part of the oldest queued candidate. Other queued records
    /// keep their order.
    pub(in crate::shell_transport) fn take_candidate_part(
        &mut self,
    ) -> Option<(TransactionId, ShellContentRecord)> {
        if self.candidate_parts.is_empty() {
            let at = self
                .inbound
                .iter()
                .position(|item| matches!(item, Inbound::Candidate(_)))?;
            let Some(Inbound::Candidate(candidate)) = self.inbound.remove(at) else {
                return None;
            };
            let transaction = candidate.transaction;
            self.candidate_parts.extend(
                candidate
                    .records()
                    .into_iter()
                    .map(|record| (transaction, record)),
            );
        }
        self.candidate_parts.pop_front()
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

    /// Only the current binding's writer cancels by clunking, and only before
    /// End or Cancel was submitted. A fenced fid's clunk releases the fid.
    fn release_writer(&mut self, qid: u64, writer: u64) {
        let Some(slot) = self.uploads.iter_mut().find(|slot| {
            slot.as_ref()
                .is_some_and(|b| b.qid == qid && b.writer == Some(writer))
        }) else {
            return;
        };
        let binding = slot.take().expect("writer's binding");
        if binding.closing || self.revoked {
            *slot = Some(Binding {
                writer: None,
                ..binding
            });
            return;
        }
        self.inbound.push_back(Inbound::Content(
            binding.transaction,
            Box::new(ShellContentRecord::ResourceCancel(ContentResourceCancel {
                grant: binding.grant,
                resource: binding.resource,
            })),
        ));
    }

    fn allocate_qid(&mut self) -> Result<u64, Errno> {
        let qid = self.next_qid;
        self.next_qid = qid.checked_add(1).ok_or(Errno::ENOSPC)?;
        Ok(qid)
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
        if *directory == Node::Uploads {
            return match name {
                WalkName::Parent => Ok(Node::Root),
                WalkName::Child(name) => std::str::from_utf8(name)
                    .ok()
                    .filter(|name| name.len() == 1)
                    .and_then(|name| name.parse::<u8>().ok())
                    .filter(|slot| *slot < self.upload_slots)
                    .map(Node::Upload)
                    .ok_or(Errno::ENOENT),
            };
        }
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
        let mut qid_path = self.qid_base + node.index();
        match handle {
            Some(Handle::Transaction(id)) => qid_path = *id,
            Some(Handle::Object(object)) => qid_path = object.qid,
            Some(Handle::Upload { binding, .. }) => qid_path = *binding,
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
            // The live binding's accepted append cursor; a fenced fid sees 0.
            (Node::Upload(slot), Some(Handle::Upload { binding, .. })) => {
                self.binding_of(*slot, *binding).map_or(0, Binding::cursor)
            }
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
            kind: if matches!(node, Node::Root | Node::Uploads) {
                NodeKind::Directory
            } else {
                NodeKind::File
            },
            qid_path,
            qid_version: 0,
            permissions: match node {
                Node::Root | Node::Uploads => 0o500,
                Node::Transaction => 0o600,
                Node::Submit | Node::Ack | Node::Upload(_) => 0o200,
                _ => 0o400,
            },
            size,
        }
    }

    fn open(&mut self, node: &Node, flags: OpenFlags) -> Result<Handle, Errno> {
        let access = flags.access().ok_or(Errno::EINVAL)?;
        let valid = match node {
            Node::Transaction => access == OpenAccess::ReadWrite,
            Node::Submit | Node::Ack | Node::Upload(_) => access == OpenAccess::Write,
            _ => access == OpenAccess::Read,
        };
        if !valid || flags.truncate() || flags.append() {
            return Err(Errno::EACCES);
        }
        match node {
            Node::Limits if self.limits.is_none() => Err(Errno::EAGAIN),
            Node::Upload(slot) => {
                let writer = self.next_qid;
                let binding = self.uploads[usize::from(*slot)]
                    .as_mut()
                    .filter(|binding| binding.admitted && !binding.closing)
                    .ok_or(Errno::EAGAIN)?;
                // The first writer of a binding is its only writer.
                if binding.writer.is_some() {
                    return Err(EBUSY);
                }
                binding.writer = Some(writer);
                let handle = Handle::Upload {
                    binding: binding.qid,
                    writer,
                };
                self.allocate_qid()?;
                Ok(handle)
            }
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
                self.staging = Some(Staging::new(id, STAGING));
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
            (Node::Upload(slot), Handle::Upload { binding, .. }) => {
                let (slot, binding) = (*slot, *binding);
                self.write_upload(slot, binding, offset, data)
            }
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
        if *directory == Node::Uploads {
            let start = cookie.min(u64::from(self.upload_slots));
            return Ok((start..u64::from(self.upload_slots))
                .take(max_entries)
                .map(|slot| DirEntry {
                    name: slot.to_string().into_bytes(),
                    entry: self.describe(&Node::Upload(slot as u8), None),
                    next: slot + 1,
                })
                .collect());
        }
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
            Some(Handle::Upload { binding, writer }) => self.release_writer(binding, writer),
            _ => {}
        }
    }
}
