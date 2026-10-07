//! Nonblocking 9P turns for the supervised lock provider. Admission follows
//! the output role: the endpoint checks the numeric credentials, and both the
//! connector's `SO_PEERPIDFD` and the launched process's pidfd must still be
//! alive before any byte reaches 9P. Every admitted connection gets a fresh
//! epoch, so nothing a replaced provider submitted can match its successor.
use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;
use std::time::{Duration, Instant};

use sophia_9p::{Errno, records::Limits, unix::Server};
use sophia_protocol::lock_files::*;

use super::{LockFileExport, LockFileQids, LockFileSettings, LockInbound};
use crate::{PolicyRole, PolicyRoleEndpoint, PolicyRoleEndpointError};

/// A supervisor-checked provider captured before a worker handoff. Private
/// fields keep a numeric PID from being paired with an unrelated pidfd.
#[derive(Debug)]
pub struct LockFileAssignee {
    pid: u32,
    pidfd: Option<OwnedFd>,
}

impl LockFileAssignee {
    pub fn from_supervisor(
        supervisor: &crate::ProcessSupervisor,
    ) -> Result<Self, LockFileTransportError> {
        let pidfd = supervisor.peer_pidfd()?;
        let pid = supervisor.peer_id().ok_or(Errno::EINVAL)?;
        Ok(Self { pid, pidfd })
    }
}

#[derive(Debug)]
pub enum LockFileTransportError {
    Endpoint(PolicyRoleEndpointError),
    File(Errno),
    Io(std::io::Error),
}

impl std::fmt::Display for LockFileTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for LockFileTransportError {}
impl From<Errno> for LockFileTransportError {
    fn from(error: Errno) -> Self {
        Self::File(error)
    }
}
impl From<std::io::Error> for LockFileTransportError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<PolicyRoleEndpointError> for LockFileTransportError {
    fn from(error: PolicyRoleEndpointError) -> Self {
        Self::Endpoint(error)
    }
}

pub struct LockFileTransport {
    endpoint: PolicyRoleEndpoint,
    assignee: Option<OwnedFd>,
    qids: LockFileQids,
    limits: LockFileLimits,
    next_epoch: u64,
    server: Option<Server<LockFileExport>>,
    negotiation_deadline: Option<Instant>,
}

fn pidfd_alive(pidfd: &OwnedFd) -> Result<bool, LockFileTransportError> {
    let mut fds = [rustix::event::PollFd::new(
        pidfd,
        rustix::event::PollFlags::IN,
    )];
    Ok(rustix::event::poll(
        &mut fds,
        Some(&rustix::event::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        }),
    )
    .map_err(std::io::Error::from)?
        == 0)
}

impl LockFileTransport {
    pub fn bind_for_supervised_uid(
        directory: impl AsRef<Path>,
        uid: u32,
        first_epoch: u64,
        limits: LockFileLimits,
    ) -> Result<Self, LockFileTransportError> {
        limits.encode().map_err(|_| Errno::EINVAL)?;
        if first_epoch == 0 {
            return Err(Errno::EINVAL.into());
        }
        Ok(Self {
            endpoint: PolicyRoleEndpoint::bind_role_for_supervised_uid(
                directory,
                PolicyRole::Lock,
                uid,
            )?,
            assignee: None,
            qids: LockFileQids::default(),
            limits,
            next_epoch: first_epoch,
            server: None,
            negotiation_deadline: None,
        })
    }

    pub fn socket_path(&self) -> &Path {
        self.endpoint.socket_path()
    }

    pub fn next_epoch(&self) -> u64 {
        self.next_epoch
    }

    /// The caller must keep this direct child unreaped through authorization.
    pub fn authorize_supervised_pid(&mut self, pid: u32) -> Result<(), LockFileTransportError> {
        let pidfd = match rustix::process::pidfd_open(
            rustix::process::Pid::from_raw(i32::try_from(pid).map_err(|_| Errno::EINVAL)?)
                .ok_or(Errno::EINVAL)?,
            rustix::process::PidfdFlags::empty(),
        ) {
            Ok(pidfd) => Some(pidfd),
            Err(rustix::io::Errno::SRCH) => None,
            Err(error) => return Err(std::io::Error::from(error).into()),
        };
        self.endpoint.authorize_supervised_pid(pid)?;
        self.assignee = pidfd;
        Ok(())
    }

    /// A protected provider requires the supervisor's checked parent
    /// relationship; a numeric PID alone cannot prove bubblewrap owns it.
    pub fn authorize_supervised_process(
        &mut self,
        supervisor: &crate::ProcessSupervisor,
    ) -> Result<(), LockFileTransportError> {
        self.authorize_assignee(LockFileAssignee::from_supervisor(supervisor)?)
    }

    pub fn authorize_assignee(
        &mut self,
        assignee: LockFileAssignee,
    ) -> Result<(), LockFileTransportError> {
        self.endpoint.authorize_supervised_pid(assignee.pid)?;
        self.assignee = assignee.pidfd;
        Ok(())
    }

    /// Nobody may connect until the next authorization. A retired provider
    /// that is still exiting keeps no claim on the role.
    pub fn revoke_assignee(&mut self) {
        self.assignee = None;
    }

    fn assignee_alive(&self) -> Result<bool, LockFileTransportError> {
        self.assignee.as_ref().map_or(Ok(false), pidfd_alive)
    }

    /// One accept attempt. `lock` is the lock object in force and
    /// `reserved_chords` the chords Session keeps for itself.
    pub fn poll_accept(
        &mut self,
        lock: &LockObject,
        reserved_chords: &[LockChordRequest],
    ) -> Result<bool, LockFileTransportError> {
        if self.server.is_some() {
            return Err(Errno::EAGAIN.into());
        }
        if !self.assignee_alive()? {
            return Ok(false);
        }
        let next = self.next_epoch.checked_add(1).ok_or(Errno::ENOSPC)?;
        let epoch = self.next_epoch;
        let Some(stream) = self.endpoint.poll_expected()? else {
            return Ok(false);
        };
        // Both identities must still be alive after the numeric credentials
        // matched: a queued socket from an earlier occupant of this PID is no
        // authority.
        let authorized = (|| {
            let connector =
                sophia_linux_peer::socket_peer_pidfd(stream.as_fd()).map_err(|error| {
                    std::io::Error::new(
                        error.kind(),
                        format!("lock admission requires SO_PEERPIDFD: {error}"),
                    )
                })?;
            Ok::<_, LockFileTransportError>(pidfd_alive(&connector)? && self.assignee_alive()?)
        })();
        if !matches!(authorized, Ok(true)) {
            if let Some(peer) = self.endpoint.active_peer() {
                self.endpoint.release_peer(peer)?;
            }
            authorized?;
            return Ok(false);
        }
        // From admission on, even a failed setup spends the epoch.
        self.next_epoch = next;
        let setup = (|| {
            let export = LockFileExport::new(
                LockFileSettings {
                    epoch,
                    limits: self.limits,
                    reserved_chords: reserved_chords.to_vec(),
                    lock: lock.clone(),
                    lock_qid: 0,
                },
                self.qids.clone(),
            )?;
            let limits = Limits::new(65_536, 512, 16, 32, 131_072, 1).map_err(|_| Errno::EINVAL)?;
            let mut server = Server::new(export, limits)?;
            let connection = server.adopt(stream).map_err(|error| error.error)?;
            server.export_mut().bind_connection(connection)?;
            Ok::<_, LockFileTransportError>(server)
        })();
        match setup {
            Ok(server) => self.server = Some(server),
            Err(error) => {
                if let Some(peer) = self.endpoint.active_peer() {
                    self.endpoint.release_peer(peer)?;
                }
                return Err(error);
            }
        }
        self.negotiation_deadline =
            Some(Instant::now() + Duration::from_millis(self.limits.assembly_timeout_ms.into()));
        Ok(true)
    }

    pub fn export(&self) -> Option<&LockFileExport> {
        self.server.as_ref().map(Server::export)
    }

    /// No waiting and no unbounded queue. `false` asks the owner to call
    /// `disconnect` before accepting a replacement.
    pub fn turn(&mut self) -> Result<bool, LockFileTransportError> {
        let Some(server) = &mut self.server else {
            return Ok(false);
        };
        let now = Instant::now();
        server.export_mut().expire(now);
        if server.export().custody().is_negotiated() {
            self.negotiation_deadline = None;
        }
        // A provider that never negotiates holds the role for nothing.
        if self
            .negotiation_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            server.export_mut().revoke();
        }
        let running = server.turn(Some(Duration::ZERO))?;
        Ok(running && server.connection_count() != 0 && !server.export().is_revoked())
    }

    pub fn take_inbound(&mut self) -> Option<LockInbound> {
        self.server.as_mut()?.export_mut().take_inbound()
    }

    /// Runs `apply` on the connected export and wakes the server so its
    /// readers see the new event.
    fn owner<T>(
        &mut self,
        apply: impl FnOnce(&mut LockFileExport) -> Result<T, Errno>,
    ) -> Result<T, LockFileTransportError> {
        let server = self.server.as_mut().ok_or(Errno::ESTALE)?;
        let value = apply(server.export_mut())?;
        server.wake().wake();
        Ok(value)
    }

    pub fn publish_lock(&mut self, lock: LockObject) -> Result<u64, LockFileTransportError> {
        self.owner(|export| export.publish_lock(lock))
    }

    pub fn entry(&mut self, entry: LockEntry) -> Result<u64, LockFileTransportError> {
        self.owner(|export| export.entry(entry))
    }

    pub fn chord(&mut self, chord: LockChord) -> Result<u64, LockFileTransportError> {
        self.owner(|export| export.chord(chord))
    }

    pub fn permit(
        &mut self,
        allocation_id: u64,
        demand_id: u64,
        expires_after: Duration,
    ) -> Result<LockFramePermit, LockFileTransportError> {
        self.owner(|export| export.permit(allocation_id, demand_id, expires_after))
    }

    pub fn outcome(
        &mut self,
        outcome: LockCandidateOutcome,
    ) -> Result<u64, LockFileTransportError> {
        self.owner(|export| export.outcome(outcome))
    }

    /// Ends the connection; the next accepted provider gets a fresh epoch.
    pub fn disconnect(&mut self) -> Result<(), LockFileTransportError> {
        if let Some(mut server) = self.server.take() {
            server.export_mut().revoke();
            server.wake().wake();
            for _ in 0..4 {
                if !matches!(server.turn(Some(Duration::ZERO)), Ok(true)) {
                    break;
                }
            }
        }
        self.negotiation_deadline = None;
        if let Some(peer) = self.endpoint.active_peer() {
            self.endpoint.release_peer(peer)?;
        }
        Ok(())
    }
}

#[path = "transport_wait.rs"]
mod wait;
