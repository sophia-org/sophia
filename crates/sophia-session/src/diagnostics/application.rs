//! Private application diagnostics. Process execution never depends on storage progress.
mod collector;
mod records;

use super::{Stamp, storage::Directory};
use std::fs::File;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Duration;

pub(super) use records::read_records;
pub use records::{ApplicationRecords, escape_bytes};

const STREAMS: usize = 64;
const LAUNCH_BYTES: u64 = 1024 * 1024;
const PACKET_BYTES: usize = 4096;
const QUEUED_PACKETS: usize = (1024 * 1024) / std::mem::size_of::<records::Packet>();
static ACTIVE: OnceLock<Arc<Shared>> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
pub enum LaunchSource {
    Startup,
    Shortcut,
    Catalog,
}

impl LaunchSource {
    fn name(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Shortcut => "shortcut",
            Self::Catalog => "catalog",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LaunchContext {
    pub source: LaunchSource,
    pub transaction: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaunchTicket(u64);

struct Launch {
    id: u64,
    requested: Stamp,
    context: LaunchContext,
    executable: Vec<u8>,
    reader: Option<File>,
    pid: Option<u32>,
    spawn: &'static str,
    spawn_reason: &'static str,
    spawned: Option<Stamp>,
    exited: Option<Stamp>,
    exit: Option<(Option<i32>, Option<i32>)>,
    read: u64,
    queued: u64,
    dropped: u64,
    eof: bool,
    incomplete: bool,
    retain: bool,
    dirty: bool,
}

struct Shared {
    launches: Mutex<Vec<Launch>>,
    enabled: AtomicBool,
    stop: AtomicBool,
    next: AtomicU64,
    refused: AtomicU64,
    storage_errors: AtomicU64,
    storage_dropped: AtomicU64,
    metadata_lost: AtomicU64,
}

pub struct ApplicationCapture {
    shared: Arc<Shared>,
    finished: mpsc::Receiver<()>,
}

impl ApplicationCapture {
    /// Creates independent collectors; installation is separate to keep fixtures isolated.
    pub fn start(path: &Path) -> io::Result<Self> {
        let directory = Directory::open(path, false)?;
        let shared = Arc::new(Shared {
            launches: Mutex::new(Vec::with_capacity(STREAMS)),
            enabled: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            next: AtomicU64::new(1),
            refused: AtomicU64::new(0),
            storage_errors: AtomicU64::new(0),
            storage_dropped: AtomicU64::new(0),
            metadata_lost: AtomicU64::new(0),
        });
        let (sender, receiver) = mpsc::sync_channel(QUEUED_PACKETS);
        let (finished_tx, finished) = mpsc::sync_channel(1);
        let storage = shared.clone();
        std::thread::Builder::new()
            .name("app-stderr-store".into())
            .spawn(move || {
                records::store(directory, receiver, &storage);
                let _ = finished_tx.send(());
            })?;
        let collector = shared.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("app-stderr-drain".into())
            .spawn(move || collector::drain(collector, sender))
        {
            shared.stop.store(true, Ordering::Release);
            return Err(error);
        }
        Ok(Self { shared, finished })
    }

    pub fn install(&self) -> io::Result<()> {
        ACTIVE
            .set(self.shared.clone())
            .map_err(|_| io::Error::other("application capture already installed"))
    }

    pub fn set_enabled(&self, enabled: bool) {
        set_shared_enabled(&self.shared, enabled);
    }

    pub fn spawn(&self, command: &mut Command, context: LaunchContext) -> io::Result<Child> {
        spawn_recorded(&self.shared, command, context)
    }

    pub fn registration(&self, pid: u32) -> Option<LaunchTicket> {
        registration_in(&self.shared, pid)
    }

    pub fn exited(&self, ticket: Option<LaunchTicket>, status: ExitStatus) {
        record_exit(&self.shared, ticket, status);
    }
}

impl Drop for ApplicationCapture {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        // A descendant holding stderr or a blocked filesystem must not hold logout.
        if self
            .finished
            .recv_timeout(Duration::from_millis(250))
            .is_err()
        {
            super::capture_line(
                "sophia_application_capture schema=1 status=incomplete reason=shutdown_timeout",
            );
        }
    }
}

pub fn set_enabled(enabled: bool) {
    if let Some(shared) = ACTIVE.get() {
        set_shared_enabled(shared, enabled);
    }
}

fn set_shared_enabled(shared: &Shared, enabled: bool) {
    shared.enabled.store(enabled, Ordering::Release);
    if !enabled && let Ok(mut launches) = shared.launches.lock() {
        for launch in launches.iter_mut() {
            launch.retain = false;
            launch.dirty = true;
        }
    }
}

/// Without a recorded daily session, preserve the caller's proof/stdout/stderr contract.
pub fn spawn(command: &mut Command, context: LaunchContext) -> io::Result<Child> {
    match ACTIVE.get() {
        Some(shared) => spawn_recorded(shared, command, context),
        None => command.spawn(),
    }
}

pub fn registration(pid: u32) -> Option<LaunchTicket> {
    ACTIVE.get().and_then(|shared| registration_in(shared, pid))
}

fn registration_in(shared: &Shared, pid: u32) -> Option<LaunchTicket> {
    shared
        .launches
        .lock()
        .ok()?
        .iter()
        .find(|launch| launch.pid == Some(pid) && launch.exit.is_none())
        .map(|launch| LaunchTicket(launch.id))
}

pub fn exited(ticket: Option<LaunchTicket>, status: ExitStatus) {
    if let Some(shared) = ACTIVE.get() {
        record_exit(shared, ticket, status);
    }
}

fn record_exit(shared: &Shared, ticket: Option<LaunchTicket>, status: ExitStatus) {
    let Some(ticket) = ticket else { return };
    if let Ok(mut launches) = shared.launches.lock()
        && let Some(launch) = launches
            .iter_mut()
            .find(|item| item.id == ticket.0 && item.exit.is_none())
    {
        // The child owner captures this ticket immediately after spawn. Repeated
        // waits or PID reuse cannot attribute an old child's exit to a new launch.
        launch.exit = Some((status.code(), status.signal()));
        launch.exited = Some(Stamp::now());
        launch.dirty = true;
        super::capture_line(&format!(
            "sophia_application_launch schema=1 status=exited launch_id={} exit_code={} exit_signal={}",
            launch.id,
            status.code().unwrap_or(0),
            status.signal().unwrap_or(0)
        ));
    }
}

fn spawn_recorded(
    shared: &Shared,
    command: &mut Command,
    context: LaunchContext,
) -> io::Result<Child> {
    let id = shared
        .next
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .ok();
    let prepared = (|| -> io::Result<(u64, Option<std::os::fd::OwnedFd>)> {
        let id = id.ok_or_else(|| io::Error::other("launch identity exhausted"))?;
        if shared.stop.load(Ordering::Acquire) {
            return Err(io::Error::other("capture stopped"));
        }
        let executable = command.get_program().as_bytes();
        if executable.len() > 768 {
            return Err(io::Error::other("executable identity too long"));
        }
        let (reader, writer) = rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC)?;
        rustix::fs::fcntl_setfl(&reader, rustix::fs::OFlags::NONBLOCK)?;
        let mut launches = shared
            .launches
            .lock()
            .map_err(|_| io::Error::other("capture lock poisoned"))?;
        if launches.len() == STREAMS {
            return Err(io::Error::other("capture stream capacity"));
        }
        let retain = shared.enabled.load(Ordering::Acquire);
        launches.push(Launch {
            id,
            requested: Stamp::now(),
            context,
            executable: executable.to_vec(),
            reader: retain.then(|| File::from(reader)),
            pid: None,
            spawn: "pending",
            exit: None,
            spawn_reason: "none",
            spawned: None,
            exited: None,
            read: 0,
            queued: 0,
            dropped: 0,
            eof: !retain,
            incomplete: false,
            retain,
            dirty: true,
        });
        Ok((id, retain.then_some(writer)))
    })();
    let Ok((id, writer)) = prepared else {
        shared.refused.fetch_add(1, Ordering::Relaxed);
        super::capture_line(&format!(
            "sophia_application_capture schema=1 status=unavailable launch_id={} reason=capture_unavailable",
            id.unwrap_or(0)
        ));
        // A diagnostic refusal must not turn a healthy application into a failed spawn.
        return command.spawn();
    };
    let piped = writer.is_some();
    if let Some(writer) = writer {
        command.stderr(Stdio::from(writer));
    }
    let result = command.spawn();
    // Command otherwise retains the pipe writer and would falsely prevent EOF.
    if piped {
        command.stderr(Stdio::inherit());
    }
    if let Ok(mut launches) = shared.launches.lock()
        && let Some(launch) = launches.iter_mut().find(|launch| launch.id == id)
    {
        launch.spawn = if result.is_ok() { "spawned" } else { "failed" };
        launch.spawned = Some(Stamp::now());
        launch.spawn_reason = result
            .as_ref()
            .err()
            .map_or("none", |error| match error.kind() {
                io::ErrorKind::NotFound => "not_found",
                io::ErrorKind::PermissionDenied => "permission_denied",
                io::ErrorKind::WouldBlock => "resource_limit",
                _ => "spawn_failure",
            });
        launch.pid = result.as_ref().ok().map(Child::id);
        launch.dirty = true;
        super::capture_line(&format!(
            "sophia_application_launch schema=1 status={} launch_id={} transaction={} reason={}",
            launch.spawn,
            launch.id,
            context.transaction.unwrap_or(0),
            launch.spawn_reason
        ));
    }
    result
}
