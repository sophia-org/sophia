use std::collections::BTreeMap;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveSeatEvent {
    Enable,
    Disable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveSeatState {
    Active,
    ReleasePending,
    Suspended,
    AcquirePending,
    Failed,
}

impl LiveSeatState {
    pub const fn observe(self, event: LiveSeatEvent) -> Self {
        match (self, event) {
            (Self::Active, LiveSeatEvent::Disable) => Self::ReleasePending,
            (Self::Suspended, LiveSeatEvent::Enable) => Self::AcquirePending,
            (Self::Active, LiveSeatEvent::Enable)
            | (Self::Suspended, LiveSeatEvent::Disable)
            | (Self::ReleasePending, LiveSeatEvent::Disable)
            | (Self::AcquirePending, LiveSeatEvent::Enable) => self,
            _ => Self::Failed,
        }
    }

    pub const fn released(self) -> Self {
        if matches!(self, Self::ReleasePending) {
            Self::Suspended
        } else {
            Self::Failed
        }
    }

    pub const fn acquired(self) -> Self {
        if matches!(self, Self::AcquirePending) {
            Self::Active
        } else {
            Self::Failed
        }
    }
}

/// A disable request never disposes of device owners on the caller's behalf.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveSeatDisableOutcome {
    Pending { leases: usize },
    Acknowledged,
}

fn try_disable(
    leases: usize,
    disable: impl FnOnce() -> Result<(), String>,
) -> Result<LiveSeatDisableOutcome, String> {
    if leases != 0 {
        return Ok(LiveSeatDisableOutcome::Pending { leases });
    }
    disable()?;
    Ok(LiveSeatDisableOutcome::Acknowledged)
}

enum LiveSeatCommand {
    Open(PathBuf, SyncSender<Result<(u64, OwnedFd), String>>),
    Close(u64),
    Switch(u8, SyncSender<Result<(), String>>),
    Disable(SyncSender<Result<LiveSeatDisableOutcome, String>>),
    Shutdown,
}

#[derive(Clone)]
pub struct LiveSeatDeviceOpener {
    name: String,
    commands: Sender<LiveSeatCommand>,
}

#[derive(Debug)]
pub struct LiveSeatDevice {
    fd: OwnedFd,
    lease: Arc<LiveSeatLease>,
}

#[derive(Debug)]
struct LiveSeatLease {
    token: u64,
    commands: Sender<LiveSeatCommand>,
}

impl LiveSeatDeviceOpener {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn open(&self, path: &Path) -> Result<LiveSeatDevice, String> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.commands
            .send(LiveSeatCommand::Open(path.to_owned(), reply_tx))
            .map_err(|_| "libseat broker stopped before device open".to_owned())?;
        let (token, fd) = reply_rx
            .recv()
            .map_err(|_| "libseat broker dropped device-open reply".to_owned())??;
        Ok(LiveSeatDevice {
            fd,
            lease: Arc::new(LiveSeatLease {
                token,
                commands: self.commands.clone(),
            }),
        })
    }
}

impl LiveSeatDevice {
    pub fn try_clone(&self) -> std::io::Result<Self> {
        Ok(Self {
            fd: duplicate_cloexec(&self.fd)?,
            lease: Arc::clone(&self.lease),
        })
    }

    pub fn try_clone_file(&self) -> std::io::Result<std::fs::File> {
        Ok(duplicate_cloexec(&self.fd)?.into())
    }

    pub fn duplicate_owned_fd(&self) -> std::io::Result<OwnedFd> {
        duplicate_cloexec(&self.fd)
    }
}

fn duplicate_cloexec(fd: impl AsFd) -> std::io::Result<OwnedFd> {
    // Seat authority must not cross an application or WM exec boundary.
    rustix::io::fcntl_dupfd_cloexec(fd, 0).map_err(Into::into)
}

impl AsFd for LiveSeatDevice {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

impl Drop for LiveSeatLease {
    fn drop(&mut self) {
        let _ = self.commands.send(LiveSeatCommand::Close(self.token));
    }
}

pub struct LiveSeatController {
    name: String,
    commands: Sender<LiveSeatCommand>,
    events: Receiver<LiveSeatEvent>,
    worker: Option<JoinHandle<()>>,
}

impl LiveSeatController {
    pub fn open() -> Result<Self, String> {
        let (commands_tx, commands_rx) = mpsc::channel();
        let (events_tx, events_rx) = mpsc::channel();
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("sophia-libseat".to_owned())
            .spawn(move || run_broker(commands_rx, events_tx, startup_tx))
            .map_err(|error| format!("libseat broker spawn failed: {error}"))?;
        let name = startup_rx
            .recv()
            .map_err(|_| "libseat broker stopped during startup".to_owned())??;
        Ok(Self {
            name,
            commands: commands_tx,
            events: events_rx,
            worker: Some(worker),
        })
    }

    pub fn device_opener(&self) -> LiveSeatDeviceOpener {
        LiveSeatDeviceOpener {
            name: self.name.clone(),
            commands: self.commands.clone(),
        }
    }

    pub fn name(&mut self) -> String {
        self.name.clone()
    }

    pub fn dispatch(&mut self) -> Result<Option<LiveSeatEvent>, String> {
        Ok(self.events.try_recv().ok())
    }

    pub fn switch_session(&mut self, terminal: u8) -> Result<(), String> {
        self.request(|reply| LiveSeatCommand::Switch(terminal, reply))
    }

    pub fn acknowledge_disable(&mut self) -> Result<LiveSeatDisableOutcome, String> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.commands
            .send(LiveSeatCommand::Disable(reply_tx))
            .map_err(|_| "libseat broker stopped before disable".to_owned())?;
        reply_rx
            .recv()
            .map_err(|_| "libseat broker dropped disable reply".to_owned())?
    }

    fn request(
        &self,
        command: impl FnOnce(SyncSender<Result<(), String>>) -> LiveSeatCommand,
    ) -> Result<(), String> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.commands
            .send(command(reply_tx))
            .map_err(|_| "libseat broker stopped before request".to_owned())?;
        reply_rx
            .recv()
            .map_err(|_| "libseat broker dropped request reply".to_owned())?
    }
}

impl Drop for LiveSeatController {
    fn drop(&mut self) {
        let _ = self.commands.send(LiveSeatCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_broker(
    commands: Receiver<LiveSeatCommand>,
    events: Sender<LiveSeatEvent>,
    startup: SyncSender<Result<String, String>>,
) {
    let callback_events = events.clone();
    let mut seat = match libseat::Seat::open(move |_seat, event| {
        let event = match event {
            libseat::SeatEvent::Enable => LiveSeatEvent::Enable,
            libseat::SeatEvent::Disable => LiveSeatEvent::Disable,
        };
        let _ = callback_events.send(event);
    }) {
        Ok(seat) => seat,
        Err(error) => {
            let _ = startup.send(Err(format!("libseat open failed: {error}")));
            return;
        }
    };
    let name = seat.name().to_owned();
    if startup.send(Ok(name)).is_err() {
        return;
    }
    let mut devices = BTreeMap::new();
    let mut next_token = 1u64;
    loop {
        let _ = seat.dispatch(0);
        match commands.recv_timeout(Duration::from_millis(2)) {
            Ok(LiveSeatCommand::Open(path, reply)) => {
                let result = seat
                    .open_device(&path)
                    .map_err(|error| format!("libseat open {} failed: {error}", path.display()))
                    .and_then(|device| {
                        let fd = duplicate_cloexec(&device)
                            .map_err(|error| format!("libseat device dup failed: {error}"))?;
                        let token = next_token;
                        next_token = next_token.saturating_add(1);
                        devices.insert(token, device);
                        Ok((token, fd))
                    });
                let _ = reply.send(result);
            }
            Ok(LiveSeatCommand::Close(token)) => {
                if let Some(device) = devices.remove(&token) {
                    let _ = seat.close_device(device);
                }
            }
            Ok(LiveSeatCommand::Switch(terminal, reply)) => {
                let result = seat
                    .switch_session(i32::from(terminal))
                    .map_err(|error| format!("libseat switch to VT{terminal} failed: {error}"));
                let _ = reply.send(result);
            }
            Ok(LiveSeatCommand::Disable(reply)) => {
                let result = try_disable(devices.len(), || {
                    seat.disable()
                        .map_err(|error| format!("libseat disable acknowledgement failed: {error}"))
                });
                let _ = reply.send(result);
            }
            Ok(LiveSeatCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    for (_, device) in devices {
        let _ = seat.close_device(device);
    }
}

#[cfg(test)]
#[path = "../tests/support/seat_disable.rs"]
mod disable_tests;
