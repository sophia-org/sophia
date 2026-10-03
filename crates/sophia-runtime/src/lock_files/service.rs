//! The lock provider's worker thread: it turns the 9P transport, hands what
//! the provider submitted to Session as events, and applies Session's
//! commands. Both queues are bounded; Session's lock state never waits on
//! the provider.
use std::collections::VecDeque;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
};
use std::thread::JoinHandle;
use std::time::Duration;

use sophia_9p::Errno;
use sophia_protocol::lock_files::*;

use super::{LockFileAssignee, LockFileTransport, LockFileTransportError, LockInbound};

const HANDOFF_CAPACITY: usize = 32;

#[derive(Debug)]
pub enum LockFileServiceCommand {
    /// A replacement provider: the current connection ends and only this
    /// process may connect next, under a fresh epoch.
    ReplaceSupervisedProcess(LockFileAssignee),
    /// Test-owner path for a direct child kept unreaped by the caller.
    ReplaceSupervisedPid(u32),
    /// The lock object in force, published to the connected provider and
    /// handed to every later one.
    PublishLock(LockObject),
    Entry(LockEntry),
    Chord(LockChord),
    Permit {
        allocation_id: u64,
        demand_id: u64,
        expires_after: Duration,
    },
    Outcome(LockCandidateOutcome),
}

#[derive(Debug)]
pub enum LockFileServiceEvent {
    Connected {
        connection_epoch: u64,
        chords: Vec<LockChordRequest>,
    },
    Inbound {
        connection_epoch: u64,
        inbound: LockInbound,
    },
    Disconnected {
        connection_epoch: u64,
    },
    ConnectionRejected {
        message: String,
    },
    Failed {
        message: String,
    },
}

pub struct LockFileService {
    commands: SyncSender<LockFileServiceCommand>,
    events: Receiver<LockFileServiceEvent>,
    stopped: Arc<AtomicBool>,
    owner_wake: sophia_wake::WakeSlot,
    thread: Option<JoinHandle<()>>,
}

impl LockFileService {
    /// `lock` is the lock object in force; `reserved_chords` are the chords
    /// Session keeps for itself, refused to every provider.
    pub fn spawn(
        transport: LockFileTransport,
        lock: LockObject,
        reserved_chords: Vec<LockChordRequest>,
    ) -> Result<Self, std::io::Error> {
        lock.encode()
            .map_err(|error| std::io::Error::other(format!("lock object: {error:?}")))?;
        let (commands, incoming) = mpsc::sync_channel(HANDOFF_CAPACITY);
        let (outgoing, events) = mpsc::sync_channel(HANDOFF_CAPACITY);
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let owner_wake = sophia_wake::WakeSlot::default();
        let worker_wake = owner_wake.clone();
        let thread = std::thread::Builder::new()
            .name("sophia-lock-files".into())
            .spawn(move || {
                let mut worker = Worker {
                    transport,
                    lock,
                    reserved_chords,
                    publish_pending: false,
                    pending: VecDeque::new(),
                    owner_wake: worker_wake,
                };
                if let Err(message) = worker.run(&incoming, &outgoing, &stop) {
                    let _ = worker.transport.disconnect();
                    let mut failed = LockFileServiceEvent::Failed { message };
                    while !stop.load(Ordering::Acquire) {
                        match outgoing.try_send(failed) {
                            Ok(()) => {
                                worker.owner_wake.notify();
                                break;
                            }
                            Err(TrySendError::Disconnected(_)) => break,
                            Err(TrySendError::Full(event)) => failed = event,
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
                let _ = worker.transport.disconnect();
                drop(outgoing);
                worker.owner_wake.notify();
            })?;
        Ok(Self {
            commands,
            events,
            stopped,
            owner_wake,
            thread: Some(thread),
        })
    }

    /// Installs the owner's wake; installation rings once.
    pub fn set_owner_wake(&self, notifier: sophia_wake::Notifier) {
        self.owner_wake.set(notifier);
    }

    pub fn owner_wake_attached(&self) -> bool {
        self.owner_wake.attached()
    }

    /// A full or disconnected queue returns the command untaken. Session
    /// keeps its lock either way: the provider only renders.
    pub fn command(&self, command: LockFileServiceCommand) -> Result<(), LockFileServiceCommand> {
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                TrySendError::Full(command) | TrySendError::Disconnected(command) => command,
            })
    }

    pub fn try_event(&self) -> Result<Option<LockFileServiceEvent>, TryRecvError> {
        match self.events.try_recv() {
            Ok(event) => Ok(Some(event)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn event_timeout(
        &self,
        timeout: Duration,
    ) -> Result<LockFileServiceEvent, mpsc::RecvTimeoutError> {
        self.events.recv_timeout(timeout)
    }
}

impl Drop for LockFileService {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Worker {
    transport: LockFileTransport,
    lock: LockObject,
    reserved_chords: Vec<LockChordRequest>,
    publish_pending: bool,
    pending: VecDeque<LockFileServiceEvent>,
    owner_wake: sophia_wake::WakeSlot,
}

impl Worker {
    fn run(
        &mut self,
        commands: &Receiver<LockFileServiceCommand>,
        events: &SyncSender<LockFileServiceEvent>,
        stopped: &AtomicBool,
    ) -> Result<(), String> {
        while !stopped.load(Ordering::Acquire) {
            while let Some(event) = self.pending.pop_front() {
                match events.try_send(event) {
                    Ok(()) => self.owner_wake.notify(),
                    Err(TrySendError::Disconnected(_)) => return Ok(()),
                    Err(TrySendError::Full(event)) => {
                        self.pending.push_front(event);
                        break;
                    }
                }
            }
            let mut commands_drained = false;
            if self.pending.is_empty() {
                match commands.try_recv() {
                    Ok(command) => self.command(command).map_err(|error| error.to_string())?,
                    Err(TryRecvError::Empty) => commands_drained = true,
                    Err(TryRecvError::Disconnected) => return Ok(()),
                }
            }
            if self.transport.export().is_some() {
                if !self.transport.turn().map_err(|error| error.to_string())? {
                    self.retire_connection()
                        .map_err(|error| error.to_string())?;
                } else {
                    // Inbound waits while events are unsent, so the owner's
                    // queue bounds what the provider can have in flight.
                    if self.pending.is_empty()
                        && let Some(inbound) = self.transport.take_inbound()
                    {
                        self.deliver(inbound);
                    }
                    if self.publish_pending && self.negotiated() {
                        self.publish().map_err(|error| error.to_string())?;
                    }
                }
            } else if self.pending.is_empty() && commands_drained {
                // A replacement starts from the newest lock object.
                match self
                    .transport
                    .poll_accept(&self.lock, &self.reserved_chords)
                {
                    Ok(true) => self.publish_pending = false,
                    Ok(false) => {}
                    Err(error) => {
                        self.pending
                            .push_back(LockFileServiceEvent::ConnectionRejected {
                                message: error.to_string(),
                            })
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }

    fn epoch(&self) -> Option<u64> {
        self.transport
            .export()
            .map(|export| export.custody().epoch())
    }

    fn negotiated(&self) -> bool {
        self.transport
            .export()
            .is_some_and(|export| export.custody().is_negotiated())
    }

    fn command(&mut self, command: LockFileServiceCommand) -> Result<(), LockFileTransportError> {
        match command {
            LockFileServiceCommand::ReplaceSupervisedProcess(assignee) => {
                self.retire_connection()?;
                self.transport.authorize_assignee(assignee)?;
            }
            LockFileServiceCommand::ReplaceSupervisedPid(pid) => {
                self.retire_connection()?;
                self.transport.authorize_supervised_pid(pid)?;
            }
            LockFileServiceCommand::PublishLock(lock) => {
                lock.encode().map_err(|_| Errno::EINVAL)?;
                self.lock = lock;
                self.publish_pending = true;
            }
            // Everything below is owed only to a negotiated provider. A
            // provider that has gone, lags or never negotiated misses it,
            // which costs it nothing but its own picture.
            LockFileServiceCommand::Entry(entry) => {
                if self.negotiated() {
                    self.provider(|transport| transport.entry(entry).map(drop))?;
                }
            }
            LockFileServiceCommand::Chord(chord) => {
                if self.negotiated() {
                    self.provider(|transport| transport.chord(chord).map(drop))?;
                }
            }
            LockFileServiceCommand::Permit {
                allocation_id,
                demand_id,
                expires_after,
            } => {
                if self.negotiated() {
                    self.provider(|transport| {
                        transport
                            .permit(allocation_id, demand_id, expires_after)
                            .map(drop)
                    })?;
                }
            }
            LockFileServiceCommand::Outcome(outcome) => {
                if self.negotiated() {
                    self.provider(|transport| transport.outcome(outcome).map(drop))?;
                }
            }
        }
        Ok(())
    }

    /// An owner operation on the provider's connection. A stale answer for
    /// work that lapsed, a full journal, or a provider revoked meanwhile is
    /// not Session's failure.
    fn provider(
        &mut self,
        apply: impl FnOnce(&mut LockFileTransport) -> Result<(), LockFileTransportError>,
    ) -> Result<(), LockFileTransportError> {
        match apply(&mut self.transport) {
            Ok(()) => Ok(()),
            Err(LockFileTransportError::File(Errno::EAGAIN | Errno::EINVAL | Errno::EACCES)) => {
                Ok(())
            }
            Err(LockFileTransportError::File(Errno::ESTALE)) => {
                if self
                    .transport
                    .export()
                    .is_some_and(|export| export.is_revoked())
                {
                    self.retire_connection()?;
                }
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn publish(&mut self) -> Result<(), LockFileTransportError> {
        let lock = self.lock.clone();
        match self.transport.publish_lock(lock) {
            Ok(_) => self.publish_pending = false,
            // The provider's journal is full; it gets the newest object once
            // it reads and acknowledges.
            Err(LockFileTransportError::File(Errno::EAGAIN)) => {}
            Err(error) => self.provider(|_| Err(error))?,
        }
        Ok(())
    }

    fn retire_connection(&mut self) -> Result<(), LockFileTransportError> {
        let Some(connection_epoch) = self.epoch() else {
            return Ok(());
        };
        self.transport.disconnect()?;
        self.publish_pending = false;
        self.pending
            .push_back(LockFileServiceEvent::Disconnected { connection_epoch });
        Ok(())
    }

    fn deliver(&mut self, inbound: LockInbound) {
        let Some(connection_epoch) = self.epoch() else {
            return;
        };
        self.pending.push_back(match inbound {
            LockInbound::Negotiated { chords } => LockFileServiceEvent::Connected {
                connection_epoch,
                chords,
            },
            inbound => LockFileServiceEvent::Inbound {
                connection_epoch,
                inbound,
            },
        });
    }
}
