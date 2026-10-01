//! Bounded worker handoff for the output file role. Physical topology
//! preparation, effects and rollback remain with the Session owner.
use std::collections::VecDeque;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::{
    AdmittedOutputProposal, OutputFileAssignee, OutputFileServiceEvent, OutputFileSubmission,
    OutputFileTransport, OutputFileTransportError,
};
use sophia_9p::Errno;
use sophia_protocol::{OutputAuthoritySnapshot, OutputV1Outcome, TransactionId};

const HANDOFF_CAPACITY: usize = 8;

#[derive(Debug)]
pub enum OutputFileServiceCommand {
    /// Test-owner path: retain the direct child unreaped until the worker
    /// acknowledges replacement. Protected peers use ReplaceSupervisedProcess.
    ReplaceSupervisedPid(u32),
    /// Move the identity captured before queuing. Failure to apply a
    /// reassignment is terminal for the service and emits Failed; the Session
    /// owner then cancels its epoch. Capture errors never reach the worker.
    ReplaceSupervisedProcess(OutputFileAssignee),
    PublishSnapshot(OutputAuthoritySnapshot),
    Settle {
        transaction: TransactionId,
        outcome: OutputV1Outcome,
    },
}

pub struct OutputFileService {
    commands: SyncSender<OutputFileServiceCommand>,
    pause: SyncSender<SyncSender<Vec<AdmittedOutputProposal>>>,
    events: Receiver<OutputFileServiceEvent>,
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl OutputFileService {
    pub fn spawn(
        transport: OutputFileTransport,
        snapshot: OutputAuthoritySnapshot,
    ) -> Result<Self, std::io::Error> {
        snapshot
            .validate()
            .map_err(|error| std::io::Error::other(format!("output snapshot: {error:?}")))?;
        let (commands, incoming) = mpsc::sync_channel(HANDOFF_CAPACITY);
        let (pause, pauses) = mpsc::sync_channel(1);
        let (outgoing, events) = mpsc::sync_channel(HANDOFF_CAPACITY);
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let thread = std::thread::Builder::new()
            .name("sophia-output-files".into())
            .spawn(move || {
                let mut worker = Worker {
                    transport,
                    snapshot,
                    publish_pending: false,
                    paused: false,
                    pending: VecDeque::with_capacity(2),
                };
                if let Err(message) = worker.run(&incoming, &pauses, &outgoing, &stop) {
                    let _ = worker.transport.disconnect();
                    // Bounded sends preserve earlier owner events. On shutdown
                    // the stop flag always releases this retry loop.
                    let mut failed = OutputFileServiceEvent::Failed { message };
                    while !stop.load(Ordering::Acquire) {
                        match outgoing.try_send(failed) {
                            Ok(()) | Err(TrySendError::Disconnected(_)) => break,
                            Err(TrySendError::Full(event)) => failed = event,
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
                let _ = worker.transport.disconnect();
            })?;
        Ok(Self {
            commands,
            pause,
            events,
            stopped,
            thread: Some(thread),
        })
    }

    /// Full and disconnected queues return the command without taking custody.
    /// The caller can retain it within its own bound or degrade the role.
    pub fn command(
        &self,
        command: OutputFileServiceCommand,
    ) -> Result<(), OutputFileServiceCommand> {
        if let OutputFileServiceCommand::PublishSnapshot(snapshot) = &command
            && snapshot.validate().is_err()
        {
            return Err(command);
        }
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                TrySendError::Full(command) | TrySendError::Disconnected(command) => command,
            })
    }

    pub fn try_event(&self) -> Result<Option<OutputFileServiceEvent>, mpsc::TryRecvError> {
        match self.events.try_recv() {
            Ok(event) => Ok(Some(event)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn event_timeout(
        &self,
        timeout: Duration,
    ) -> Result<OutputFileServiceEvent, mpsc::RecvTimeoutError> {
        self.events.recv_timeout(timeout)
    }

    pub fn pause_acceptance(
        &self,
        timeout: Duration,
    ) -> Result<Vec<AdmittedOutputProposal>, &'static str> {
        let (reply, result) = mpsc::sync_channel(1);
        self.pause
            .try_send(reply)
            .map_err(|_| "output pause queue unavailable")?;
        result
            .recv_timeout(timeout)
            .map_err(|_| "output pause deadline expired")
    }

    /// Revoke a departed supervised process without blocking the Session
    /// turn. A queued pause already satisfies this request. The worker emits
    /// Disconnected after earlier events, so accepted work reaches cancellation.
    pub fn request_pause(&self) -> Result<(), &'static str> {
        let (reply, _result) = mpsc::sync_channel(1);
        match self.pause.try_send(reply) {
            Ok(()) | Err(TrySendError::Full(_)) => Ok(()),
            Err(TrySendError::Disconnected(_)) => Err("output pause worker disconnected"),
        }
    }
}

impl Drop for OutputFileService {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Worker {
    transport: OutputFileTransport,
    snapshot: OutputAuthoritySnapshot,
    publish_pending: bool,
    paused: bool,
    // One unsent event plus, at most, a disconnect while that event waits.
    pending: VecDeque<OutputFileServiceEvent>,
}

impl Worker {
    fn run(
        &mut self,
        commands: &Receiver<OutputFileServiceCommand>,
        pauses: &Receiver<SyncSender<Vec<AdmittedOutputProposal>>>,
        events: &SyncSender<OutputFileServiceEvent>,
        stopped: &AtomicBool,
    ) -> Result<(), String> {
        while !stopped.load(Ordering::Acquire) {
            if let Ok(reply) = pauses.try_recv() {
                self.paused = true;
                let epoch = self
                    .transport
                    .export()
                    .map(|export| export.admission().connection().connection_epoch());
                let abandoned = self
                    .transport
                    .disconnect()
                    .map_err(|error| error.to_string())?;
                let _ = reply.try_send(abandoned);
                if let Some(connection_epoch) = epoch {
                    self.pending
                        .push_back(OutputFileServiceEvent::Disconnected { connection_epoch });
                }
            }
            while let Some(event) = self.pending.pop_front() {
                match events.try_send(event) {
                    Ok(()) => {}
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
                    if self.pending.is_empty()
                        && let Some(delivery) = self.transport.take_delivery()
                    {
                        self.deliver(delivery);
                    }
                    if self.publish_pending
                        && self
                            .transport
                            .export()
                            .expect("connected")
                            .admission()
                            .connection()
                            .selected_capabilities()
                            != 0
                    {
                        self.publish().map_err(|error| error.to_string())?;
                    }
                }
            } else if !self.paused && self.pending.is_empty() && commands_drained {
                // A replacement bootstrap must include already queued commits.
                // Keep one-command fairness for live exports, but accept only
                // after observing an empty owner-command queue.
                match self.transport.poll_accept(&self.snapshot) {
                    Ok(true) => self.publish_pending = false,
                    Ok(false) => {}
                    Err(error) => {
                        self.pending
                            .push_back(OutputFileServiceEvent::ConnectionRejected {
                                message: error.to_string(),
                            })
                    }
                }
            }
            debug_assert!(self.pending.len() <= 2);
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }

    fn command(
        &mut self,
        command: OutputFileServiceCommand,
    ) -> Result<(), OutputFileTransportError> {
        match command {
            OutputFileServiceCommand::ReplaceSupervisedPid(pid) => {
                let abandoned = self.transport.disconnect()?;
                self.transport.authorize_supervised_pid(pid)?;
                self.paused = false;
                self.pending
                    .push_back(OutputFileServiceEvent::AssigneeReplaced {
                        connection_epoch: self.transport.next_epoch(),
                        abandoned,
                    });
            }
            OutputFileServiceCommand::ReplaceSupervisedProcess(assignee) => {
                let abandoned = self.transport.disconnect()?;
                self.transport.authorize_assignee(assignee)?;
                self.paused = false;
                self.pending
                    .push_back(OutputFileServiceEvent::AssigneeReplaced {
                        connection_epoch: self.transport.next_epoch(),
                        abandoned,
                    });
            }
            OutputFileServiceCommand::PublishSnapshot(snapshot) => {
                snapshot.validate().map_err(|_| Errno::EINVAL)?;
                self.snapshot = snapshot;
                self.publish_pending = true;
            }
            OutputFileServiceCommand::Settle {
                transaction,
                outcome,
            } => {
                let Some(export) = self.transport.export() else {
                    return Ok(());
                };
                let epoch = export.admission().connection().connection_epoch();
                if outcome.connection_epoch != 0 && outcome.connection_epoch < epoch {
                    return Ok(());
                }
                match self.transport.settle(transaction, outcome) {
                    Ok(Some(proposal)) => self
                        .pending
                        .push_back(OutputFileServiceEvent::Promoted(proposal)),
                    Ok(None) => {}
                    Err(error) => self.owner_error(error)?,
                }
            }
        }
        Ok(())
    }

    fn publish(&mut self) -> Result<(), OutputFileTransportError> {
        match self.transport.publish(&self.snapshot) {
            Ok(_) => self.publish_pending = false,
            Err(OutputFileTransportError::File(Errno::EAGAIN)) => {}
            Err(error) => self.owner_error(error)?,
        }
        Ok(())
    }

    fn owner_error(
        &mut self,
        error: OutputFileTransportError,
    ) -> Result<(), OutputFileTransportError> {
        // Owner operations also check deadlines. Expiry between worker turns
        // retires this connection; an invalid owner epoch remains an error.
        if matches!(error, OutputFileTransportError::File(Errno::ESTALE))
            && self
                .transport
                .export()
                .is_some_and(|export| export.is_revoked())
        {
            self.retire_connection()
        } else {
            Err(error)
        }
    }

    fn retire_connection(&mut self) -> Result<(), OutputFileTransportError> {
        let connection_epoch = self
            .transport
            .export()
            .expect("connected")
            .admission()
            .connection()
            .connection_epoch();
        self.transport.disconnect()?;
        // The owner cancels observed work by epoch. Transport custody can also
        // include queued proposals that never reached the physical owner.
        self.pending
            .push_back(OutputFileServiceEvent::Disconnected { connection_epoch });
        Ok(())
    }

    fn deliver(&mut self, delivery: OutputFileSubmission) {
        let event = match delivery {
            OutputFileSubmission::Replayed => return,
            OutputFileSubmission::Negotiated(welcome) => OutputFileServiceEvent::Connected {
                connection_epoch: welcome.connection_epoch,
            },
            OutputFileSubmission::Refused(reason) => OutputFileServiceEvent::ConnectionRejected {
                message: format!("output negotiation refused: {reason:?}"),
            },
            OutputFileSubmission::Proposal {
                proposal,
                admission,
            } => OutputFileServiceEvent::Proposal {
                proposal,
                admission,
            },
            OutputFileSubmission::Rejected(transaction) => {
                OutputFileServiceEvent::ProposalRejected {
                    transaction,
                    message: "output candidate failed semantic admission".into(),
                }
            }
        };
        self.pending.push_back(event);
    }
}

#[cfg(test)]
#[path = "../tests/support/output_file_worker.rs"]
mod tests;
