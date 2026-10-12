//! Filesystem publication only: endpoint listeners go to the existing role
//! owners. Nothing here authenticates, proxies a connection or grants access.
use sophia_protocol::{NamespaceId, namespace_view::*};
use std::fs;
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

pub const MAX_RETAINED_VIEWS: usize = 64;

#[path = "../tests/support/namespace_publication.rs"]
mod tests;

/// Session routes this listener to the owner of `endpoint.service`, retaining
/// the captured scope. Aliases are listeners of that same export instance.
pub struct NamespaceListener {
    pub endpoint: NamespaceEndpoint,
    pub listener: UnixListener,
}

struct Resources {
    directory: PathBuf,
    view: CompiledNamespace,
    _parent: Arc<Parent>,
}
struct Parent(PathBuf);
impl Drop for Parent {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}
struct TemporarySocket(PathBuf);
impl Drop for TemporarySocket {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
impl Drop for Resources {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

/// A connection keeps this lease while it names the old view. It confers no
/// authority: role checks still consult live admission and grant incarnation.
#[derive(Clone)]
pub struct NamespaceViewLease(Arc<Resources>);
impl NamespaceViewLease {
    pub fn directory(&self) -> &Path {
        &self.0.directory
    }
    pub fn view(&self) -> &CompiledNamespace {
        &self.0.view
    }
}

pub struct PreparedNamespace {
    lease: NamespaceViewLease,
    listeners: Vec<NamespaceListener>,
    publisher: PathBuf,
}

pub struct PublishedNamespace {
    pub lease: NamespaceViewLease,
    pub listeners: Vec<NamespaceListener>,
}

/// One identity's private parent. `current` changes atomically; launched
/// processes receive the exact generation directory, never that mutable name.
pub struct NamespacePublisher {
    directory: PathBuf,
    parent: Arc<Parent>,
    namespace: NamespaceId,
    principal: u64,
    next: u64,
    last_revision: u64,
    current: Option<NamespaceViewLease>,
    retained: Vec<Weak<Resources>>,
}
impl NamespacePublisher {
    /// The parent is already private, owned, absolute and symlink-free. This
    /// owner allocates its own random child; it never adopts another publisher.
    pub fn new(parent: &Path, namespace: NamespaceId, principal: u64) -> io::Result<Self> {
        let m = fs::symlink_metadata(parent)?;
        if !parent.is_absolute()
            || fs::canonicalize(parent)? != parent
            || !m.is_dir()
            || m.uid() != rustix::process::geteuid().as_raw()
            || m.mode() & 0o077 != 0
            || !namespace.is_valid()
            || principal == 0
        {
            return Err(refused(
                "namespace publication parent or identity is invalid",
            ));
        }
        let mut nonce = [0; 16];
        if rustix::rand::getrandom(&mut nonce, rustix::rand::GetRandomFlags::NONBLOCK)?
            != nonce.len()
        {
            return Err(io::Error::other(
                "namespace publication entropy unavailable",
            ));
        }
        let name = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let directory = parent.join(format!("ns-{name}"));
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let parent = Arc::new(Parent(directory.clone()));
        Ok(Self {
            directory,
            parent,
            namespace,
            principal,
            next: 1,
            last_revision: 0,
            current: None,
            retained: Vec::new(),
        })
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn current(&self) -> Option<NamespaceViewLease> {
        self.current.clone()
    }

    pub fn prepare(
        &mut self,
        view: CompiledNamespace,
        discovery: NamespaceDiscovery,
    ) -> io::Result<PreparedNamespace> {
        self.prepare_inner(view, discovery, |_| Ok(()))
    }

    fn prepare_inner(
        &mut self,
        view: CompiledNamespace,
        discovery: NamespaceDiscovery,
        mut after_bind: impl FnMut(usize) -> io::Result<()>,
    ) -> io::Result<PreparedNamespace> {
        if view.namespace != self.namespace
            || view.principal != self.principal
            || view.revision <= self.last_revision
            || discovery.revision != view.revision
            || discovery.endpoints.len() != view.endpoints.len()
        {
            return Err(refused(
                "namespace publication identity or revision mismatch",
            ));
        }
        for (endpoint, observed) in view.endpoints.iter().zip(&discovery.endpoints) {
            let scope = endpoint.endpoint.scope;
            if endpoint.path != observed.path
                || endpoint.endpoint.service != observed.service
                || scope.namespace != self.namespace
                || scope.principal != self.principal
                || scope.grant == 0
                || scope.generation == 0
            {
                return Err(refused("namespace discovery differs from admitted view"));
            }
        }
        let bytes = encode_discovery(&discovery).map_err(io::Error::other)?;
        self.retained.retain(|r| r.strong_count() > 0);
        if self.retained.len() >= MAX_RETAINED_VIEWS {
            return Err(refused("namespace retained view capacity exhausted"));
        }
        let generation = self.next;
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| refused("namespace generation exhausted"))?;
        let directory = self.directory.join(format!("v{generation}"));
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let resources = Arc::new(Resources {
            directory,
            view,
            _parent: self.parent.clone(),
        });
        let lease = NamespaceViewLease(resources);
        let mut listeners = Vec::new();
        let root = fs::File::open(&self.directory)?;
        for endpoint in &lease.view().endpoints {
            let path = lease.directory().join(&endpoint.path);
            if let Some(parent) = path.parent() {
                fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(parent)?;
            }
            // Bind through a pinned directory descriptor, then rename the
            // socket inode into the view. Logical names need not fit sun_path;
            // Linux SDK readers likewise connect through a pinned O_PATH fd.
            let listener =
                UnixListener::bind(format!("/proc/self/fd/{}/.socket", root.as_raw_fd()))?;
            let temporary = TemporarySocket(self.directory.join(".socket"));
            fs::rename(&temporary.0, &path)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
            listener.set_nonblocking(true)?;
            listeners.push(NamespaceListener {
                endpoint: endpoint.clone(),
                listener,
            });
            after_bind(listeners.len())?;
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lease.directory().join(DISCOVERY_NAME))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::set_permissions(
            lease.directory().join(DISCOVERY_NAME),
            fs::Permissions::from_mode(0o400),
        )?;
        fs::File::open(lease.directory())?.sync_all()?;
        self.retained.push(Arc::downgrade(&lease.0));
        Ok(PreparedNamespace {
            lease,
            listeners,
            publisher: self.directory.clone(),
        })
    }

    /// Session applies authority reductions and verifies pinned config before
    /// this call. Failure never rolls back those independent revocations.
    pub fn publish(&mut self, prepared: PreparedNamespace) -> io::Result<PublishedNamespace> {
        if prepared.publisher != self.directory
            || prepared.lease.view().revision <= self.last_revision
        {
            return Err(refused("stale or foreign namespace candidate"));
        }
        let next = self.directory.join(".current-next");
        symlink(
            prepared
                .lease
                .directory()
                .file_name()
                .expect("generation directory"),
            &next,
        )?;
        if let Err(error) = fs::rename(&next, self.directory.join("current")) {
            let _ = fs::remove_file(next);
            return Err(error);
        }
        // The rename is the publication point. No fallible action follows it;
        // callers can never mistake an already-published view for a refusal.
        self.last_revision = prepared.lease.view().revision;
        self.current = Some(prepared.lease.clone());
        Ok(PublishedNamespace {
            lease: prepared.lease,
            listeners: prepared.listeners,
        })
    }
}
impl Drop for NamespacePublisher {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.directory.join("current"));
        self.current = None;
        // Old leases may outlive the publisher. Do not recursively remove
        // their endpoints; their last owner reclaims the generation directory.
        // Parent's last reference removes the empty directory after old leases.
    }
}
fn refused(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
