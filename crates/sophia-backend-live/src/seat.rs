use std::collections::BTreeMap;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::thread::JoinHandle;

use rustix::event::{PollFd, PollFlags};
use sophia_wake::{Notifier, Wake, WakeSlot};

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

/// Every way into the broker. The broker sleeps until its seat connection or
/// this wake is readable, so a command queued without the ring would wait for
/// the next seat event.
#[derive(Clone)]
struct LiveSeatCommands {
    sender: Option<Sender<LiveSeatCommand>>,
    wake: Notifier,
}

impl LiveSeatCommands {
    fn send(&self, command: LiveSeatCommand) -> Result<(), mpsc::SendError<LiveSeatCommand>> {
        self.sender
            .as_ref()
            .expect("live command sender")
            .send(command)?;
        self.wake.notify();
        Ok(())
    }
}

impl Drop for LiveSeatCommands {
    fn drop(&mut self) {
        drop(self.sender.take());
        self.wake.notify();
    }
}

impl std::fmt::Debug for LiveSeatCommands {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LiveSeatCommands")
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct LiveSeatDeviceOpener {
    name: String,
    gpu_admission: crate::LiveGpuAdmission,
    commands: LiveSeatCommands,
}

#[derive(Debug)]
pub struct LiveSeatDevice {
    fd: OwnedFd,
    lease: Arc<LiveSeatLease>,
}

#[derive(Debug)]
struct LiveSeatLease {
    token: u64,
    commands: LiveSeatCommands,
}

impl LiveSeatDeviceOpener {
    pub fn gpu_admission(&self) -> &crate::LiveGpuAdmission {
        &self.gpu_admission
    }

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
    gpu_admission: crate::LiveGpuAdmission,
    commands: LiveSeatCommands,
    events: Receiver<LiveSeatEvent>,
    /// Why the broker stopped, if it stopped on its own.
    failure: Receiver<String>,
    /// The owner's wake, rung after each seat event and when the broker stops.
    owner_wake: WakeSlot,
    worker: Option<JoinHandle<()>>,
}

impl LiveSeatController {
    pub fn open() -> Result<Self, String> {
        Self::open_with_gpu_admission(crate::LiveGpuAdmission::default())
    }

    pub fn open_with_gpu_admission(gpu_admission: crate::LiveGpuAdmission) -> Result<Self, String> {
        let wake = Wake::new().map_err(|error| format!("libseat broker wake failed: {error}"))?;
        let (commands_tx, commands_rx) = mpsc::channel();
        let commands = LiveSeatCommands {
            sender: Some(commands_tx),
            wake: wake.notifier(),
        };
        let (events_tx, events_rx) = mpsc::channel();
        let (failure_tx, failure_rx) = mpsc::sync_channel(1);
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let owner_wake = WakeSlot::default();
        let broker_owner_wake = owner_wake.clone();
        let worker = std::thread::Builder::new()
            .name("sophia-libseat".to_owned())
            .spawn(move || {
                run_broker(
                    commands_rx,
                    wake,
                    events_tx,
                    failure_tx,
                    broker_owner_wake,
                    startup_tx,
                );
            })
            .map_err(|error| format!("libseat broker spawn failed: {error}"))?;
        let name = startup_rx
            .recv()
            .map_err(|_| "libseat broker stopped during startup".to_owned())??;
        Ok(Self {
            name,
            gpu_admission,
            commands,
            events: events_rx,
            failure: failure_rx,
            owner_wake,
            worker: Some(worker),
        })
    }

    /// Installs the owner's wake. Installation rings once, so a seat event
    /// that arrived before the owner attached is not left for the next one.
    pub fn set_owner_wake(&self, notifier: Notifier) {
        self.owner_wake.set(notifier);
    }

    pub fn device_opener(&self) -> LiveSeatDeviceOpener {
        LiveSeatDeviceOpener {
            name: self.name.clone(),
            gpu_admission: self.gpu_admission.clone(),
            commands: self.commands.clone(),
        }
    }

    pub fn name(&mut self) -> String {
        self.name.clone()
    }

    /// Takes one seat event. A broker that stopped on its own is an error:
    /// nothing will serve the seat again.
    pub fn dispatch(&mut self) -> Result<Option<LiveSeatEvent>, String> {
        match self.events.try_recv() {
            Ok(event) => {
                // Rings coalesce, so one wake can cover two events and this
                // takes only one. Ring again rather than leave the other for
                // an unrelated wake; the cost is one empty dispatch.
                self.owner_wake.notify();
                Ok(Some(event))
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(self
                .failure
                .try_recv()
                .unwrap_or_else(|_| "libseat broker stopped".to_owned())),
        }
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
    wake: Wake,
    events: Sender<LiveSeatEvent>,
    failure: SyncSender<String>,
    owner_wake: WakeSlot,
    startup: SyncSender<Result<String, String>>,
) {
    let callback_events = events.clone();
    let callback_owner_wake = owner_wake.clone();
    let seat = match libseat::Seat::open(move |_seat, event| {
        let event = match event {
            libseat::SeatEvent::Enable => LiveSeatEvent::Enable,
            libseat::SeatEvent::Disable => LiveSeatEvent::Disable,
        };
        if callback_events.send(event).is_ok() {
            callback_owner_wake.notify();
        }
    }) {
        Ok(seat) => seat,
        Err(error) => {
            let _ = startup.send(Err(format!("libseat open failed: {error}")));
            return;
        }
    };
    let mut broker = LibseatBroker {
        seat,
        devices: BTreeMap::new(),
        next_token: 1,
    };
    let name = broker.seat.name().to_owned();
    if startup.send(Ok(name)).is_err() {
        return;
    }
    if let Err(error) = run_broker_loop(&mut broker, &commands, &wake) {
        let _ = failure.try_send(error);
    }
    broker.close_all();
    // The seat owns the callback's event sender. Both senders go before the
    // ring, so a woken owner finds the channel already disconnected.
    drop(broker);
    drop(events);
    owner_wake.notify();
}

/// What the broker loop needs from libseat. Tests drive the production loop
/// through this with a descriptor and replies they control.
trait SeatBrokerBackend {
    /// Runs callbacks for whatever the connection already holds; never blocks.
    fn dispatch_ready(&mut self) -> Result<(), String>;
    /// The connection descriptor, or a failure the owner must observe.
    fn connection(&mut self) -> Result<BorrowedFd<'_>, String>;
    /// Serves one command. The loop handles `Shutdown` itself.
    fn execute(&mut self, command: LiveSeatCommand);
}

struct LibseatBroker {
    seat: libseat::Seat,
    /// Each device with the descriptor libseat handed out, closed with it.
    devices: BTreeMap<u64, sophia_seat_device::SeatDevice>,
    next_token: u64,
}

impl LibseatBroker {
    fn close_all(&mut self) {
        for (_, device) in std::mem::take(&mut self.devices) {
            let _ = device.close(&mut self.seat);
        }
    }
}

impl SeatBrokerBackend for LibseatBroker {
    fn dispatch_ready(&mut self) -> Result<(), String> {
        self.seat
            .dispatch(0)
            .map(|_| ())
            .map_err(|error| format!("libseat dispatch failed: {error}"))
    }

    fn connection(&mut self) -> Result<BorrowedFd<'_>, String> {
        self.seat
            .get_fd()
            .map_err(|error| format!("libseat descriptor failed: {error}"))
    }

    fn execute(&mut self, command: LiveSeatCommand) {
        match command {
            LiveSeatCommand::Open(path, reply) => {
                let result = sophia_seat_device::SeatDevice::open(&mut self.seat, &path)
                    .map_err(|error| format!("libseat open {} failed: {error}", path.display()))
                    .and_then(|device| match duplicate_cloexec(&device) {
                        Ok(fd) => {
                            let token = self.next_token;
                            self.next_token = self.next_token.saturating_add(1);
                            self.devices.insert(token, device);
                            Ok((token, fd))
                        }
                        Err(error) => {
                            // Release the device on its seat as well as its
                            // descriptor; dropping it would do only the latter.
                            let _ = device.close(&mut self.seat);
                            Err(format!("libseat device dup failed: {error}"))
                        }
                    });
                let _ = reply.send(result);
            }
            LiveSeatCommand::Close(token) => {
                if let Some(device) = self.devices.remove(&token) {
                    let _ = device.close(&mut self.seat);
                }
            }
            LiveSeatCommand::Switch(terminal, reply) => {
                let result = self
                    .seat
                    .switch_session(i32::from(terminal))
                    .map_err(|error| format!("libseat switch to VT{terminal} failed: {error}"));
                let _ = reply.send(result);
            }
            LiveSeatCommand::Disable(reply) => {
                let seat = &mut self.seat;
                let result = try_disable(self.devices.len(), || {
                    seat.disable()
                        .map_err(|error| format!("libseat disable acknowledgement failed: {error}"))
                });
                let _ = reply.send(result);
            }
            LiveSeatCommand::Shutdown => {}
        }
    }
}

/// Serves commands and seat events until shutdown, sleeping without a
/// timeout in between. A lost seat connection is reported to the owner.
fn run_broker_loop(
    backend: &mut impl SeatBrokerBackend,
    commands: &Receiver<LiveSeatCommand>,
    wake: &Wake,
) -> Result<(), String> {
    loop {
        backend.dispatch_ready()?;
        let mut served = false;
        loop {
            match commands.try_recv() {
                Ok(LiveSeatCommand::Shutdown) | Err(TryRecvError::Disconnected) => return Ok(()),
                Ok(command) => {
                    backend.execute(command);
                    served = true;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        // A request can leave libseat holding events it read while waiting
        // for its own reply. Dispatch again before sleeping on the connection.
        if served {
            continue;
        }
        let readiness = wait_for_seat(backend.connection()?, wake.as_fd())?;
        if readiness.connection_closed {
            return Err("libseat connection closed while waiting".to_owned());
        }
        if readiness.commands {
            // Cleared before the queue is read again, so a command sent after
            // that read stays readable.
            wake.clear()
                .map_err(|error| format!("libseat broker wake failed: {error}"))?;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SeatWaitReadiness {
    commands: bool,
    connection_closed: bool,
}

fn wait_for_seat(
    connection: BorrowedFd<'_>,
    commands: BorrowedFd<'_>,
) -> Result<SeatWaitReadiness, String> {
    let failed = PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL;
    let mut fds = [
        PollFd::new(&commands, PollFlags::IN),
        PollFd::new(&connection, PollFlags::IN),
    ];
    wait_untimed(&mut fds)?;
    Ok(SeatWaitReadiness {
        commands: fds[0].revents().intersects(PollFlags::IN | failed),
        connection_closed: fds[1].revents().intersects(failed),
    })
}

fn wait_untimed(fds: &mut [PollFd<'_>]) -> Result<(), String> {
    sophia_wake::wait(fds, None)
        .map(|_| ())
        .map_err(|error| format!("libseat broker wait failed: {error}"))
}

#[cfg(test)]
#[path = "../tests/support/seat_disable.rs"]
mod disable_tests;

#[cfg(test)]
#[path = "../tests/support/seat_broker_wake.rs"]
mod broker_wake_tests;
