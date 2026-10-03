//! The `sophia_lock_fs_v1` files of one admitted provider connection.
//! Socket admission stays with the endpoint; attach strings, paths, fids and
//! qids grant nothing. File custody follows the output and shell contracts:
//! one candidate staged in `transaction`, handed over by `submit`, events
//! read from `events` and released by `ack`.
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sophia_9p::connection::ConnectionId;
use sophia_9p::export::*;
use sophia_9p::journal::{Staging, StagingBounds};
use sophia_9p::records::{OpenAccess, OpenFlags};
use sophia_9p::{Errno, ReadOutcome};
use sophia_protocol::lock_files::*;

use super::{LockFileCustody, LockFileSettings, LockInbound};

const EBUSY: Errno = Errno(16);

/// Qids shared across a provider's connection epochs, so no published qid
/// ever names different bytes.
#[derive(Clone)]
pub struct LockFileQids(Arc<AtomicU64>);

impl Default for LockFileQids {
    fn default() -> Self {
        Self(Arc::new(AtomicU64::new(1)))
    }
}

impl LockFileQids {
    fn allocate(&self, count: u64) -> Result<u64, Errno> {
        self.0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(count)
            })
            .map_err(|_| Errno::ENOSPC)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockFileNode {
    Root,
    Api,
    Limits,
    Lock,
    Events,
    Transaction,
    Submit,
    Ack,
    Uploads,
    Upload(u8),
}

use LockFileNode as Node;

impl Node {
    fn index(self) -> u64 {
        match self {
            Self::Root => 0,
            Self::Api => 1,
            Self::Limits => 2,
            Self::Lock => 3,
            Self::Events => 4,
            Self::Transaction => 5,
            Self::Submit => 6,
            Self::Ack => 7,
            Self::Uploads => 8,
            Self::Upload(slot) => 9 + u64::from(slot),
        }
    }
}

const NODE_QIDS: u64 = 16;

const ENTRIES: [(&[u8], Node); 8] = [
    (b"api", Node::Api),
    (b"limits", Node::Limits),
    (b"lock", Node::Lock),
    (b"events", Node::Events),
    (b"transaction", Node::Transaction),
    (b"submit", Node::Submit),
    (b"ack", Node::Ack),
    (b"upload", Node::Uploads),
];

/// One published lock object. A publication never edits it; an open fid
/// keeps reading the generation it opened.
struct LockFile {
    qid: u64,
    bytes: Vec<u8>,
}

pub struct LockFileHandle(Handle);

enum Handle {
    Plain,
    Lock(Arc<LockFile>),
    Transaction(u64),
    Upload(u64),
}

pub struct LockFileExport {
    custody: LockFileCustody,
    qids: LockFileQids,
    base: u64,
    api: Vec<u8>,
    limits: Vec<u8>,
    lock: Arc<LockFile>,
    connection: Option<ConnectionId>,
    attached: bool,
    staging: Option<Staging>,
}

fn slice(bytes: &[u8], offset: u64, count: u32) -> Vec<u8> {
    let start = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    bytes[start..start.saturating_add(count as usize).min(bytes.len())].to_vec()
}

fn object(epoch: u64, kind: LockFileKind, body: &[u8]) -> Result<Vec<u8>, Errno> {
    encode_lock_file_record(
        LockFileHeader {
            kind,
            connection_epoch: epoch,
            submission_id: 0,
            sequence: 0,
        },
        body,
    )
    .map_err(|_| Errno::EINVAL)
}

impl LockFileExport {
    /// `settings.lock_qid` is ignored: the export names its own objects.
    pub fn new(mut settings: LockFileSettings, qids: LockFileQids) -> Result<Self, Errno> {
        let base = qids.allocate(NODE_QIDS)?;
        let lock_qid = qids.allocate(1)?;
        settings.lock_qid = lock_qid;
        let epoch = settings.epoch;
        let limits = object(
            epoch,
            LockFileKind::Limits,
            &settings.limits.encode().map_err(|_| Errno::EINVAL)?,
        )?;
        let lock = object(
            epoch,
            LockFileKind::Lock,
            &settings.lock.encode().map_err(|_| Errno::EINVAL)?,
        )?;
        Ok(Self {
            custody: LockFileCustody::new(settings)?,
            qids,
            base,
            api: format!("sophia-lock-files version={LOCK_FILE_API_VERSION} epoch={epoch}\n")
                .into_bytes(),
            limits,
            lock: Arc::new(LockFile {
                qid: lock_qid,
                bytes: lock,
            }),
            connection: None,
            attached: false,
            staging: None,
        })
    }

    /// Bind only the connection the endpoint admitted; never replaced.
    pub fn bind_connection(&mut self, connection: ConnectionId) -> Result<(), Errno> {
        if self.custody.is_revoked() || self.connection.is_some() {
            return Err(Errno::EACCES);
        }
        self.connection = Some(connection);
        Ok(())
    }

    pub fn custody(&self) -> &LockFileCustody {
        &self.custody
    }

    pub fn revoke(&mut self) {
        self.custody.revoke();
        self.staging = None;
    }

    pub fn is_revoked(&self) -> bool {
        self.custody.is_revoked()
    }

    /// The worker polls this even when the peer sends nothing.
    pub fn expire(&mut self, now: Instant) {
        self.custody.expire(now);
        if self.staging.as_ref().is_some_and(|s| s.expired(now)) {
            self.staging = None;
        }
        if self.custody.is_revoked() {
            self.staging = None;
        }
    }

    pub fn wait(&self, now: Instant, maximum: Duration) -> Duration {
        let wait = self.custody.wait(now, maximum);
        self.staging
            .as_ref()
            .map_or(wait, |staging| staging.wait(now, wait))
    }

    pub fn take_inbound(&mut self) -> Option<LockInbound> {
        self.custody.take_inbound()
    }

    /// Session's new lock object, under a fresh qid.
    pub fn publish_lock(&mut self, lock: LockObject) -> Result<u64, Errno> {
        let bytes = object(
            self.custody.epoch(),
            LockFileKind::Lock,
            &lock.encode().map_err(|_| Errno::EINVAL)?,
        )?;
        let qid = self.qids.allocate(1)?;
        let generation = self.custody.publish_lock(lock, qid)?;
        self.lock = Arc::new(LockFile { qid, bytes });
        Ok(generation)
    }

    pub fn entry(&mut self, entry: LockEntry) -> Result<u64, Errno> {
        self.custody.entry(entry)
    }

    pub fn chord(&mut self, chord: LockChord) -> Result<u64, Errno> {
        self.custody.chord(chord)
    }

    pub fn permit(
        &mut self,
        allocation_id: u64,
        demand_id: u64,
        expires_after: Duration,
    ) -> Result<LockFramePermit, Errno> {
        self.custody
            .permit(allocation_id, demand_id, expires_after, Instant::now())
    }

    pub fn outcome(&mut self, outcome: LockCandidateOutcome) -> Result<u64, Errno> {
        self.custody.outcome(outcome)
    }

    fn live(&mut self) -> Result<(), Errno> {
        self.expire(Instant::now());
        if self.custody.is_revoked() {
            Err(Errno::ESTALE)
        } else {
            Ok(())
        }
    }

    fn submit(&mut self, data: &[u8]) -> Result<(), Errno> {
        let submit = decode_lock_file_submit(data).map_err(|_| Errno::EINVAL)?;
        if submit.connection_epoch != self.custody.epoch() {
            return Err(Errno::ESTALE);
        }
        let staging = self.staging.as_ref().ok_or(Errno::EINVAL)?;
        if staging.bytes.len() != submit.candidate_bytes as usize {
            return Err(Errno::EINVAL);
        }
        let record = decode_lock_file_record(&staging.bytes, LockFileClass::Candidate)
            .map_err(|_| Errno::EINVAL)?;
        if record.header.submission_id != submit.submission_id {
            return Err(Errno::EINVAL);
        }
        self.custody.submit(&staging.bytes, Instant::now())?;
        // Submitted: the slot is free for the next candidate, and the fid
        // that staged this one can no longer change it.
        self.staging = None;
        Ok(())
    }

    fn acknowledge(&mut self, data: &[u8]) -> Result<(), Errno> {
        let ack = decode_lock_file_ack(data).map_err(|_| Errno::EINVAL)?;
        self.custody.acknowledge(ack)
    }

    fn upload_slots(&self) -> u8 {
        self.custody.limits().upload_slots as u8
    }
}

impl Export for LockFileExport {
    type Node = Node;
    type Handle = LockFileHandle;

    fn attach(&mut self, context: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        self.live()?;
        // One attach per admitted epoch; a replacement needs a fresh stream.
        if self.attached || self.connection != Some(context.connection) {
            return Err(Errno::EACCES);
        }
        self.attached = true;
        Ok(Attachment {
            root: Node::Root,
            epoch: Epoch(self.custody.epoch()),
        })
    }

    fn check(&mut self, access: &Access<'_, Node>) -> Result<(), Errno> {
        self.live()?;
        if !self.attached
            || access.epoch != Epoch(self.custody.epoch())
            || self.connection != Some(access.connection)
        {
            return Err(Errno::ESTALE);
        }
        Ok(())
    }

    fn lookup(&mut self, directory: &Node, name: WalkName<'_>) -> Result<Node, Errno> {
        match (directory, name) {
            (Node::Root, WalkName::Parent) | (Node::Uploads, WalkName::Parent) => Ok(Node::Root),
            (Node::Root, WalkName::Child(name)) => ENTRIES
                .iter()
                .find(|(entry, _)| *entry == name)
                .map(|(_, node)| *node)
                .ok_or(Errno::ENOENT),
            (Node::Uploads, WalkName::Child(name)) => std::str::from_utf8(name)
                .ok()
                .filter(|name| name.len() == 1)
                .and_then(|name| name.parse::<u8>().ok())
                .filter(|slot| *slot < self.upload_slots())
                .map(Node::Upload)
                .ok_or(Errno::ENOENT),
            _ => Err(Errno::ENOTDIR),
        }
    }

    fn describe(&self, node: &Node, handle: Option<&LockFileHandle>) -> Entry {
        let mut qid_path = self.base + node.index();
        let size = match (node, handle.map(|handle| &handle.0)) {
            (Node::Api, _) => self.api.len() as u64,
            (Node::Limits, _) => self.limits.len() as u64,
            (Node::Events, _) => self.custody.position().tail,
            (Node::Lock, Some(Handle::Lock(object))) => {
                qid_path = object.qid;
                object.bytes.len() as u64
            }
            (Node::Lock, _) => {
                qid_path = self.lock.qid;
                self.lock.bytes.len() as u64
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
        let directory = matches!(node, Node::Root | Node::Uploads);
        Entry {
            kind: if directory {
                NodeKind::Directory
            } else {
                NodeKind::File
            },
            qid_path,
            qid_version: 0,
            size,
            permissions: match node {
                Node::Root | Node::Uploads => 0o500,
                Node::Transaction => 0o600,
                Node::Submit | Node::Ack | Node::Upload(_) => 0o200,
                _ => 0o400,
            },
        }
    }

    fn open(&mut self, node: &Node, flags: OpenFlags) -> Result<LockFileHandle, Errno> {
        self.live()?;
        let access = flags.access().ok_or(Errno::EINVAL)?;
        let allowed = match node {
            Node::Transaction => access == OpenAccess::ReadWrite,
            Node::Submit | Node::Ack | Node::Upload(_) => access == OpenAccess::Write,
            _ => access == OpenAccess::Read,
        };
        if !allowed || flags.truncate() || flags.append() {
            return Err(Errno::EACCES);
        }
        let handle = match node {
            Node::Lock => Handle::Lock(Arc::clone(&self.lock)),
            Node::Transaction => {
                if self.staging.is_some() {
                    return Err(EBUSY);
                }
                let id = self.qids.allocate(1)?;
                let limits = self.custody.limits();
                self.staging = Some(Staging::new(
                    id,
                    StagingBounds {
                        header_bytes: LOCK_FILE_HEADER_BYTES,
                        max_bytes: LOCK_FILE_MAX_CANDIDATE_BYTES,
                        assembly: Duration::from_millis(limits.assembly_timeout_ms.into()),
                    },
                ));
                Handle::Transaction(id)
            }
            // A writer is bound to the slot's current upload, and fenced
            // from any later one.
            Node::Upload(slot) => {
                Handle::Upload(self.custody.upload_binding(*slot).ok_or(Errno::ESTALE)?)
            }
            _ => Handle::Plain,
        };
        Ok(LockFileHandle(handle))
    }

    fn read(
        &mut self,
        node: &Node,
        handle: &mut LockFileHandle,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        self.live()?;
        let bytes = match (node, &handle.0) {
            (Node::Api, _) => slice(&self.api, offset, count),
            (Node::Limits, _) => slice(&self.limits, offset, count),
            (Node::Events, _) => return self.custody.read(offset, count),
            (Node::Lock, Handle::Lock(object)) => slice(&object.bytes, offset, count),
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
        handle: &mut LockFileHandle,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, Errno> {
        self.live()?;
        match (node, &handle.0) {
            (Node::Transaction, Handle::Transaction(id)) => self
                .staging
                .as_mut()
                .filter(|s| s.handle == *id)
                .ok_or(Errno::ESTALE)?
                .write(offset, data, Instant::now()),
            (Node::Upload(slot), Handle::Upload(binding)) => {
                self.custody.write_upload(*slot, *binding, offset, data)
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
        _handle: &mut LockFileHandle,
        cookie: u64,
        max_entries: usize,
    ) -> Result<Vec<DirEntry>, Errno> {
        let entries: Vec<(Vec<u8>, Node)> = match directory {
            Node::Root => ENTRIES
                .iter()
                .map(|(name, node)| (name.to_vec(), *node))
                .collect(),
            Node::Uploads => (0..self.upload_slots())
                .map(|slot| (slot.to_string().into_bytes(), Node::Upload(slot)))
                .collect(),
            _ => return Err(Errno::ENOTDIR),
        };
        let start = usize::try_from(cookie)
            .unwrap_or(usize::MAX)
            .min(entries.len());
        Ok(entries
            .into_iter()
            .enumerate()
            .skip(start)
            .take(max_entries)
            .map(|(index, (name, node))| DirEntry {
                name,
                entry: self.describe(&node, None),
                next: (index + 1) as u64,
            })
            .collect())
    }

    fn release(&mut self, _node: Node, handle: Option<LockFileHandle>) {
        if let Some(Handle::Transaction(id)) = handle.map(|handle| handle.0)
            && self.staging.as_ref().is_some_and(|s| s.handle == id)
        {
            self.staging = None;
        }
    }
}
