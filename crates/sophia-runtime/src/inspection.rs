//! Independently enabled host read-only inspection. The service owns admission,
//! copied safe records and observation loss, never WM command/ACK authority.

mod export;
mod publication;
mod worker;

#[path = "../tests/support/inspection_owner.rs"]
mod tests;

pub use publication::{InspectionPublisher, PublishOutcome};
pub use sophia_protocol::inspection::*;

use crate::host_domain::HostDomain;
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;

pub const SOPHIA_WM_INSPECT_SOCKET_ENV: &str = "SOPHIA_WM_INSPECT_SOCKET";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectionError {
    Record(InspectionRecordError),
    Fenced,
    Stopped,
    Exhausted,
    Poisoned,
}
impl std::fmt::Display for InspectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Record(_) => "invalid inspection record",
            Self::Fenced => "inspection publication epoch fenced",
            Self::Stopped => "inspection service stopped",
            Self::Exhausted => "inspection identity space exhausted",
            Self::Poisoned => "inspection publication unavailable",
        })
    }
}
impl std::error::Error for InspectionError {}
impl From<InspectionRecordError> for InspectionError {
    fn from(value: InspectionRecordError) -> Self {
        Self::Record(value)
    }
}

pub struct InspectionService {
    directory: PathBuf,
    socket: PathBuf,
    shared: Arc<publication::Shared>,
    thread: Option<JoinHandle<()>>,
}

impl InspectionService {
    /// Fail closed before publishing an endpoint if host admission prerequisites
    /// are unavailable. The caller may leave inspection disabled and continue.
    pub fn bind(runtime_directory: &Path) -> io::Result<Self> {
        let metadata = std::fs::symlink_metadata(runtime_directory)?;
        if !runtime_directory.is_absolute()
            || !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "inspection runtime directory must be private and owned",
            ));
        }
        let domain = Arc::new(HostDomain::new()?);
        let mut nonce = [0_u8; 16];
        rustix::rand::getrandom(&mut nonce, rustix::rand::GetRandomFlags::empty())?;
        if nonce == [0; 16] {
            return Err(io::Error::other("invalid inspection identity"));
        }
        let name = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let directory = runtime_directory.join(format!("sophia-inspection-{name}"));
        std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let socket = directory.join("inspection.sock");
        let setup = (|| {
            let listener = UnixListener::bind(&socket)?;
            std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
            listener.set_nonblocking(true)?;
            let (wake_tx, wake_rx) = UnixStream::pair()?;
            wake_tx.set_nonblocking(true)?;
            wake_rx.set_nonblocking(true)?;
            let shared = Arc::new(publication::Shared::new(wake_tx));
            let worker_shared = shared.clone();
            let thread = std::thread::Builder::new()
                .name("sophia-inspection-v1".into())
                .spawn(move || worker::run(listener, wake_rx, domain, worker_shared))?;
            Ok(Self {
                directory: directory.clone(),
                socket: socket.clone(),
                shared,
                thread: Some(thread),
            })
        })();
        if setup.is_err() {
            let _ = std::fs::remove_file(&socket);
            let _ = std::fs::remove_dir(&directory);
        }
        setup
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket
    }
    pub fn publisher(&self) -> InspectionPublisher {
        InspectionPublisher {
            shared: self.shared.clone(),
        }
    }
    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Invalidate prior attachments synchronously, without taking the publication
    /// lock. Only Session's owner may replace this fence, not publisher clones.
    /// A fresh safe snapshot is required before snapshot/watch access; api and
    /// unavailable status remain readable by newly admitted host readers.
    pub fn fence(
        &mut self,
        wm_epoch: u64,
        excluded_wm_pid: Option<u32>,
    ) -> Result<(), InspectionError> {
        self.shared.fence(wm_epoch, excluded_wm_pid)
    }
}

impl Drop for InspectionService {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
        self.shared.wake();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_dir(&self.directory);
    }
}
