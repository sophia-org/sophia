//! Nonblocking 9P turns for the supervised lock provider. Admission follows
//! the endpoint's expected-peer filter and live assignee check precede 9P.
//! Authenticated connections additionally require an afid and current launch
//! custody; only the legacy branch requires the connector's `SO_PEERPIDFD`.
//! Every admitted connection gets a fresh epoch, so nothing a replaced provider
//! submitted can match its successor.
use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;
use std::time::{Duration, Instant};

use sophia_9p::{Errno, records::Limits, unix::Server};
use sophia_protocol::lock_files::*;

use super::image_service::{ImageService, LockImagePolicy};
use super::image_worker::ImageWorker;
use super::leases::OrphanLeases;
use super::{
    LockFileCustody, LockFileExport, LockFileQids, LockFileSettings, LockImageReadiness,
    LockInbound,
};
use crate::authenticated_files::{
    AuthenticatedFiles, FileAuthentication, FileAuthenticationFactory,
};
use crate::{PolicyRole, PolicyRoleEndpoint, PolicyRoleEndpointError};

/// A supervisor-checked provider captured before a worker handoff. Private
/// fields keep a numeric PID from being paired with an unrelated pidfd.
pub struct LockFileAssignee {
    pid: u32,
    pidfd: Option<OwnedFd>,
    authentication: Option<Box<dyn FileAuthenticationFactory>>,
}

impl std::fmt::Debug for LockFileAssignee {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockFileAssignee")
            .field("pid", &self.pid)
            .field("authenticated", &self.authentication.is_some())
            .finish_non_exhaustive()
    }
}

impl LockFileAssignee {
    pub fn from_supervisor(
        supervisor: &crate::ProcessSupervisor,
    ) -> Result<Self, LockFileTransportError> {
        let pidfd = supervisor.peer_pidfd()?;
        let pid = supervisor.peer_id().ok_or(Errno::EINVAL)?;
        Ok(Self {
            pid,
            pidfd,
            authentication: None,
        })
    }
    /// Session policy opts this protected launch into afid admission. The
    /// expected-peer filter still applies, but socket credentials confer no root.
    pub fn with_authentication(mut self, factory: Box<dyn FileAuthenticationFactory>) -> Self {
        self.authentication = Some(factory);
        self
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
    server: Option<Server<AuthenticatedFiles<LockFileExport>>>,
    authentication: Option<Box<dyn FileAuthenticationFactory>>,
    retiring: Vec<Box<dyn FileAuthentication>>,
    negotiation_deadline: Option<Instant>,
    images: Option<LockImagePolicy>,
    /// The epoch's channel, once negotiation disclosed it; `image_bound`
    /// keeps a failed bind from retrying within the epoch.
    pub(super) image: Option<ImageService>,
    image_bound: bool,
    /// Sealed images of ended epochs still read somewhere; the next epoch
    /// inherits them against its budgets.
    pub(super) orphans: OrphanLeases,
    /// Validates sealed sources for every epoch; created with the policy.
    pub(super) worker: Option<ImageWorker>,
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
            authentication: None,
            retiring: Vec::new(),
            negotiation_deadline: None,
            images: None,
            image: None,
            image_bound: false,
            orphans: OrphanLeases::default(),
            worker: None,
        })
    }

    /// Offer sealed images to later authenticated connections. The policy
    /// is checked now against these lock limits, so a later epoch cannot
    /// fail its setup on it. Legacy connections never receive a channel.
    pub fn enable_sealed_images(
        &mut self,
        policy: LockImagePolicy,
    ) -> Result<(), LockFileTransportError> {
        let lock = LockObject {
            lock_epoch: 0,
            topology_generation: 1,
            phase: LockPhase::Unlocked,
            allocations: Vec::new(),
        };
        LockFileCustody::new(LockFileSettings {
            epoch: self.next_epoch,
            limits: self.limits,
            reserved_chords: Vec::new(),
            lock,
            lock_qid: 0,
        })?
        .configure_image(policy.settings(self.next_epoch, &self.limits, 1))?;
        let jobs = usize::from(self.limits.upload_slots).min(4);
        let bytes = usize::try_from(self.limits.max_resource_bytes).map_err(|_| Errno::EINVAL)?;
        let validation =
            usize::try_from(policy.validation_chunk_bytes).map_err(|_| Errno::EINVAL)?;
        self.worker = Some(ImageWorker::new(jobs, bytes, validation)?);
        self.images = Some(policy);
        Ok(())
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
        self.authentication = None;
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
        self.authentication = assignee.authentication;
        Ok(())
    }

    /// Nobody may connect until the next authorization. A retired provider
    /// that is still exiting keeps no claim on the role.
    pub fn revoke_assignee(&mut self) {
        self.assignee = None;
        self.authentication = None;
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
        if self.retiring.len() >= 8 {
            return Ok(false);
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
            if self.authentication.is_some() {
                // Afid verification, immutable identity and current launch
                // custody gate every operation. A socket pidfd is not required
                // on this path; stale queued sockets cannot authenticate.
                return self.assignee_alive();
            }
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
            let mut export = LockFileExport::new(
                LockFileSettings {
                    epoch,
                    limits: self.limits,
                    reserved_chords: reserved_chords.to_vec(),
                    lock: lock.clone(),
                    lock_qid: 0,
                },
                self.qids.clone(),
            )?;
            if self.authentication.is_some()
                && let Some(policy) = self.images
            {
                export.configure_image(policy.settings(
                    epoch,
                    &self.limits,
                    lock.topology_generation,
                ))?;
            }
            self.orphans.bequeath(export.custody_mut());
            let limits = Limits::new(65_536, 512, 16, 32, 131_072, 1).map_err(|_| Errno::EINVAL)?;
            let export = if self.authentication.is_some() {
                AuthenticatedFiles::pending(export)
            } else {
                AuthenticatedFiles::legacy(export)
            };
            let mut server = Server::new(export, limits)?;
            let connection = server.adopt(stream).map_err(|error| error.error)?;
            server
                .export_mut()
                .files_mut()
                .bind_connection(connection)?;
            if let Some(factory) = &mut self.authentication {
                let owner = factory.create(connection, Instant::now(), server.wake())?;
                server.export_mut().install(owner)?;
            }
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
        self.negotiation_deadline = self.authentication.is_none().then(|| {
            Instant::now() + Duration::from_millis(self.limits.assembly_timeout_ms.into())
        });
        Ok(true)
    }

    pub fn export(&self) -> Option<&LockFileExport> {
        self.server.as_ref().map(|s| s.export().files())
    }

    /// No waiting and no unbounded queue. `false` asks the owner to call
    /// `disconnect` before accepting a replacement.
    pub fn turn(&mut self) -> Result<bool, LockFileTransportError> {
        let Some(server) = &mut self.server else {
            self.poll_detached_images();
            return Ok(false);
        };
        let now = Instant::now();
        if server.export_mut().poll(now).is_err() {
            server.export_mut().files_mut().revoke();
        }
        server.export_mut().files_mut().expire(now);
        if server.export().files().custody().is_negotiated() {
            self.negotiation_deadline = None;
        } else if self.negotiation_deadline.is_none() && server.export().admitted() {
            self.negotiation_deadline =
                Some(now + Duration::from_millis(self.limits.assembly_timeout_ms.into()));
        }
        // A provider that never negotiates holds the role for nothing.
        if self
            .negotiation_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            server.export_mut().files_mut().revoke();
        }
        let mut journalled = service_image(
            &mut self.image,
            &mut self.image_bound,
            &self.endpoint,
            server.export_mut(),
            self.worker.as_mut(),
            now,
        );
        // Released leases return their budget before admissions run.
        let custody = server.export_mut().files_mut().custody_mut();
        self.orphans.poll();
        journalled |= custody.collect_sealed_leases().unwrap_or(false);
        if let Some(worker) = &mut self.worker {
            journalled |= worker.serve(custody, now);
        }
        if journalled {
            server.wake().wake();
        }
        let running = server.turn(Some(Duration::ZERO))?;
        Ok(running && server.connection_count() != 0 && !server.export().files().is_revoked())
    }

    pub fn take_inbound(&mut self) -> Option<LockInbound> {
        self.server
            .as_mut()?
            .export_mut()
            .files_mut()
            .take_inbound()
    }

    /// Runs `apply` on the connected export and wakes the server so its
    /// readers see the new event.
    fn owner<T>(
        &mut self,
        apply: impl FnOnce(&mut LockFileExport) -> Result<T, Errno>,
    ) -> Result<T, LockFileTransportError> {
        let server = self.server.as_mut().ok_or(Errno::ESTALE)?;
        let value = apply(server.export_mut().files_mut())?;
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
    /// Its image channel and listener close with it.
    pub fn disconnect(&mut self) -> Result<(), LockFileTransportError> {
        self.image = None;
        self.image_bound = false;
        if let Some(mut server) = self.server.take() {
            server.export_mut().files_mut().revoke();
            if let Some(worker) = &mut self.worker {
                worker.end_epoch();
            }
            self.orphans
                .adopt(server.export_mut().files_mut().custody_mut());
            server.wake().wake();
            for _ in 0..4 {
                if !matches!(server.turn(Some(Duration::ZERO)), Ok(true)) {
                    break;
                }
            }
            server.wake().stop();
            let _ = server.turn(Some(Duration::ZERO));
            if let Some(owner) = server.export_mut().retire(Instant::now()) {
                if self.retiring.len() >= 8 {
                    return Err(Errno::ENOSPC.into());
                }
                self.retiring.push(owner);
            }
        }
        self.negotiation_deadline = None;
        if let Some(peer) = self.endpoint.active_peer() {
            self.endpoint.release_peer(peer)?;
        }
        Ok(())
    }

    /// Continue exact cancellation while disconnected; these owners retain
    /// their endpoint/global quota until Factotum answers or is failed closed.
    /// Validation worker credits held, reservations and owed completions
    /// of live and ended epochs alike.
    pub fn image_jobs_pending(&self) -> usize {
        self.worker.as_ref().map_or(0, ImageWorker::pending)
    }

    /// Sealed images of this and earlier epochs still charged, retiring or
    /// orphaned.
    pub fn sealed_leases_pending(&self) -> usize {
        self.orphans.len()
            + self
                .export()
                .map_or(0, |export| export.custody().sealed_leases_pending())
    }

    /// Between connections: takes ended epochs' worker completions and
    /// drops released orphans. Owners call this every turn while no
    /// provider is connected; the worker's readiness and the lease poll
    /// wake them for it.
    pub fn poll_detached_images(&mut self) {
        if self.server.is_some() {
            return;
        }
        if let Some(worker) = &mut self.worker {
            worker.drain_ended();
        }
        self.orphans.poll();
    }

    pub fn poll_authentication_cleanup(&mut self, now: Instant) {
        self.retiring.retain_mut(|owner| {
            let _ = owner.poll(now);
            !owner.drained()
        });
    }
}

/// Bind the listener once the epoch discloses its channel, then serve it.
/// A failed bind closes the channel for the epoch; 9P bytes continue.
fn service_image(
    image: &mut Option<ImageService>,
    bound: &mut bool,
    endpoint: &PolicyRoleEndpoint,
    files: &mut AuthenticatedFiles<LockFileExport>,
    worker: Option<&mut ImageWorker>,
    now: Instant,
) -> bool {
    let export = files.files();
    if !*bound && export.custody().image_readiness() == LockImageReadiness::AwaitingHello {
        *bound = true;
        let settings = export.custody().image_settings().cloned();
        let service = (|| {
            let settings = settings?;
            let listener =
                crate::image_channel::ImageChannelListener::bind(endpoint.socket_path().parent()?)
                    .ok()?;
            Some(ImageService::new(
                listener,
                endpoint.active_peer()?,
                export.connection()?,
                &settings,
            ))
        })();
        match service {
            Some(service) => *image = Some(service),
            None => {
                let generation = export
                    .custody()
                    .image_settings()
                    .map(|settings| settings.discovery.channel_generation);
                if let Some(generation) = generation {
                    let _ = files.files_mut().custody_mut().image_channel_hello_failed(
                        generation,
                        LockChannelClosedReason::ProtocolFault,
                    );
                    return true;
                }
            }
        }
    }
    image
        .as_mut()
        .is_some_and(|image| image.service(files, worker, now))
}

#[path = "transport_wait.rs"]
mod wait;

#[cfg(test)]
#[path = "../../tests/support/lock_image_cancel_worker.rs"]
mod cancel_tests;
