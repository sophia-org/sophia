//! Nonblocking private control exchange on a caller-owned 9P pipeline.
//!
//! The connection owner dispatches replies by tag, so a waiting PAM read need
//! not block launch control. This client never polls, waits, spawns or queues
//! owner requests. It holds one exchange, including its unconsumed result.
//! After a fatal error the owner must drop the shared pipeline and retire its
//! delegations: a timed-out write may already have changed Factotum's state.

use super::wire::{self, Command, Response};
use sophia_9p::{Errno, Fid, Tag};
use sophia_9p_client::pipeline::{Pipeline, PipelineError, Reply};
use std::time::Instant;

#[derive(Debug)]
pub enum Error {
    Transport(PipelineError),
    Remote(Errno),
    Unexpected,
    Deadline,
    Closed,
    Busy,
    ForeignTag,
}

enum State {
    Attach(Tag, Fid),
    WalkNext(Fid),
    Walk(Tag, Fid),
    OpenNext(Fid),
    Open(Tag, Fid),
    Ready(Fid),
    Write(Tag, Fid, Command, usize),
    ReadNext(Fid, Command),
    Read(Tag, Fid, Command),
    Complete(Fid, Result<Response, Errno>),
    Failed,
}

/// The private control fid lasts for the shared connection's lifetime. The
/// owner must not reset that connection or independently clunk this client's
/// fids while using it. No Debug implementation exposes a challenge.
pub struct LaunchClient {
    state: State,
    deadline: Option<Instant>,
}

impl LaunchClient {
    /// Queue the private attach on an already version-negotiated pipeline.
    /// Version negotiation belongs off the Session owner loop.
    pub fn start(pipeline: &mut Pipeline, now: Instant, deadline: Instant) -> Result<Self, Error> {
        if now >= deadline {
            return Err(Error::Deadline);
        }
        let (tag, root) = pipeline
            .attach(b"", wire::ATTACH)
            .map_err(Error::Transport)?;
        Ok(Self {
            state: State::Attach(tag, root),
            deadline: Some(deadline),
        })
    }

    pub fn ready(&self) -> bool {
        matches!(self.state, State::Ready(_))
    }

    /// Only this tag may be passed to `receive`. Other replies remain the
    /// connection owner's responsibility (including PAM and flush replies).
    pub fn owns(&self, tag: Tag) -> bool {
        match self.state {
            State::Attach(t, _)
            | State::Walk(t, _)
            | State::Open(t, _)
            | State::Write(t, ..)
            | State::Read(t, ..) => t == tag,
            _ => false,
        }
    }

    /// Admit one command. Busy and pipeline capacity refusals leave it entirely
    /// with the caller. The result must be consumed before another command.
    /// `deadline` is the original owner deadline, never renewed by progress.
    pub fn submit(
        &mut self,
        pipeline: &mut Pipeline,
        mut command: Command,
        now: Instant,
        deadline: Instant,
    ) -> Result<(), Error> {
        self.check(now)?;
        let State::Ready(rpc) = self.state else {
            return Err(Error::Busy);
        };
        if now >= deadline {
            return Err(Error::Deadline);
        }
        if let Command::Begin { remaining_msec, .. } = &mut command {
            // Floor rather than extend a sub-millisecond remaining budget.
            *remaining_msec = (*remaining_msec)
                .min(u32::try_from(deadline.duration_since(now).as_millis()).unwrap_or(u32::MAX));
            if *remaining_msec == 0 {
                return Err(Error::Deadline);
            }
        }
        let bytes = command.encode();
        let tag = match pipeline.write(rpc, 0, &bytes) {
            Ok(tag) => tag,
            Err(error @ PipelineError::Limit(_)) => return Err(Error::Transport(error)),
            Err(error) => return self.fail(Error::Transport(error)),
        };
        self.state = State::Write(tag, rpc, command, bytes.len());
        self.deadline = Some(deadline);
        Ok(())
    }

    /// Queue at most one next request. A pipeline limit is retriable without
    /// losing the next step; no loop or socket wait occurs here.
    pub fn advance(&mut self, pipeline: &mut Pipeline, now: Instant) -> Result<(), Error> {
        self.check(now)?;
        let next = match self.state {
            State::WalkNext(root) => pipeline
                .walk(root, &[b"rpc"])
                .map(|(tag, rpc)| State::Walk(tag, rpc)),
            State::OpenNext(rpc) => pipeline.lopen(rpc, 2).map(|tag| State::Open(tag, rpc)),
            State::ReadNext(rpc, command) => pipeline
                .read(rpc, 0, wire::MAX_RECORD_BYTES as u32)
                .map(|tag| State::Read(tag, rpc, command)),
            _ => return Ok(()),
        };
        match next {
            Ok(state) => {
                self.state = state;
                Ok(())
            }
            Err(error @ PipelineError::Limit(_)) => Err(Error::Transport(error)),
            Err(error) => self.fail(Error::Transport(error)),
        }
    }

    pub fn receive(&mut self, tag: Tag, reply: Reply, now: Instant) -> Result<(), Error> {
        if !self.owns(tag) {
            return Err(Error::ForeignTag);
        }
        self.check(now)?;
        let next = match (&self.state, reply) {
            (State::Attach(_, root), Reply::Attach(_)) => State::WalkNext(*root),
            (State::Walk(_, rpc), Reply::Walk(qids)) if qids.len() == 1 => State::OpenNext(*rpc),
            (State::Open(_, rpc), Reply::Lopen { .. }) => {
                self.deadline = None;
                State::Ready(*rpc)
            }
            (State::Write(_, rpc, command, size), Reply::Write(n)) if n as usize == *size => {
                State::ReadNext(*rpc, *command)
            }
            (State::Write(_, rpc, ..), Reply::Error(errno)) => State::Complete(*rpc, Err(errno)),
            (State::Read(_, rpc, command), Reply::Read(bytes)) => {
                let Ok(response) = Response::decode(&bytes) else {
                    return self.fail(Error::Unexpected);
                };
                if !correlated(*command, response) {
                    return self.fail(Error::Unexpected);
                }
                State::Complete(*rpc, Ok(response))
            }
            (_, Reply::Error(errno)) => return self.fail(Error::Remote(errno)),
            _ => return self.fail(Error::Unexpected),
        };
        self.state = next;
        Ok(())
    }

    /// Even a buffered verdict expires at the owner's original deadline.
    pub fn take(&mut self, now: Instant) -> Result<Option<Result<Response, Errno>>, Error> {
        self.check(now)?;
        if let State::Complete(rpc, response) = self.state {
            self.state = State::Ready(rpc);
            self.deadline = None;
            Ok(Some(response))
        } else {
            Ok(None)
        }
    }

    fn check(&mut self, now: Instant) -> Result<(), Error> {
        if matches!(self.state, State::Failed) {
            return Err(Error::Closed);
        }
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            return self.fail(Error::Deadline);
        }
        Ok(())
    }

    fn fail<T>(&mut self, error: Error) -> Result<T, Error> {
        self.state = State::Failed;
        self.deadline = None;
        Err(error)
    }
}

fn correlated(command: Command, response: Response) -> bool {
    match (command, response) {
        (Command::Begin { .. }, Response::Challenge(_))
        | (Command::Take { .. }, Response::Pending) => true,
        (Command::Take { attempt }, Response::Verified(verdict)) => verdict.attempt == attempt,
        (
            Command::Initialize { .. }
            | Command::Reserve(_)
            | Command::Activate { .. }
            | Command::Retire { .. }
            | Command::Relay { .. }
            | Command::Cancel { .. },
            Response::Done,
        ) => true,
        _ => false,
    }
}
