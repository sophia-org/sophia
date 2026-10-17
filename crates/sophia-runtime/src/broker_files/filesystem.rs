use super::*;
use crate::{ProtectionDomainEvidence, ProtectionDomainRole};
use sophia_9p::{
    connection::ConnectionId,
    export::*,
    journal::{Staging, StagingBounds},
    records::{OpenAccess, OpenFlags},
};
use std::cell::RefCell;

/// The Session owner prepares all effects without applying them, then commits
/// infallibly while the export holds the receipt reservation.
pub trait BrokerFileEffects {
    type Prepared;
    fn prepare(&mut self, set: &BrokerFileResponseSet<'_>) -> Result<Self::Prepared, Errno>;
    fn commit(&mut self, prepared: Self::Prepared);
    /// A semantic authority failure ends service, rather than inviting a retry
    /// against owner state that can no longer be trusted.
    fn failed(&self) -> bool {
        false
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrokerFileNode {
    Root,
    Api,
    Limits,
    Events,
    Transaction,
    Submit,
    Ack,
}
use BrokerFileNode as Node;
const ENTRIES: [(&[u8], Node); 6] = [
    (b"api", Node::Api),
    (b"limits", Node::Limits),
    (b"events", Node::Events),
    (b"transaction", Node::Transaction),
    (b"submit", Node::Submit),
    (b"ack", Node::Ack),
];
pub struct BrokerFileHandle {
    qid: u64,
    snapshot: Option<Vec<u8>>,
}
pub struct BrokerFileExport<A> {
    custody: BrokerFileCustody,
    owner: A,
    connection: ConnectionId,
    peer: PeerCredentials,
    attached: bool,
    base: u64,
    next_qid: u64,
    staging: Option<Staging>,
    submitted: bool,
}
impl<A: BrokerFileEffects> BrokerFileExport<A> {
    /// Trusted service supplies the kernel peer and supervisor's protection
    /// record. There is deliberately no PID-only constructor. Qid allocation and
    /// the request watermark must be carried across replacement exports.
    pub fn new(
        owner: A,
        connection: ConnectionId,
        peer: PeerCredentials,
        evidence: &ProtectionDomainEvidence,
        expected_uid: u32,
        epoch: u64,
        base: u64,
        last_request: u64,
        limits: BrokerFileLimits,
    ) -> Result<Self, Errno> {
        if peer.uid != expected_uid
            || peer.pid <= 0
            || peer.pid as u32 != evidence.peer_pid
            || !evidence
                .roles
                .contains(&ProtectionDomainRole::MetadataBroker)
        {
            return Err(Errno::EACCES);
        }
        let next_qid = base
            .checked_add(7)
            .filter(|_| base != 0)
            .ok_or(Errno::ENOSPC)?;
        Ok(Self {
            custody: BrokerFileCustody::new_after(epoch, limits, last_request)?,
            owner,
            connection,
            peer,
            attached: false,
            base,
            next_qid,
            staging: None,
            submitted: false,
        })
    }
    pub(super) fn bind_connection(&mut self, connection: ConnectionId) -> Result<(), Errno> {
        if self.attached || self.connection != ConnectionId(0) || connection == ConnectionId(0) {
            return Err(Errno::EACCES);
        }
        self.connection = connection;
        Ok(())
    }
    pub(super) fn wait(&self, now: Instant, maximum: Duration) -> Duration {
        let mut wait = maximum;
        for due in [
            self.custody.ack_due,
            self.custody.refusal.map(|(_, d)| d),
            self.custody.pending.as_ref().map(|p| p.due),
        ]
        .into_iter()
        .flatten()
        {
            wait = wait.min(due.saturating_duration_since(now));
        }
        if !self.submitted
            && let Some(staging) = &self.staging
        {
            wait = staging.wait(now, wait);
        }
        wait
    }
    pub fn custody(&self) -> &BrokerFileCustody {
        &self.custody
    }
    pub fn owner(&self) -> &A {
        &self.owner
    }
    pub fn owner_mut(&mut self) -> &mut A {
        &mut self.owner
    }
    pub fn publish(
        &mut self,
        id: u64,
        request: BrokerFileRequest<'_>,
        now: Instant,
    ) -> Result<(), Errno> {
        self.custody.publish(id, request, now)
    }
    pub fn next_qid(&self) -> u64 {
        self.next_qid
    }
    pub fn expire(&mut self, now: Instant) {
        if self.owner.failed() {
            self.custody.close();
        }
        self.custody.expire(now);
        if !self.submitted && self.staging.as_ref().is_some_and(|s| s.expired(now)) {
            self.staging = None;
        }
    }
    fn live(&mut self) -> Result<(), Errno> {
        self.expire(Instant::now());
        if self.custody.closed() {
            Err(Errno::ESTALE)
        } else {
            Ok(())
        }
    }
    fn submit(&mut self, data: &[u8]) -> Result<(), Errno> {
        let staged = &self.staging.as_ref().ok_or(Errno::EINVAL)?.bytes;
        if staged.len() < 8 {
            return Err(Errno::EINVAL);
        }
        let kind = u16::from_le_bytes([staged[6], staged[7]]);
        match kind {
            256 => self.custody.negotiate(data, staged, Instant::now())?,
            257 => {
                let owner = RefCell::new(&mut self.owner);
                self.custody.respond(
                    data,
                    staged,
                    Instant::now(),
                    |set| owner.borrow_mut().prepare(set),
                    |prepared| owner.borrow_mut().commit(prepared),
                )?;
            }
            _ => return Err(Errno::EINVAL),
        }
        self.submitted = true;
        Ok(())
    }
}
fn slice(bytes: &[u8], offset: u64, count: u32) -> Vec<u8> {
    let start = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    bytes[start..start.saturating_add(count as usize).min(bytes.len())].to_vec()
}
impl<A: BrokerFileEffects> Export for BrokerFileExport<A> {
    type Node = Node;
    type Handle = BrokerFileHandle;
    fn attach(&mut self, c: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        self.live()?;
        if self.attached || c.connection != self.connection || c.peer != Some(self.peer) {
            return Err(Errno::EACCES);
        }
        self.attached = true;
        Ok(Attachment {
            root: Node::Root,
            epoch: Epoch(self.custody.epoch),
        })
    }
    fn check(&mut self, a: &Access<'_, Node>) -> Result<(), Errno> {
        self.live()?;
        if !self.attached || a.connection != self.connection || a.epoch != Epoch(self.custody.epoch)
        {
            return Err(Errno::ESTALE);
        }
        Ok(())
    }
    fn connection_closed(&mut self, c: ConnectionId) {
        if c == self.connection {
            self.custody.close();
            self.staging = None;
        }
    }
    fn wire_reset(&mut self, c: ConnectionId) {
        if self.attached {
            self.connection_closed(c);
        }
    }
    fn lookup(&mut self, node: &Node, name: WalkName<'_>) -> Result<Node, Errno> {
        match (node, name) {
            (Node::Root, WalkName::Parent) => Ok(Node::Root),
            (Node::Root, WalkName::Child(name)) => ENTRIES
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, n)| *n)
                .ok_or(Errno::ENOENT),
            _ => Err(Errno::ENOTDIR),
        }
    }
    fn describe(&self, node: &Node, handle: Option<&BrokerFileHandle>) -> Entry {
        let qid = handle.map_or(self.base + *node as u64, |h| h.qid);
        let size = match node {
            Node::Api => format!(
                "sophia-broker-files version=1 role=metadata-broker epoch={} fd_transfer=none\n",
                self.custody.epoch
            )
            .len() as u64,
            Node::Limits => 72,
            Node::Events => self.custody.journal.size(),
            Node::Transaction => self
                .staging
                .as_ref()
                .filter(|s| s.handle == qid)
                .map_or(0, |s| s.bytes.len() as u64),
            _ => 0,
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
                Node::Transaction => 0o600,
                Node::Submit | Node::Ack => 0o200,
                _ => 0o400,
            },
        }
    }
    fn open(&mut self, node: &Node, flags: OpenFlags) -> Result<Self::Handle, Errno> {
        self.live()?;
        let access = match node {
            Node::Transaction => OpenAccess::ReadWrite,
            Node::Submit | Node::Ack => OpenAccess::Write,
            _ => OpenAccess::Read,
        };
        if flags.access() != Some(access) || flags.append() || flags.truncate() {
            return Err(Errno::EACCES);
        }
        let mut qid = self.base + *node as u64;
        let snapshot=match node {
            Node::Api=>Some(format!("sophia-broker-files version=1 role=metadata-broker epoch={} fd_transfer=none\n",self.custody.epoch).into_bytes()),
            Node::Limits=>Some(self.custody.limits.encode(self.custody.epoch).map_err(|_|Errno::EINVAL)?.to_vec()),
            Node::Transaction=>{
                if self.custody.refusal.is_some() {return Err(Errno::EACCES);}
                if self.staging.is_some() || self.custody.retained().is_some() {return Err(Errno(16));}
                qid=self.next_qid;self.next_qid=qid.checked_add(1).ok_or(Errno::ENOSPC)?;
                self.staging=Some(Staging::new(qid,StagingBounds {header_bytes:40,max_bytes:448,
                    assembly:Duration::from_millis(u64::from(self.custody.limits.assembly_timeout_ms))}));
                self.submitted=false;None
            }
            _=>None,
        };
        Ok(BrokerFileHandle { qid, snapshot })
    }
    fn read(
        &mut self,
        node: &Node,
        handle: &mut Self::Handle,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        self.live()?;
        match node {
            Node::Api | Node::Limits => Ok(ReadOutcome::Ready(slice(
                handle.snapshot.as_ref().ok_or(Errno::EINVAL)?,
                offset,
                count,
            ))),
            Node::Events => self.custody.read(offset, count, Instant::now()),
            Node::Transaction => {
                if self.submitted && self.custody.retained().is_none() {
                    return Err(Errno::ESTALE);
                }
                let s = self
                    .staging
                    .as_ref()
                    .filter(|s| s.handle == handle.qid)
                    .ok_or(Errno::ESTALE)?;
                Ok(ReadOutcome::Ready(slice(&s.bytes, offset, count)))
            }
            _ => Err(Errno::EACCES),
        }
    }
    fn write(
        &mut self,
        node: &Node,
        handle: &mut Self::Handle,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, Errno> {
        self.live()?;
        match node {
            Node::Transaction => {
                let s = self
                    .staging
                    .as_mut()
                    .filter(|s| s.handle == handle.qid)
                    .ok_or(Errno::ESTALE)?;
                if self.submitted {
                    return Err(Errno::EACCES);
                }
                s.write(offset, data, Instant::now())
            }
            Node::Submit if offset == 0 => {
                self.submit(data)?;
                Ok(data.len() as u32)
            }
            Node::Ack if offset == 0 => {
                self.custody.acknowledge(data, Instant::now())?;
                Ok(data.len() as u32)
            }
            _ => Err(Errno::EINVAL),
        }
    }
    fn readdir(
        &mut self,
        node: &Node,
        _handle: &mut Self::Handle,
        cookie: u64,
        max: usize,
    ) -> Result<Vec<DirEntry>, Errno> {
        if *node != Node::Root {
            return Err(Errno::ENOTDIR);
        }
        let start = usize::try_from(cookie)
            .unwrap_or(usize::MAX)
            .min(ENTRIES.len());
        Ok(ENTRIES[start..]
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
    fn release(&mut self, _node: Node, handle: Option<Self::Handle>) {
        if let Some(handle) = handle
            && self
                .staging
                .as_ref()
                .is_some_and(|s| s.handle == handle.qid)
        {
            self.staging = None;
            self.submitted = false;
        }
    }
}
