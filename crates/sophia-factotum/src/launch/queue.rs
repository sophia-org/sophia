//! Bounded ingress to Session's existing Factotum connection owner.
//!
//! These are trusted owner commands, never a child-facing API. A reservation
//! covers queued work, an exchange, its unread result and any cancellation
//! cleanup. Dropping a ticket cannot leave an unbounded late-result queue.

use super::wire::{Command, Response};
use crate::client::{ClientError, SessionClient};
use sophia_9p::Errno;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
    },
    time::{Duration, Instant},
};

pub const CAPACITY: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Capacity,
    Deadline,
    Closed,
    Remote(Errno),
}

struct Shared {
    reserved: AtomicUsize,
    closed: AtomicBool,
}
struct State {
    cancelled: AtomicBool,
    consumed: AtomicBool,
}

#[derive(Clone)]
pub struct Ingress {
    sender: SyncSender<Work>,
    shared: Arc<Shared>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

pub struct Ticket {
    result: Receiver<Result<Response, Error>>,
    state: Arc<State>,
    deadline: Instant,
    done: bool,
    shared: Arc<Shared>,
}

struct Work {
    command: Command,
    deadline: Instant,
    state: Arc<State>,
    result: SyncSender<Result<Response, Error>>,
    shared: Arc<Shared>,
    cleanup: bool,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl Drop for Work {
    fn drop(&mut self) {
        self.shared.reserved.fetch_sub(1, Ordering::AcqRel);
        // Also wake a pending read on closure/cancellation, when no response
        // was published. Ticket::take then observes the disconnected sender.
        if let Some(wake) = &self.wake {
            wake();
        }
    }
}

pub struct Owner {
    receiver: Receiver<Work>,
    shared: Arc<Shared>,
    waiting: Option<Work>,
    active: Option<Work>,
    completed: Vec<Work>,
}

/// Dropping a connected owner also drops the real transport. A detached queue
/// can never leave its issued commands or remote delegations running behind it.
pub struct ConnectionOwner {
    queue: Owner,
    client: SessionClient,
}
impl ConnectionOwner {
    pub fn poll(&mut self, now: Instant) -> Result<(), ClientError> {
        self.queue.poll(&mut self.client, now)
    }
    /// The same owner advances PAM through this client, never a second stream.
    pub fn client_mut(&mut self) -> &mut SessionClient {
        &mut self.client
    }
}
impl Drop for ConnectionOwner {
    fn drop(&mut self) {
        self.client.close();
    }
}

pub fn channel() -> (Ingress, Owner) {
    let (sender, receiver) = sync_channel(CAPACITY);
    let shared = Arc::new(Shared {
        reserved: AtomicUsize::new(0),
        closed: AtomicBool::new(false),
    });
    (
        Ingress {
            sender,
            shared: Arc::clone(&shared),
            wake: None,
        },
        Owner {
            receiver,
            shared,
            waiting: None,
            active: None,
            completed: Vec::with_capacity(CAPACITY),
        },
    )
}

impl Ingress {
    pub fn is_open(&self) -> bool {
        !self.shared.closed.load(Ordering::Acquire)
    }
    /// Trusted Session owner failure. Results become unusable immediately;
    /// the connection owner closes the actual stream on its next bounded turn.
    pub fn invalidate(&self) {
        self.shared.closed.store(true, Ordering::Release);
    }
    pub fn with_wake(mut self, wake: impl Fn() + Send + Sync + 'static) -> Self {
        self.wake = Some(Arc::new(wake));
        self
    }
    /// No wait, unbounded queue, or allocation before capacity is reserved.
    pub fn submit(
        &self,
        command: Command,
        now: Instant,
        deadline: Instant,
    ) -> Result<Ticket, Error> {
        if self.shared.closed.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        if deadline <= now {
            return Err(Error::Deadline);
        }
        self.shared
            .reserved
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < CAPACITY).then_some(n + 1)
            })
            .map_err(|_| Error::Capacity)?;
        let (sender, result) = sync_channel(1);
        let state = Arc::new(State {
            cancelled: AtomicBool::new(false),
            consumed: AtomicBool::new(false),
        });
        let work = Work {
            command,
            deadline,
            state: Arc::clone(&state),
            result: sender,
            shared: Arc::clone(&self.shared),
            cleanup: false,
            wake: self.wake.clone(),
        };
        self.sender.try_send(work).map_err(|_| Error::Closed)?;
        Ok(Ticket {
            result,
            state,
            deadline,
            done: false,
            shared: Arc::clone(&self.shared),
        })
    }
}

impl Ticket {
    pub fn take(&mut self, now: Instant) -> Result<Option<Response>, Error> {
        if self.done || self.shared.closed.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        if now >= self.deadline {
            self.state.cancelled.store(true, Ordering::Release);
            self.done = true;
            return Err(Error::Deadline);
        }
        let result = match self.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return Ok(None),
            Err(TryRecvError::Disconnected) => Err(Error::Closed),
        };
        self.state.consumed.store(true, Ordering::Release);
        self.done = true;
        result.map(Some)
    }
}
impl Drop for Ticket {
    fn drop(&mut self) {
        if !self.done {
            self.state.cancelled.store(true, Ordering::Release);
        }
    }
}

impl Owner {
    pub fn connect(self, client: SessionClient) -> ConnectionOwner {
        ConnectionOwner {
            queue: self,
            client,
        }
    }
    /// Call only on the shared connection owner thread. One exchange can be
    /// advanced per turn; PAM tags continue to be dispatched by SessionClient.
    fn poll(&mut self, client: &mut SessionClient, now: Instant) -> Result<(), ClientError> {
        let result = self.progress(client, now);
        if result.is_err() {
            self.shared.closed.store(true, Ordering::Release);
            self.waiting = None;
            self.active = None;
            self.completed.clear();
            while self.receiver.try_recv().is_ok() {}
        }
        result
    }

    fn progress(&mut self, client: &mut SessionClient, now: Instant) -> Result<(), ClientError> {
        if self.shared.closed.load(Ordering::Acquire) {
            client.close();
            return Err(ClientError::Closed);
        }
        client.poll(now)?;
        if self.active.is_some()
            && let Some(result) = client.take_launch(now)?
        {
            let mut work = self.active.take().expect("active exchange");
            if work.cleanup {
                // An acknowledged Cancel/Retire ends remote custody. A remote
                // refusal must fail closed rather than freeing an uncertain slot.
                if !matches!(result, Ok(Response::Done)) {
                    client.close();
                    return Err(ClientError::Unexpected("launch cancellation"));
                }
            } else {
                work.cleanup = result.is_ok() && cleanup(work.command).is_some();
                let _ = work.result.try_send(result.map_err(Error::Remote));
                if let Some(wake) = &work.wake {
                    wake();
                }
                self.completed.push(work);
            }
        }
        self.completed
            .retain(|w| !w.state.consumed.load(Ordering::Acquire));
        if self.waiting.is_none()
            && let Some(index) = self
                .completed
                .iter()
                .position(|w| w.state.cancelled.load(Ordering::Acquire) || now >= w.deadline)
        {
            let mut work = self.completed.swap_remove(index);
            if work.cleanup {
                work.command = cleanup(work.command).expect("cleanup command");
                // Cancellation gets its own bounded exchange, not a renewed
                // authentication deadline. No result can escape this ticket;
                // its reservation remains charged until remote cleanup answers.
                work.deadline = now + Duration::from_secs(5);
                self.waiting = Some(work);
            }
        }
        if self.active.is_some() {
            return Ok(());
        }
        if self.waiting.is_none() {
            self.waiting = self.receiver.try_recv().ok();
        }
        let Some(work) = self.waiting.as_ref() else {
            return Ok(());
        };
        if !work.cleanup && (work.state.cancelled.load(Ordering::Acquire) || now >= work.deadline) {
            let work = self.waiting.take().expect("waiting request");
            let _ = work.result.try_send(Err(Error::Deadline));
            return Ok(());
        }
        // Queue residence must not extend Factotum's attempt lifetime.
        let command = match work.command {
            Command::Begin {
                attempt,
                remaining_msec,
            } => Command::Begin {
                attempt,
                remaining_msec: remaining_msec.min(
                    u32::try_from(work.deadline.saturating_duration_since(now).as_millis())
                        .unwrap_or(u32::MAX),
                ),
            },
            command => command,
        };
        match client.submit_launch(command, now, work.deadline) {
            Ok(()) => self.active = self.waiting.take(),
            Err(ClientError::Launch(
                super::client::Error::Busy
                | super::client::Error::Transport(sophia_9p_client::pipeline::PipelineError::Limit(
                    _,
                )),
            )) => {}
            Err(error) => {
                client.close();
                return Err(error);
            }
        }
        Ok(())
    }
}

fn cleanup(command: Command) -> Option<Command> {
    match command {
        Command::Begin { attempt, .. } => Some(Command::Cancel { attempt }),
        Command::Reserve(delegation) | Command::ReserveAudiences { delegation, .. } => {
            Some(Command::Retire {
                launch: delegation.identity.launch,
            })
        }
        Command::Activate { launch } => Some(Command::Retire { launch }),
        _ => None,
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Release);
    }
}
