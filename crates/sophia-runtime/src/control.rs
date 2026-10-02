mod admission;
mod client;
mod transport;

pub use client::*;

use sophia_protocol::{ControlCatalog, ControlCommand, ControlOutcome};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, Ordering};
use std::sync::{
    Arc, Mutex,
    mpsc::{Receiver, sync_channel},
};
use std::thread::JoinHandle;
use std::time::Instant;

pub const SOPHIA_CONTROL_SOCKET_ENV: &str = "SOPHIA_CONTROL_SOCKET";
pub const CONTROL_MAX_PENDING: usize = 16;

struct TicketState {
    // 0 queued, 3 asks for a fresh admission check, 4 checked, 1 dispatched,
    // 2 cancelled. The worker performs all peer inspection outside the owner.
    phase: AtomicU8,
    outcome: AtomicU16,
    settled: AtomicBool,
    wake: Arc<UnixStream>,
}

#[derive(Clone)]
pub struct ControlTicket {
    pub connection: u64,
    pub request: u64,
    pub generation: u64,
    pub command: ControlCommand,
    pub received: Instant,
    state: Arc<TicketState>,
}

impl ControlTicket {
    /// Claim only immediately before dispatch. A cancelled queued request
    /// cannot subsequently take effect, even if its event remained buffered.
    pub fn claim(&self) -> bool {
        if self
            .state
            .phase
            .compare_exchange(0, 3, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            wake(&self.state.wake);
        }
        self.state
            .phase
            .compare_exchange(4, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    pub fn dispatched(&self) -> bool {
        self.state.phase.load(Ordering::Acquire) == 1
    }
    pub fn cancelled(&self) -> bool {
        self.state.phase.load(Ordering::Acquire) == 2
    }
    pub fn finish(&self, outcome: ControlOutcome) {
        self.cancel_queued();
        let _ = self.state.outcome.compare_exchange(
            0,
            outcome as u16,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.state.settled.store(true, Ordering::Release);
        wake(&self.state.wake);
    }
    fn expire(&self) {
        if self.cancel_queued() {
            self.finish(ControlOutcome::TimedOut);
        } else {
            let _ = self.state.outcome.compare_exchange(
                0,
                ControlOutcome::Indeterminate as u16,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
    fn disconnect(&self) {
        if self.cancel_queued() {
            self.state.settled.store(true, Ordering::Release);
        }
    }
    fn cancel_queued(&self) -> bool {
        self.state
            .phase
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |phase| {
                matches!(phase, 0 | 3 | 4).then_some(2)
            })
            .is_ok()
    }
}

fn wake(stream: &UnixStream) {
    let _ = (&*stream).write(&[1]);
}

struct View {
    catalog: Arc<ControlCatalog>,
    excluded: Vec<u32>,
}

/// One worker owns all untrusted byte streams. The owner loop never parses
/// sockets, inspects /proc, or waits on a client or a worker result.
pub struct ControlService {
    directory: PathBuf,
    socket: PathBuf,
    view: Arc<Mutex<View>>,
    requests: Receiver<ControlTicket>,
    stop: Arc<AtomicBool>,
    wake: Arc<UnixStream>,
    /// The owner's wake. The worker rings it after queuing a ticket, after
    /// settling a claim's admission recheck, and when it stops.
    owner_wake: sophia_wake::WakeSlot,
    thread: Option<JoinHandle<()>>,
}

impl ControlService {
    pub fn bind(runtime_directory: &Path) -> io::Result<Self> {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata(runtime_directory)?;
        if !runtime_directory.is_absolute()
            || !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "control runtime directory must be private and owned",
            ));
        }
        let domain = admission::HostDomain::new()?;
        let mut nonce = [0_u8; 16];
        rustix::rand::getrandom(&mut nonce, rustix::rand::GetRandomFlags::empty())?;
        let session_id = [
            u64::from_le_bytes(nonce[..8].try_into().unwrap()),
            u64::from_le_bytes(nonce[8..].try_into().unwrap()),
        ];
        if session_id == [0, 0] {
            return Err(io::Error::other("invalid random session identity"));
        }
        let directory = runtime_directory.join(format!(
            "sophia-control-{:016x}{:016x}",
            session_id[0], session_id[1]
        ));
        std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let socket = directory.join("control.sock");
        let setup = (|| {
            let listener = UnixListener::bind(&socket)?;
            std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
            listener.set_nonblocking(true)?;
            let (wake_tx, wake_rx) = UnixStream::pair()?;
            wake_tx.set_nonblocking(true)?;
            wake_rx.set_nonblocking(true)?;
            let wake = Arc::new(wake_tx);
            let view = Arc::new(Mutex::new(View {
                catalog: Arc::new(ControlCatalog {
                    generation: 1,
                    commands: Vec::new(),
                }),
                excluded: Vec::new(),
            }));
            let stop = Arc::new(AtomicBool::new(false));
            let (tx, requests) = sync_channel(CONTROL_MAX_PENDING);
            let owner_wake = sophia_wake::WakeSlot::default();
            let thread_owner_wake = owner_wake.clone();
            let thread_view = view.clone();
            let thread_stop = stop.clone();
            let thread_wake = wake.clone();
            let thread = std::thread::Builder::new()
                .name("sophia-control-v1".into())
                .spawn(move || {
                    // Dropped when the worker returns, which rings the owner
                    // after the queue is already disconnected.
                    let requests = sophia_wake::SignalSender::new(tx, thread_owner_wake.clone());
                    transport::run(
                        listener,
                        wake_rx,
                        thread_wake,
                        domain,
                        session_id,
                        thread_view,
                        thread_stop,
                        requests,
                        thread_owner_wake,
                    );
                })?;
            Ok(Self {
                directory: directory.clone(),
                socket: socket.clone(),
                view,
                requests,
                stop,
                wake,
                owner_wake,
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
    /// Returns false under brief contention; the owner retries the publication
    /// next turn and continues rejecting old generations independently.
    pub fn publish(&self, catalog: Arc<ControlCatalog>, excluded: &[u32]) -> bool {
        if excluded.len() > 32 || sophia_protocol::validate_control_catalog(&catalog).is_err() {
            return false;
        }
        let Ok(mut view) = self.view.try_lock() else {
            return false;
        };
        view.catalog = catalog;
        view.excluded.clear();
        view.excluded.extend(excluded.iter());
        drop(view);
        wake(&self.wake);
        true
    }
    pub fn try_request(&self) -> Option<ControlTicket> {
        self.requests.try_recv().ok()
    }
    /// Installs the owner's wake. Installation rings once, so a ticket queued
    /// before the owner attached is not left for the next one.
    pub fn set_owner_wake(&self, notifier: sophia_wake::Notifier) {
        self.owner_wake.set(notifier);
    }
    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }
}

use std::os::unix::fs::DirBuilderExt;

impl Drop for ControlService {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        wake(&self.wake);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_dir(&self.directory);
    }
}
