//! One component's `sophia_shell_fs_v1` export. It owns file custody only:
//! the journal, the attach's candidate buffer and the typed inbound queue.
//! Admission, negotiation and every content decision stay with the existing
//! shell owners, which the transport feeds from `take_inbound`.
use std::collections::VecDeque;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use sophia_9p::connection::ConnectionId;
use sophia_9p::{
    Access, AttachContext, Attachment, DirEntry, Entry, Epoch, Errno, Export, NodeKind, OpenAccess,
    OpenFlags, ReadOutcome, WalkName,
};
use sophia_protocol::shell_files::*;
use sophia_protocol::{
    CatalogActivation, ContentGrant, ContentLimits, ContentResourceId, NativeLauncherActivation,
    NativeLauncherAllocationRequest, NativeLauncherInputAck, ShellCatalogActionRecord,
    ShellContentRecord, ShellIndicatorActivation, ShellNativeLauncherRecord, ShellV1ClientHello,
    TransactionId,
};

use super::journal::{Journal, JournalBounds};
use sophia_9p::journal::{Staging, StagingBounds};

mod authentication;
mod image;
pub(super) mod image_offers;
mod image_topology;
use image::{ImageChannelState, PendingImageRefusal, ShellImageReadiness};
pub(in crate::shell_transport) use image::{
    ShellImageChannelConfig, ShellImageReadiness as ImageReadiness,
};
mod inbound;
mod objects;
mod snapshots;
mod submission;
mod transaction;

#[cfg(test)]
#[path = "../../../tests/support/shell_image_inbound.rs"]
mod image_inbound_tests;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::shell_transport) enum Node {
    Root,
    Api,
    Limits,
    Outputs,
    Events,
    Transaction,
    TransactionReuse,
    Submit,
    Ack,
    /// The launcher/dock application catalog, plain or with r8 identities.
    Catalog,
    /// The bar's indicator snapshot, disclosed only with bit 9 negotiated.
    Indicators,
    Descriptors,
    Tabs,
    Shortcuts,
    ImageChannel,
    ImageLimits,
    ImageFormats,
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
            Self::Catalog => 13,
            Self::Indicators => 14,
            Self::Descriptors => 15,
            Self::Tabs => 16,
            Self::Shortcuts => 17,
            Self::TransactionReuse => 18,
            Self::ImageChannel => 19,
            Self::ImageLimits => 20,
            Self::ImageFormats => 21,
        }
    }

    /// The object kind this node names, for the pinned snapshot feeds.
    fn object_kind(self) -> Option<ShellFileKind> {
        match self {
            Self::Outputs => Some(ShellFileKind::Outputs),
            Self::Catalog => Some(ShellFileKind::Catalog),
            Self::Indicators => Some(ShellFileKind::Indicators),
            Self::Descriptors => Some(ShellFileKind::Descriptors),
            Self::Tabs => Some(ShellFileKind::Tabs),
            Self::Shortcuts => Some(ShellFileKind::Shortcuts),
            Self::ImageLimits => Some(ShellFileKind::ImageLimits),
            Self::ImageFormats => Some(ShellFileKind::ImageFormats),
            _ => None,
        }
    }
}

/// Logical qids reserved per epoch for the fixed nodes (root .. shortcuts).
const NODE_QIDS: u64 = 32;

pub(in crate::shell_transport) enum Handle {
    Plain,
    Transaction(u64),
    ReusableTransaction(u64),
    /// A pinned snapshot object: immutable, whatever is published later.
    Object(Arc<Object>),
    ImageDiscovery(Arc<image_topology::DiscoverySnapshot>),
}

/// One published snapshot object. A new publication never edits it; it
/// lives while it is current or pinned by an open fid.
pub(in crate::shell_transport) struct Object {
    kind: ShellFileKind,
    qid: u64,
    bytes: Vec<u8>,
}

/// One feed's current object and whether an open fid pins it. Publication
/// continues while a pin is held; the pinned bytes never change underneath it.
#[derive(Default)]
struct ObjectSlot {
    current: Option<Arc<Object>>,
    // Observation only: the open fid, not this weak reference, owns the pin.
    pinned: Option<Weak<Object>>,
}

/// The family of a queued whole candidate, before it is exploded into parts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::shell_transport) enum CandidateFamily {
    Base,
    Native,
    Catalog,
}

pub(in crate::shell_transport) use super::super::wire::{
    CatalogCandidatePart, NativeCandidatePart,
};

/// A submission whose custody the export has taken; the owners decide it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::shell_transport) enum Inbound {
    Negotiate(ShellV1ClientHello),
    ImageBegin(ShellImageMetadata),
    Content(TransactionId, Box<ShellContentRecord>),
    /// One whole candidate; its parts reach the owner in wire order.
    Candidate(Box<ShellFileCandidate>),
    NativeAllocation(TransactionId, Box<NativeLauncherAllocationRequest>),
    /// One whole native-launcher candidate; its parts reach the owner in
    /// wire order through `take_native_candidate_part`.
    NativeCandidate(Box<ShellFileNativeCandidate>),
    NativeInputAck(TransactionId, Box<NativeLauncherInputAck>),
    NativeActivate(TransactionId, Box<NativeLauncherActivation>),
    /// One whole catalog candidate; its parts reach the owner in wire order
    /// through `take_catalog_candidate_part`.
    CatalogCandidate(Box<ShellFileCatalogCandidate>),
    CatalogActivate(TransactionId, Box<CatalogActivation>),
    IndicatorActivate(TransactionId, Box<ShellIndicatorActivation>),
    /// Native whole descriptor-family candidate or acknowledgement. File
    /// custody does not decide presentation, work area or activation.
    Descriptor(Box<ShellFileDescriptorRecord>),
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
    /// The role profile discloses `catalog`, decided before the peer
    /// connects (launcher and dock; never the bar's Legacy profile).
    catalog_allowed: bool,
    /// Selected by the server before accepting the peer, not by bit 0.
    descriptor: bool,
    /// The negotiated welcome's capability set, once negotiation completed.
    capabilities: u64,
    qid_base: u64,
    next_qid: u64,
    limits: Option<Vec<u8>>,
    content_limits: Option<ContentLimits>,
    image_limits: ObjectSlot,
    image_formats: ObjectSlot,
    outputs: ObjectSlot,
    catalog: ObjectSlot,
    indicators: ObjectSlot,
    descriptors: ObjectSlot,
    tabs: ObjectSlot,
    shortcuts: ObjectSlot,
    /// Two encoded copies per disclosed feed and the shared build scratch.
    /// Fixed at negotiation, independently of the content resource grant.
    snapshot_reserved_bytes: usize,
    staging: Option<Staging>,
    reuse_writer: Option<transaction::ReusableWriter>,
    accepted: Option<Accepted>,
    image: Option<ImageChannelState>,
    image_revision: u16,
    pending_image_refusal: Option<PendingImageRefusal>,
    pending_image_closed: Option<ShellChannelClosed>,
    submission_watermark: u64,
    journal: Journal,
    inbound: VecDeque<Inbound>,
    /// The Begin, Chunk and End of the candidate being handed to the owner.
    candidate_parts: VecDeque<(TransactionId, ShellContentRecord)>,
    /// As `candidate_parts`, for a whole `NativeCandidate` under assembly.
    native_candidate_parts: VecDeque<(TransactionId, NativeCandidatePart)>,
    /// As `candidate_parts`, for a whole `CatalogCandidate` under assembly.
    catalog_candidate_parts: VecDeque<(TransactionId, CatalogCandidatePart)>,
}

fn slice(bytes: &[u8], offset: u64, count: u32) -> ReadOutcome {
    let start = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    let end = start.saturating_add(count as usize).min(bytes.len());
    ReadOutcome::Ready(bytes[start..end].to_vec())
}

impl ShellFiles {
    pub(in crate::shell_transport) fn is_descriptor(&self) -> bool {
        self.descriptor && self.negotiated
    }
    /// An export awaiting exactly one Negotiate record for `epoch`. Qids are
    /// logical per component and continue from `qid_base` across epochs.
    /// `catalog_allowed` is fixed by the role profile Session selected before
    /// the peer connected (launcher, dock), independent of negotiation.
    pub(in crate::shell_transport) fn awaiting_negotiation(
        epoch: u64,
        role: &str,
        catalog_allowed: bool,
        bounds: JournalBounds,
        qid_base: u64,
        now: Instant,
    ) -> Self {
        Self {
            epoch,
            // The attach's epoch is disclosed here, so a client learns it in
            // band before writing its first record header.
            api: format!(
                "sophia-shell-files version={SHELL_FILE_API_VERSION} role={role} epoch={epoch} fd_transfer=none\n"
            )
            .into_bytes(),
            connection: None,
            attached: false,
            revoked: false,
            negotiate_accepted: false,
            negotiated: false,
            content: false,
            catalog_allowed,
            descriptor: role == "descriptor",
            capabilities: 0,
            qid_base,
            next_qid: qid_base + NODE_QIDS,
            limits: None,
            content_limits: None,
            image_limits: ObjectSlot::default(),
            image_formats: ObjectSlot::default(),
            outputs: ObjectSlot::default(),
            catalog: ObjectSlot::default(),
            indicators: ObjectSlot::default(),
            descriptors: ObjectSlot::default(),
            tabs: ObjectSlot::default(),
            shortcuts: ObjectSlot::default(),
            snapshot_reserved_bytes: 0,
            staging: None,
            reuse_writer: None,
            accepted: None,
            image: None,
            image_revision: 0,
            pending_image_refusal: None,
            pending_image_closed: None,
            submission_watermark: 0,
            journal: Journal::new(epoch, bounds, now),
            inbound: VecDeque::with_capacity(INBOUND_RECORDS),
            candidate_parts: VecDeque::with_capacity(3),
            native_candidate_parts: VecDeque::with_capacity(3),
            catalog_candidate_parts: VecDeque::with_capacity(3),
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
        self.reuse_writer = None;
        self.accepted = None;
        self.inbound.clear();
    }

    /// Records the owners' selection: the immutable limits object, if the
    /// profile has one, whether content records may be submitted, and the
    /// negotiated capability set that gates the native, catalog and
    /// indicator families and the `indicators` root name.
    pub(in crate::shell_transport) fn complete_negotiation(
        &mut self,
        limits: Option<(Vec<u8>, ContentLimits)>,
        capabilities: u64,
    ) -> Result<(), Errno> {
        self.complete_negotiation_with_image_revision(limits, capabilities, 0)
    }

    /// Negotiation completion with the selected image revision hooked in
    /// atomically: disclosure, readiness and candidate gating all read
    /// this one recorded value, never a later unchecked setter.
    pub(in crate::shell_transport) fn complete_negotiation_with_image_revision(
        &mut self,
        limits: Option<(Vec<u8>, ContentLimits)>,
        capabilities: u64,
        image_revision: u16,
    ) -> Result<(), Errno> {
        if self.revoked {
            return Err(Errno::ESTALE);
        }
        if self.negotiated {
            return Err(EALREADY);
        }
        let image_objects = if self.image.is_some()
            && image_revision == 9
            && capabilities & ((1 << 13) | (1 << 14)) != 0
        {
            let (_, content_limits) = limits.as_ref().ok_or(Errno::EACCES)?;
            self.validate_image_content_limits(content_limits)?;
            self.prepare_image_objects()?
        } else {
            Vec::new()
        };
        self.content = limits.is_some();
        if let Some((bytes, limits)) = limits {
            self.limits = Some(bytes);
            self.content_limits = Some(limits);
        }
        self.capabilities = capabilities;
        self.image_revision = image_revision;
        if image_revision == 9 {
            self.journal.enable_image_revision();
        }
        self.negotiated = true;
        self.install_image_objects(image_objects);
        self.snapshot_reserved_bytes = self.selected_snapshot_bound();
        Ok(())
    }

    /// Journals one event, preserving image-offer status accounting.
    pub(in crate::shell_transport) fn append_event(
        &mut self,
        kind: ShellFileKind,
        body: &[u8],
        credited: bool,
    ) -> Result<u64, Errno> {
        if shell_file_descriptor_max_bytes(kind).is_some() {
            if !self.supports_descriptor_kind(kind)
                || shell_file_class(kind) != ShellFileClass::Event
            {
                return Err(Errno::EACCES);
            }
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
            decode_shell_file_descriptor(&record, kind).map_err(|_| Errno::EINVAL)?;
        }
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
            let decoded = decode_shell_file_resource_status(&record).map_err(|_| Errno::EINVAL)?;
            match decoded.record {
                ShellContentRecord::ResourceStatus(status) => Some((decoded.transaction, status)),
                _ => return Err(Errno::EINVAL),
            }
        } else {
            None
        };
        let sequence = if let Some((transaction, status)) = &status {
            match self.append_image_status(*transaction, status, body)? {
                Some(sequence) => sequence,
                None => self.journal.append(kind, body, credited)?,
            }
        } else {
            self.journal.append(kind, body, credited)?
        };
        #[cfg(feature = "comparator-journal-trace")]
        self.trace_outcome(
            sequence,
            kind,
            body,
            status.as_ref().map(|(transaction, status)| {
                (
                    transaction.raw(),
                    trace::trace_key("resource", status.resource.id, status.resource.generation),
                    status.status,
                )
            }),
        );
        Ok(sequence)
    }

    pub(in crate::shell_transport) fn journal(&self) -> &Journal {
        &self.journal
    }

    pub(in crate::shell_transport) fn expire(&mut self) {
        if self
            .staging
            .as_ref()
            .is_some_and(|s| s.expired(Instant::now()))
        {
            self.staging = None;
            self.reuse_writer = None;
        }
    }

    fn allocate_qid(&mut self) -> Result<u64, Errno> {
        let qid = self.next_qid;
        self.next_qid = qid.checked_add(1).ok_or(Errno::ENOSPC)?;
        Ok(qid)
    }
}

#[cfg(test)]
#[path = "../../../tests/support/shell_file_descriptor_export.rs"]
mod tests;

impl Export for ShellFiles {
    const READS_BUFFERED: bool = true;
    type Node = Node;
    type Handle = Handle;

    fn attach(&mut self, context: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        use crate::authenticated_files::{PreparedFileRoot, ProtectedFiles};
        Ok(self.prepare_authenticated_root(context)?.publish())
    }

    fn check(&mut self, access: &Access<'_, Node>) -> Result<(), Errno> {
        self.expire();
        if self.revoked
            || !self.attached
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
            WalkName::Child(name) => self
                .root_entries()
                .into_iter()
                .find(|(entry, _)| *entry == name)
                .map(|(_, node)| node)
                .ok_or(Errno::ENOENT),
        }
    }

    fn describe(&self, node: &Node, handle: Option<&Handle>) -> Entry {
        let mut qid_path = self.qid_base + node.index();
        match handle {
            Some(Handle::Transaction(id) | Handle::ReusableTransaction(id)) => qid_path = *id,
            Some(Handle::Object(object)) => qid_path = object.qid,
            Some(Handle::ImageDiscovery(discovery)) => qid_path = discovery.qid,
            _ => {
                if *node == Node::ImageChannel
                    && let Some(state) = &self.image
                {
                    qid_path = state.discovery.qid;
                }
                if let Some(kind) = node.object_kind()
                    && let Some(object) = self
                        .object_slot(kind)
                        .and_then(|slot| slot.current.as_ref())
                {
                    qid_path = object.qid;
                }
            }
        }
        let size = match (node, handle) {
            (Node::Api, _) => self.api.len() as u64,
            (Node::ImageChannel, Some(Handle::ImageDiscovery(discovery))) => {
                discovery.text.len() as u64
            }
            (Node::ImageChannel, _) => self
                .image
                .as_ref()
                .map_or(0, |state| state.discovery.text.len() as u64),
            (Node::Limits, _) => self.limits.as_ref().map_or(0, |l| l.len() as u64),
            (node, Some(Handle::Object(object))) if node.object_kind().is_some() => {
                object.bytes.len() as u64
            }
            (node, _) if node.object_kind().is_some() => node
                .object_kind()
                .and_then(|kind| self.object_slot(kind))
                .and_then(|slot| slot.current.as_ref())
                .map_or(0, |o| o.bytes.len() as u64),
            (Node::Events, _) => self.journal.size(),
            (Node::Transaction, Some(Handle::Transaction(id)))
            | (Node::TransactionReuse, Some(Handle::ReusableTransaction(id))) => self
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
            kind: if matches!(node, Node::Root) {
                NodeKind::Directory
            } else {
                NodeKind::File
            },
            qid_path,
            qid_version: 0,
            permissions: match node {
                Node::Root => 0o500,
                Node::Transaction | Node::TransactionReuse => 0o600,
                Node::Submit | Node::Ack => 0o200,
                _ => 0o400,
            },
            size,
        }
    }

    fn open(&mut self, node: &Node, flags: OpenFlags) -> Result<Handle, Errno> {
        let access = flags.access().ok_or(Errno::EINVAL)?;
        let valid = match node {
            Node::Transaction | Node::TransactionReuse => access == OpenAccess::ReadWrite,
            Node::Submit | Node::Ack => access == OpenAccess::Write,
            _ => access == OpenAccess::Read,
        };
        if !valid || flags.truncate() || flags.append() {
            return Err(Errno::EACCES);
        }
        match node {
            Node::Limits if self.limits.is_none() => Err(Errno::EAGAIN),
            Node::ImageChannel if self.image_disclosed() => Ok(Handle::ImageDiscovery(
                self.image
                    .as_ref()
                    .expect("disclosed image")
                    .discovery
                    .clone(),
            )),
            Node::ImageChannel => Err(Errno::ENOENT),
            node if node.object_kind().is_some() => {
                let kind = node.object_kind().expect("object node");
                let slot = self.object_slot_mut(kind).expect("valid object kind");
                // One pin per feed per attach; publication continues meanwhile.
                if slot.pinned.is_some() {
                    return Err(EBUSY);
                }
                let object = slot.current.clone().ok_or(Errno::EAGAIN)?;
                slot.pinned = Some(Arc::downgrade(&object));
                Ok(Handle::Object(object))
            }
            Node::Transaction => {
                self.expire();
                if self.staging.is_some() || self.accepted.is_some() || self.reuse_writer.is_some()
                {
                    return Err(EBUSY);
                }
                let id = self.allocate_qid()?;
                self.staging = Some(Staging::new(id, STAGING));
                Ok(Handle::Transaction(id))
            }
            Node::TransactionReuse => self.open_reusable_transaction(),
            _ => Ok(Handle::Plain),
        }
    }

    fn read_buffered(
        &mut self,
        node: &Node,
        handle: &mut Handle,
        offset: u64,
        count: u32,
        scratch: &mut Vec<u8>,
    ) -> Result<ReadOutcome, Errno> {
        if matches!(node, Node::Events) {
            self.journal.read_buffered(offset, count, scratch)
        } else {
            self.read(node, handle, offset, count)
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
            (Node::ImageChannel, Handle::ImageDiscovery(discovery)) => {
                Ok(slice(discovery.text.as_bytes(), offset, count))
            }
            (Node::Limits, _) => self
                .limits
                .as_ref()
                .map(|limits| slice(limits, offset, count))
                .ok_or(Errno::EAGAIN),
            (node, Handle::Object(object)) if node.object_kind().is_some() => {
                Ok(slice(&object.bytes, offset, count))
            }
            (Node::Events, _) => self.journal.read(offset, count),
            (Node::TransactionReuse, Handle::ReusableTransaction(id)) => {
                self.read_reusable_transaction(*id, offset, count)
            }
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
        #[cfg(feature = "comparator-progress")]
        {
            use crate::comparator_counters::progress::{Activity, observe};
            observe(match node {
                Node::Transaction | Node::TransactionReuse => Activity::Record,
                Node::Submit => Activity::Submit,
                Node::Upload(_) => Activity::Pixel,
                _ => Activity::Other,
            });
        }
        match (node, handle) {
            (Node::TransactionReuse, Handle::ReusableTransaction(id)) => {
                self.write_reusable_transaction(*id, offset, data)
            }
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
                // A pending image refusal retries from ACK progress; its
                // outcome never fails the acknowledgement itself.
                let _ = self.retry_pending_image_refusal();
                if self
                    .accepted
                    .as_ref()
                    .is_some_and(|a| a.sequence <= ack.sequence)
                {
                    #[cfg(feature = "comparator-journal-trace")]
                    self.trace_released(ack.sequence);
                    self.recycle_transaction();
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
        let entries = self.root_entries();
        let start = usize::try_from(cookie)
            .unwrap_or(usize::MAX)
            .min(entries.len());
        Ok(entries[start..]
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
            Some(Handle::ReusableTransaction(id)) => self.release_reusable_transaction(id),
            Some(Handle::Transaction(id))
                if self.staging.as_ref().is_some_and(|s| s.handle == id) =>
            {
                self.staging = None
            }
            Some(Handle::Object(object)) => {
                if let Some(slot) = self.object_slot_mut(object.kind) {
                    slot.pinned = None;
                }
            }
            _ => {}
        }
    }
}

#[cfg(feature = "comparator-journal-trace")]
#[path = "export/trace.rs"]
mod trace;
