//! Authentication around a role's existing file owner. File semantics and
//! in-place writes stay with that owner; Session supplies the afid authority.
use sophia_9p::{
    ConnectionId, Errno, Fid, OpenFlags, ReadOutcome, authentication::Attempt, export::*,
};
use std::time::{Duration, Instant};

/// Trusted, transport-scoped authentication owner. Implementations must retain
/// identity across reset and must not wait on a peer or another owner thread.
pub trait FileAuthentication: Send {
    fn claims(&self, context: &AttachContext<'_>) -> Result<(), Errno>;
    fn begin(&mut self, afid: Fid, now: Instant) -> Result<Attempt, Errno>;
    fn read(
        &mut self,
        attempt: Attempt,
        offset: u64,
        count: u32,
        now: Instant,
    ) -> Result<ReadOutcome, Errno>;
    fn commit(&mut self, attempt: Attempt, now: Instant) -> Result<(), Errno>;
    fn check(&mut self, connection: ConnectionId, now: Instant) -> Result<(), Errno>;
    fn release(&mut self, attempt: Attempt, now: Instant);
    fn reset(&mut self, now: Instant) -> Result<(), Errno>;
    fn close(&mut self, now: Instant);
    /// Includes idle deadlines and current launch custody, even without I/O.
    fn poll(&mut self, now: Instant) -> Result<(), Errno>;
    /// A closed owner retains its quota slot until cleanup is acknowledged.
    fn drained(&self) -> bool;
    fn admitted(&self) -> bool;
    /// Immutable facts, not proof of current authority. Use check before an
    /// operation; namespace and launch revocation still govern retained fids.
    fn admission_context(&self) -> Option<sophia_protocol::ClientAdmissionContext>;
    fn wait(&self, now: Instant) -> Duration;
}

/// Session supplies one factory per authorized role endpoint. Construction is
/// bounded and reserves the transport slot before any authentication traffic.
pub trait FileAuthenticationFactory: Send {
    fn create(
        &mut self,
        connection: ConnectionId,
        now: Instant,
        wake: sophia_9p::unix::Wake,
    ) -> Result<Box<dyn FileAuthentication>, Errno>;

    /// Hand closed transport custody to its Session cleanup loop without
    /// waiting on that loop from a worker being joined. Refusal returns custody
    /// to the caller for fail-closed disposal.
    fn retire(
        &mut self,
        owner: Box<dyn FileAuthentication>,
    ) -> Result<(), Box<dyn FileAuthentication>> {
        Err(owner)
    }
}

/// The role policy has checked its root; publication cannot subsequently fail.
pub trait PreparedFileRoot {
    type Node;
    fn publish(self) -> Attachment<Self::Node>;
}

pub trait ProtectedFiles: Export {
    type Prepared<'a>: PreparedFileRoot<Node = Self::Node>
    where
        Self: 'a;
    fn prepare_authenticated_root(
        &mut self,
        context: &AttachContext<'_>,
    ) -> Result<Self::Prepared<'_>, Errno>;
    fn allocate_auth_qid(&mut self) -> Result<u64, Errno>;
    /// Called after the core has released every fid. Keep role epoch and data;
    /// only the transport's immutable identity may reattach after fresh auth.
    fn reset_authenticated_root(&mut self, connection: ConnectionId);
}

#[derive(Clone, Eq, PartialEq)]
pub enum AuthenticatedNode<N> {
    File(N),
    Auth { attempt: Attempt, qid: u64 },
}
pub enum AuthenticatedHandle<H> {
    File(H),
    Auth,
}

enum Admission {
    Legacy,
    Pending,
    Authenticated(Box<dyn FileAuthentication>),
}

/// Pending authenticated construction refuses everything; failure to install
/// an owner can never fall back to legacy attach. The mode is owner-selected.
pub struct AuthenticatedFiles<E> {
    files: E,
    admission: Admission,
}
impl<E> AuthenticatedFiles<E> {
    pub fn legacy(files: E) -> Self {
        Self {
            files,
            admission: Admission::Legacy,
        }
    }
    pub fn pending(files: E) -> Self {
        Self {
            files,
            admission: Admission::Pending,
        }
    }
    pub fn install(&mut self, owner: Box<dyn FileAuthentication>) -> Result<(), Errno> {
        if !matches!(self.admission, Admission::Pending) {
            return Err(Errno::EACCES);
        }
        self.admission = Admission::Authenticated(owner);
        Ok(())
    }
    pub fn files(&self) -> &E {
        &self.files
    }
    pub fn files_mut(&mut self) -> &mut E {
        &mut self.files
    }
    pub fn admission_context(&self) -> Option<sophia_protocol::ClientAdmissionContext> {
        match &self.admission {
            Admission::Authenticated(owner) => owner.admission_context(),
            Admission::Legacy | Admission::Pending => None,
        }
    }
    pub fn poll(&mut self, now: Instant) -> Result<(), Errno> {
        match &mut self.admission {
            Admission::Legacy => Ok(()),
            Admission::Pending => Err(Errno::EACCES),
            Admission::Authenticated(owner) => owner.poll(now),
        }
    }
    pub fn wait(&self, now: Instant) -> Duration {
        match &self.admission {
            Admission::Legacy => Duration::MAX,
            Admission::Pending => Duration::ZERO,
            Admission::Authenticated(owner) => owner.wait(now),
        }
    }
    pub fn admitted(&self) -> bool {
        match &self.admission {
            Admission::Legacy => true,
            Admission::Pending => false,
            Admission::Authenticated(owner) => owner.admitted(),
        }
    }
    /// Detach cleanup before dropping the server. The transport must keep
    /// polling this owner, and its quota, until drained (or failed closed).
    pub fn retire(&mut self, now: Instant) -> Option<Box<dyn FileAuthentication>> {
        match std::mem::replace(&mut self.admission, Admission::Pending) {
            Admission::Authenticated(mut owner) => {
                owner.close(now);
                Some(owner)
            }
            _ => None,
        }
    }
    fn authentication(&mut self) -> Result<&mut (dyn FileAuthentication + '_), Errno> {
        match &mut self.admission {
            Admission::Authenticated(owner) => Ok(owner.as_mut()),
            _ => Err(Errno::EACCES),
        }
    }
}

impl<E: ProtectedFiles> Export for AuthenticatedFiles<E> {
    type Node = AuthenticatedNode<E::Node>;
    type Handle = AuthenticatedHandle<E::Handle>;
    const WRITES_IN_PLACE: bool = E::WRITES_IN_PLACE;
    fn requires_authentication(&self) -> bool {
        !matches!(self.admission, Admission::Legacy)
    }

    fn attach(&mut self, c: &AttachContext<'_>) -> Result<Attachment<Self::Node>, Errno> {
        if !matches!(self.admission, Admission::Legacy) {
            return Err(Errno::EACCES);
        }
        self.files.attach(c).map(map_attachment)
    }
    fn auth(
        &mut self,
        c: &AttachContext<'_>,
        afid: Fid,
    ) -> Result<AuthAttachment<Self::Node, Self::Handle>, Errno> {
        if matches!(self.admission, Admission::Legacy) {
            return Err(Errno::EOPNOTSUPP);
        }
        self.authentication()?.claims(c)?;
        let qid = self.files.allocate_auth_qid()?;
        let attempt = self.authentication()?.begin(afid, Instant::now())?;
        Ok(AuthAttachment {
            node: AuthenticatedNode::Auth { attempt, qid },
            handle: AuthenticatedHandle::Auth,
            epoch: Epoch(attempt.wire_generation),
            qid_path: qid,
        })
    }
    fn attach_authenticated(
        &mut self,
        c: &AttachContext<'_>,
        node: &Self::Node,
        handle: &mut Self::Handle,
    ) -> Result<Attachment<Self::Node>, Errno> {
        let (AuthenticatedNode::Auth { attempt, .. }, AuthenticatedHandle::Auth) = (node, handle)
        else {
            return Err(Errno::EACCES);
        };
        if attempt.connection != c.connection {
            return Err(Errno::EACCES);
        }
        let Admission::Authenticated(owner) = &mut self.admission else {
            return Err(Errno::EACCES);
        };
        owner.claims(c)?;
        let root = self.files.prepare_authenticated_root(c)?;
        owner.commit(*attempt, Instant::now())?;
        Ok(map_attachment(root.publish()))
    }
    fn check(&mut self, a: &Access<'_, Self::Node>) -> Result<(), Errno> {
        match a.node {
            AuthenticatedNode::Auth { attempt, .. } => {
                if attempt.connection != a.connection {
                    return Err(Errno::EACCES);
                }
                self.authentication()?.poll(Instant::now())
            }
            AuthenticatedNode::File(node) => {
                match &mut self.admission {
                    Admission::Authenticated(owner) => owner.check(a.connection, Instant::now())?,
                    Admission::Pending => return Err(Errno::EACCES),
                    Admission::Legacy => (),
                }
                self.files.check(&Access {
                    connection: a.connection,
                    epoch: a.epoch,
                    node,
                    operation: a.operation,
                })
            }
        }
    }
    fn guard_read_delivery(&self, node: &Self::Node) -> bool {
        matches!(node, AuthenticatedNode::File(node) if self.files.guard_read_delivery(node))
    }
    fn with_delivery<T>(
        &mut self,
        access: &Access<'_, Self::Node>,
        deliver: impl FnOnce() -> T,
    ) -> Result<T, Errno> {
        self.check(access)?;
        match access.node {
            AuthenticatedNode::File(node) => self.files.with_delivery(
                &Access {
                    connection: access.connection,
                    epoch: access.epoch,
                    node,
                    operation: access.operation,
                },
                deliver,
            ),
            _ => Ok(deliver()),
        }
    }
    fn lookup(&mut self, node: &Self::Node, name: WalkName<'_>) -> Result<Self::Node, Errno> {
        let AuthenticatedNode::File(node) = node else {
            return Err(Errno::EACCES);
        };
        self.files.lookup(node, name).map(AuthenticatedNode::File)
    }
    fn describe(&self, node: &Self::Node, handle: Option<&Self::Handle>) -> Entry {
        match node {
            AuthenticatedNode::Auth { qid, .. } => Entry {
                kind: NodeKind::File,
                qid_path: *qid,
                qid_version: 0,
                permissions: 0o400,
                size: 48,
            },
            AuthenticatedNode::File(node) => self.files.describe(
                node,
                handle.and_then(|h| match h {
                    AuthenticatedHandle::File(h) => Some(h),
                    _ => None,
                }),
            ),
        }
    }
    fn open(&mut self, node: &Self::Node, flags: OpenFlags) -> Result<Self::Handle, Errno> {
        let AuthenticatedNode::File(node) = node else {
            return Err(Errno::EACCES);
        };
        self.files.open(node, flags).map(AuthenticatedHandle::File)
    }
    fn read(
        &mut self,
        node: &Self::Node,
        handle: &mut Self::Handle,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        match (node, handle) {
            (AuthenticatedNode::Auth { attempt, .. }, AuthenticatedHandle::Auth) => self
                .authentication()?
                .read(*attempt, offset, count, Instant::now()),
            (AuthenticatedNode::File(node), AuthenticatedHandle::File(handle)) => {
                self.files.read(node, handle, offset, count)
            }
            _ => Err(Errno::EACCES),
        }
    }
    fn write(
        &mut self,
        node: &Self::Node,
        handle: &mut Self::Handle,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, Errno> {
        let (AuthenticatedNode::File(node), AuthenticatedHandle::File(handle)) = (node, handle)
        else {
            return Err(Errno::EACCES);
        };
        self.files.write(node, handle, offset, data)
    }
    fn write_destination(
        &mut self,
        node: &Self::Node,
        handle: &mut Self::Handle,
        offset: u64,
        len: u32,
        received: u32,
    ) -> Result<Option<&mut [u8]>, Errno> {
        let (AuthenticatedNode::File(node), AuthenticatedHandle::File(handle)) = (node, handle)
        else {
            return Err(Errno::EACCES);
        };
        self.files
            .write_destination(node, handle, offset, len, received)
    }
    fn write_received(
        &mut self,
        node: &Self::Node,
        handle: &mut Self::Handle,
        offset: u64,
        len: u32,
    ) -> Result<u32, Errno> {
        let (AuthenticatedNode::File(node), AuthenticatedHandle::File(handle)) = (node, handle)
        else {
            return Err(Errno::EACCES);
        };
        self.files.write_received(node, handle, offset, len)
    }
    fn readdir(
        &mut self,
        node: &Self::Node,
        handle: &mut Self::Handle,
        cookie: u64,
        count: usize,
    ) -> Result<Vec<DirEntry>, Errno> {
        let (AuthenticatedNode::File(node), AuthenticatedHandle::File(handle)) = (node, handle)
        else {
            return Err(Errno::EACCES);
        };
        self.files.readdir(node, handle, cookie, count)
    }
    fn release(&mut self, node: Self::Node, handle: Option<Self::Handle>) {
        match node {
            AuthenticatedNode::Auth { attempt, .. } => {
                if let Ok(owner) = self.authentication() {
                    owner.release(attempt, Instant::now());
                }
            }
            AuthenticatedNode::File(node) => self.files.release(
                node,
                handle.and_then(|h| match h {
                    AuthenticatedHandle::File(h) => Some(h),
                    _ => None,
                }),
            ),
        }
    }
    fn wire_reset(&mut self, connection: ConnectionId) {
        match &mut self.admission {
            Admission::Authenticated(owner) => {
                if owner.reset(Instant::now()).is_err() {
                    owner.close(Instant::now());
                }
                self.files.reset_authenticated_root(connection);
            }
            Admission::Legacy => self.files.wire_reset(connection),
            Admission::Pending => (),
        }
    }
    fn connection_closed(&mut self, connection: ConnectionId) {
        if let Admission::Authenticated(owner) = &mut self.admission {
            owner.close(Instant::now());
        }
        self.files.connection_closed(connection);
    }
}

fn map_attachment<N>(a: Attachment<N>) -> Attachment<AuthenticatedNode<N>> {
    Attachment {
        root: AuthenticatedNode::File(a.root),
        epoch: a.epoch,
    }
}
