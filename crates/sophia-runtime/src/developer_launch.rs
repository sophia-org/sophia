//! Explicit host-administrative launch requests served by the shared 9P core.
//! Session decides recipes and owns processes; this worker only owns requests.
mod export;
mod ticket;
mod worker;
use crate::host_domain::HostDomain;
use std::io;
use std::os::unix::{
    fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    net::UnixListener,
};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, sync_channel},
};
use std::thread::JoinHandle;
use std::time::Duration;
pub use ticket::DeveloperLaunchTicket;

pub const DEVELOPER_LAUNCH_MAX_PEERS: usize = 8;
pub const DEVELOPER_LAUNCH_ADMISSION_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEVELOPER_LAUNCH_RESULT_TIMEOUT: Duration = Duration::from_secs(30);

struct Shared {
    stopped: AtomicBool,
    worker_wake: sophia_wake::Notifier,
    owner_wake: sophia_wake::WakeSlot,
}

/// Calling bind is an administrative publication decision. The Session caller
/// must require host-admin policy AND an explicitly enabled developer recipe.
pub struct DeveloperLaunchService {
    directory: PathBuf,
    socket: PathBuf,
    requests: Receiver<DeveloperLaunchTicket>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}
impl DeveloperLaunchService {
    pub fn bind(runtime: &Path) -> io::Result<Self> {
        let metadata = std::fs::symlink_metadata(runtime)?;
        if !runtime.is_absolute()
            || !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o077 != 0
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "launch runtime directory must be private and owned",
            ));
        }
        let domain = Arc::new(HostDomain::new()?);
        let wake = sophia_wake::Wake::new()?;
        let shared = Arc::new(Shared {
            stopped: AtomicBool::new(false),
            worker_wake: wake.notifier(),
            owner_wake: Default::default(),
        });
        let mut nonce = [0u8; 16];
        rustix::rand::getrandom(&mut nonce, rustix::rand::GetRandomFlags::empty())?;
        let name = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let directory = runtime.join(format!("sophia-launch-{name}"));
        std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let socket = directory.join("launch.sock");
        let setup = (|| {
            let listener = UnixListener::bind(&socket)?;
            std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
            listener.set_nonblocking(true)?;
            let (sender, requests) = sync_channel(DEVELOPER_LAUNCH_MAX_PEERS);
            let state = shared.clone();
            let thread = std::thread::Builder::new()
                .name("sophia-launch-files".into())
                .spawn(move || worker::run(listener, wake, domain, state, sender))?;
            Ok(Self {
                directory: directory.clone(),
                socket: socket.clone(),
                requests,
                shared,
                thread: Some(thread),
            })
        })();
        if setup.is_err() {
            let _ = std::fs::remove_file(socket);
            let _ = std::fs::remove_dir(directory);
        }
        setup
    }
    pub fn socket_path(&self) -> &Path {
        &self.socket
    }
    pub fn set_owner_wake(&self, wake: sophia_wake::Notifier) {
        self.shared.owner_wake.set(wake);
    }
    pub fn try_request(&self) -> Option<DeveloperLaunchTicket> {
        self.requests.try_recv().ok()
    }
    pub fn is_running(&self) -> bool {
        !self.shared.stopped.load(Ordering::Acquire)
            && self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }
}
impl Drop for DeveloperLaunchService {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::Release);
        self.shared.worker_wake.notify();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_dir(&self.directory);
    }
}
