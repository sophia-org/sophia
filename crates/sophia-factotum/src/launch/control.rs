//! One exclusive private control handle. A write reserves its bounded response
//! before another command can run; clunk drops every delegation and attempt.

use super::{
    LaunchAuthority, Refusal,
    wire::{Command, Response},
};
use sophia_9p::Errno;
use std::time::Instant;

pub struct LaunchControl {
    authority: Option<LaunchAuthority>,
    response: Option<Vec<u8>>,
    origin: Instant,
}

impl std::fmt::Debug for LaunchControl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LaunchControl { .. }")
    }
}

impl Default for LaunchControl {
    fn default() -> Self {
        Self {
            authority: None,
            response: None,
            origin: Instant::now(),
        }
    }
}

impl LaunchControl {
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), Errno> {
        if self.response.is_some() {
            return Err(Errno::EAGAIN);
        }
        let command = Command::decode(bytes)?;
        let now = u64::try_from(self.origin.elapsed().as_millis()).map_err(|_| Errno::EIO)?;
        let response = self.execute(command, now).map_err(errno)?;
        self.response = Some(response.encode());
        Ok(())
    }

    pub fn read(&mut self, count: usize) -> Result<Vec<u8>, Errno> {
        let response = self.response.as_ref().ok_or(Errno(42))?;
        if count < response.len() {
            return Err(Errno::EINVAL);
        }
        Ok(self.response.take().expect("response checked"))
    }

    fn execute(&mut self, command: Command, now: u64) -> Result<Response, Refusal> {
        if let Command::Initialize { session } = command {
            if self.authority.is_some() {
                return Err(Refusal::Busy);
            }
            self.authority = Some(LaunchAuthority::new(session, now)?);
            return Ok(Response::Done);
        }
        let owner = self.authority.as_mut().ok_or(Refusal::Invalid)?;
        match command {
            Command::Initialize { .. } => unreachable!(),
            Command::Reserve(d) => owner.reserve(d, now)?,
            Command::Activate { launch } => owner.activate(launch, now)?,
            Command::Retire { launch } => owner.retire(launch, now)?,
            Command::Begin {
                attempt,
                remaining_msec,
            } => {
                let deadline = now
                    .checked_add(u64::from(remaining_msec))
                    .ok_or(Refusal::Invalid)?;
                return owner.begin(attempt, deadline, now).map(Response::Challenge);
            }
            Command::Relay { launch, nonce } => owner.relay(launch, nonce, now)?,
            Command::Take { attempt } => {
                return owner
                    .take_verdict(attempt, now)
                    .map(|v| v.map_or(Response::Pending, Response::Verified));
            }
            Command::Cancel { attempt } => owner.cancel(attempt, now)?,
        }
        Ok(Response::Done)
    }
}

fn errno(r: Refusal) -> Errno {
    match r {
        Refusal::Invalid => Errno::EINVAL,
        Refusal::Stale => Errno::ESTALE,
        Refusal::Busy => Errno(16),
        Refusal::Capacity | Refusal::Reserved | Refusal::RandomUnavailable => Errno::EAGAIN,
        Refusal::Audience => Errno::EACCES,
        Refusal::ClockReversed | Refusal::Closed => Errno::EIO,
    }
}
