//! The session lock's provider process (t294): the renderer the operator
//! selected for what a locked session shows.
//!
//! It runs in a protection domain of its own (no network, its socket
//! directory and its config file read-only, a render node only when the
//! profile grants it) and is resident from session start, so its first image
//! is ready soon after a lock begins. A provider that exits is restarted with
//! a growing delay and never given up for good; each replacement connects
//! under a fresh epoch of the one service the session keeps for it. A direct
//! grant follows the admitted render device: a change replaces the process,
//! never the service (`crate::session_lock_succession`). None of this can hold
//! a lock open or end one: without a provider, or while it is down, every head
//! shows Engine's fill.
use std::path::Path;
use std::time::{Duration, Instant};

use sophia_config::ShellGpuMode;
use sophia_protocol::lock_files::{LockChordRequest, LockFileLimits, LockObject};
use sophia_runtime::lock_files::{
    LockFileAssignee, LockFileService, LockFileServiceCommand, LockFileServiceEvent,
    LockFileTransport,
};
use sophia_runtime::{
    ProcessLaunchSpec, ProcessSupervisor, ProtectionDomainRole, ProtectionDomainSpec,
    ProtectionPath, SupervisedProcessKind, SupervisorCommand,
};

use super::metadata_shell::gpu::ShellGpuLaunchPolicy;
use crate::session_lock_succession::{LockProviderStart, LockProviderSuccession};

/// The provider's configuration file, when the profile names one.
pub(super) const SOPHIA_LOCK_CONFIG_ENV: &str = "SOPHIA_LOCK_CONFIG";
/// The diagnostic pacing sample's opt-in, exactly "1".
const SOPHIA_DIAGNOSTIC_LOCK_PACING_ENV: &str = "SOPHIA_DIAGNOSTIC_LOCK_PACING";
const PACING_SAMPLE_INTERVAL: Duration = Duration::from_secs(5);
const FIRST_RESTART: Duration = Duration::from_secs(1);
const LONGEST_RESTART: Duration = Duration::from_secs(60);

pub(super) struct LockProvider {
    service: LockFileService,
    supervisor: ProcessSupervisor,
    restart_at: Option<Instant>,
    backoff: Duration,
    owner_wake: sophia_wake::Notifier,
    drops: std::cell::Cell<crate::session_lock_frames::SessionLockCommandDrops>,
    /// The next diagnostic pacing sample, when the opt-in is set.
    pacing_sample_at: Option<Instant>,
    /// The ungranted launch every grant is prepared from.
    base: ProcessLaunchSpec,
    gpu: ShellGpuMode,
    /// The latest admitted render device; only a direct grant names one.
    device: Option<sophia_backend_live::LiveRenderDeviceIdentitySnapshot>,
    /// The grant epoch of the launch in the supervisor; zero before the first.
    grant_epoch: u64,
    succession: LockProviderSuccession,
    /// The running process's authorization while the service's queue is full.
    assignee: Option<LockFileAssignee>,
    /// Events of a retired process ignored since the last marker.
    ignored: u64,
    failure_reported: bool,
}

impl LockProvider {
    /// Binds the provider's endpoint and starts its service, which live as
    /// long as the session. The process is launched by the first poll; a
    /// direct grant without an admitted device waits for one.
    pub(super) fn start(
        selection: &sophia_config::LockProviderConfig,
        directory: &Path,
        bubblewrap: &Path,
        gpu_device: Option<sophia_backend_live::LiveRenderDeviceIdentitySnapshot>,
        lock: LockObject,
        limits: LockFileLimits,
        reserved_chords: Vec<LockChordRequest>,
        wake: sophia_wake::Notifier,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let transport = LockFileTransport::bind_for_supervised_uid(
            directory,
            rustix::process::geteuid().as_raw(),
            1,
            limits,
        )?;
        let mut domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::LockProvider])?
            .bubblewrap_path(bubblewrap)
            .path(ProtectionPath::read_only(directory))?;
        let mut base = ProcessLaunchSpec::new(&selection.executable)
            .env(
                sophia_runtime::SOPHIA_LOCK_9P_SOCKET_ENV,
                transport.socket_path(),
            )
            .process_group();
        if let Some(config) = &selection.config {
            domain = domain.path(ProtectionPath::read_only(config))?;
            base = base.env(SOPHIA_LOCK_CONFIG_ENV, config);
        }
        Self::with_service(
            transport,
            base.protection_domain(domain),
            selection.gpu,
            gpu_device,
            lock,
            reserved_chords,
            wake,
        )
    }

    fn with_service(
        transport: LockFileTransport,
        base: ProcessLaunchSpec,
        gpu: ShellGpuMode,
        gpu_device: Option<sophia_backend_live::LiveRenderDeviceIdentitySnapshot>,
        lock: LockObject,
        reserved_chords: Vec<LockChordRequest>,
        wake: sophia_wake::Notifier,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let supervisor = ProcessSupervisor::new(SupervisedProcessKind::LockProvider, base.clone());
        let service = LockFileService::spawn(transport, lock, reserved_chords)?;
        service.set_owner_wake(wake.clone());
        Ok(Self {
            service,
            supervisor,
            restart_at: Some(Instant::now()),
            backoff: FIRST_RESTART,
            owner_wake: wake,
            drops: Default::default(),
            pacing_sample_at: crate::session_lock_frames::session_lock_pacing_enabled(
                std::env::var(SOPHIA_DIAGNOSTIC_LOCK_PACING_ENV)
                    .ok()
                    .as_deref(),
            )
            .then(Instant::now),
            base,
            gpu,
            // Only a direct grant carries a device; a denied one must not.
            device: gpu_device.filter(|_| gpu == ShellGpuMode::Direct),
            grant_epoch: 0,
            succession: LockProviderSuccession::default(),
            assignee: None,
            ignored: 0,
            failure_reported: false,
        })
    }

    /// Follows the admitted render device. A direct grant names one device;
    /// when it changes, appears or is lost, the running process is asked to
    /// exit, never waited for, and its successor is granted the latest
    /// device. Returns whether the provider is being replaced, so Session
    /// revokes what the old one was granted.
    pub(super) fn follow_device(
        &mut self,
        device: Option<sophia_backend_live::LiveRenderDeviceIdentitySnapshot>,
        now: Instant,
    ) -> bool {
        if self.gpu != ShellGpuMode::Direct || self.device == device || self.succession.failed() {
            return false;
        }
        self.device = device;
        crate::session_println!(
            "sophia_live_lock_provider schema=1 status=regrant device={}",
            self.device.is_some(),
        );
        self.replace_process(now);
        true
    }

    /// Retires the running process without waiting for it, and owes its
    /// successor a launch prepared for the latest device.
    fn replace_process(&mut self, now: Instant) {
        // An authorization not yet queued is for an obsolete grant.
        self.assignee = None;
        if self.succession.device_changed()
            && let Err(error) = self.supervisor.request_termination()
        {
            crate::session_eprintln!(
                "sophia_live_lock_provider schema=1 status=termination_failed error={error}"
            );
        }
        self.restart_at = Some(now);
        self.queue_owed();
    }

    /// Queues what the service is owed, retirement first. A full queue keeps
    /// it owed for the next pass.
    fn queue_owed(&mut self) {
        if self.succession.retire_owed()
            && self
                .service
                .command(LockFileServiceCommand::RetireSupervisedProcess)
                .is_ok()
        {
            self.succession.retire_queued();
        }
        if self.succession.authorization_due()
            && let Some(assignee) = self.assignee.take()
        {
            match self
                .service
                .command(LockFileServiceCommand::ReplaceSupervisedProcess(assignee))
            {
                Ok(()) => self.succession.authorization_queued(),
                Err(LockFileServiceCommand::ReplaceSupervisedProcess(assignee)) => {
                    self.assignee = Some(assignee);
                }
                Err(_) => {}
            }
        }
    }

    /// The service stopped: nothing restarts it, since a new transport would
    /// begin its epochs again. The process is asked to exit.
    fn fail(&mut self, reason: &str) {
        let Some(terminate) = self.succession.fail() else {
            return;
        };
        crate::session_eprintln!(
            "sophia_live_lock_provider schema=1 status=service_failed error={reason}"
        );
        self.restart_at = None;
        self.assignee = None;
        if terminate {
            let _ = self.supervisor.request_termination();
        }
    }

    /// Whether the service stopped since the last call, so Session revokes
    /// what the provider was granted. Reported once.
    pub(super) fn take_failure(&mut self) -> bool {
        self.succession.failed() && !std::mem::replace(&mut self.failure_reported, true)
    }

    /// Takes what the provider sent, notices its exit and restarts it when
    /// its delay has passed. Never blocks.
    pub(super) fn poll(&mut self, now: Instant) -> Vec<LockFileServiceEvent> {
        if self.succession.failed() {
            // Reaping continues without blocking; nothing else does.
            if matches!(self.supervisor.poll(), Ok(Some(_))) {
                self.succession.child_reaped();
            }
            return Vec::new();
        }
        self.queue_owed();
        // A provider cannot extend the owner pass by refilling its handoff.
        // Every publication rings the owner; another pass drains the remainder.
        let mut taken = Vec::new();
        while taken.len() < 8 {
            match self.service.try_event() {
                Ok(Some(event)) => taken.push(Ok(event)),
                Ok(None) => break,
                Err(_) => {
                    taken.push(Err(()));
                    break;
                }
            }
        }
        if taken.len() == 8 {
            self.owner_wake.notify();
        }
        let events = self.admit_events(taken);
        if self.succession.failed() {
            return events;
        }
        match self.supervisor.poll() {
            Ok(Some(_)) => {
                let asked = self.succession.child_reaped();
                if self.restart_at.is_none() {
                    if asked {
                        self.restart_at = Some(now);
                    } else {
                        crate::session_eprintln!(
                            "sophia_live_lock_provider schema=1 status=exited restart_after_ms={}",
                            self.backoff.as_millis(),
                        );
                        self.restart_at = Some(now + self.backoff);
                        self.backoff = (self.backoff * 2).min(LONGEST_RESTART);
                    }
                }
            }
            Ok(None) => {}
            Err(error) => crate::session_eprintln!(
                "sophia_live_lock_provider schema=1 status=reap_failed error={error}"
            ),
        }
        if self.restart_at.is_some_and(|at| now >= at) {
            let admitted = self.gpu == ShellGpuMode::Denied || self.device.is_some();
            match self.succession.start(admitted) {
                LockProviderStart::Wait => {}
                LockProviderStart::AwaitDevice => {
                    self.restart_at = None;
                    crate::session_eprintln!(
                        "sophia_live_lock_provider schema=1 status=awaiting_device"
                    );
                }
                LockProviderStart::Start { regrant } => {
                    self.restart_at = None;
                    if let Err(error) = self.launch(regrant) {
                        crate::session_eprintln!(
                            "sophia_live_lock_provider schema=1 status=restart_failed error={error}"
                        );
                        self.restart_at = Some(now + self.backoff);
                        self.backoff = (self.backoff * 2).min(LONGEST_RESTART);
                    }
                }
            }
        }
        self.queue_owed();
        events
    }

    /// What goes on to Session from one drained batch. A stopped service,
    /// whether its queue closed or it reported failure, ends the provider and
    /// discards the whole batch: Session revokes its grants instead.
    fn admit_events(
        &mut self,
        taken: Vec<Result<LockFileServiceEvent, ()>>,
    ) -> Vec<LockFileServiceEvent> {
        let mut events = Vec::new();
        for event in taken {
            let event = match event {
                Ok(LockFileServiceEvent::Failed { message }) => {
                    self.fail(&message);
                    return Vec::new();
                }
                Ok(event) => event,
                Err(()) => {
                    self.fail("the lock file service stopped");
                    return Vec::new();
                }
            };
            if let LockFileServiceEvent::Retired { next_epoch } = event {
                crate::session_println!(
                    "sophia_live_lock_provider schema=1 status=retired next_epoch={next_epoch} ignored={}",
                    std::mem::take(&mut self.ignored),
                );
            }
            if !self.succession.hands_on(&event) {
                if !matches!(event, LockFileServiceEvent::Retired { .. }) {
                    // The retired process's; it is never admitted.
                    self.ignored = self.ignored.saturating_add(1);
                }
                continue;
            }
            if matches!(event, LockFileServiceEvent::Connected { .. }) {
                // A provider that got as far as negotiating restarts promptly
                // if it fails later.
                self.backoff = FIRST_RESTART;
            }
            events.push(event);
        }
        events
    }

    /// Spawns the process, first preparing a grant from the ungranted base
    /// for the latest device when one is owed. A failure leaves the grant
    /// owed: no older launch is used and no grant epoch wraps.
    pub(super) fn launch(&mut self, regrant: bool) -> Result<(), String> {
        if regrant {
            let epoch = self
                .grant_epoch
                .checked_add(1)
                .ok_or("the lock provider's grant epoch is exhausted")?;
            let policy = ShellGpuLaunchPolicy::new(self.gpu, self.device.clone())?;
            let (spec, _) = policy.prepare(&self.base, epoch)?;
            self.supervisor
                .replace_launch_spec(spec)
                .map_err(|error| error.to_string())?;
            self.grant_epoch = epoch;
            self.succession.regranted();
        }
        self.supervisor
            .apply(SupervisorCommand::StartProcess {
                process: SupervisedProcessKind::LockProvider,
                delay: Duration::ZERO,
            })
            .map_err(|error| error.to_string())?
            .ok_or("the lock provider did not start")?;
        self.succession.child_started();
        match LockFileAssignee::from_supervisor(&self.supervisor) {
            Ok(assignee) => {
                self.assignee = Some(assignee);
                Ok(())
            }
            Err(error) => {
                // A process that cannot be authorized is not kept.
                self.succession.child_terminating();
                let _ = self.supervisor.request_termination();
                Err(error.to_string())
            }
        }
    }

    // No owner deadline is needed for a restart: the owner never sleeps
    // longer than its 25 ms maintenance budget, and restarts wait seconds.

    /// Session's lock state, entries, permits and outcomes. A full queue
    /// drops the command: the provider only renders. A dropped permit or
    /// outcome leaves that allocation waiting, so every drop is counted and
    /// the first and each power of two of a kind is recorded.
    pub(super) fn command(&self, command: LockFileServiceCommand) {
        use crate::session_lock_frames::SessionLockCommandKind as Kind;
        if self.succession.failed() {
            return;
        }
        let kind = match &command {
            LockFileServiceCommand::Permit { .. } => Kind::Permit,
            LockFileServiceCommand::Outcome(_) => Kind::Outcome,
            _ => Kind::Other,
        };
        if self.service.command(command).is_err() {
            let mut drops = self.drops.get();
            let record = drops.dropped(kind);
            self.drops.set(drops);
            if let Some(record) = record {
                crate::session_eprintln!("{record}");
            }
        }
    }

    /// Whether the diagnostic pacing sample was opted in at start.
    pub(super) fn pacing_enabled(&self) -> bool {
        self.pacing_sample_at.is_some()
    }

    /// Whether a diagnostic pacing sample is due: never without the opt-in,
    /// otherwise every five seconds.
    pub(super) fn pacing_sample_due(&mut self, now: Instant) -> bool {
        let Some(at) = self.pacing_sample_at else {
            return false;
        };
        if now < at {
            return false;
        }
        self.pacing_sample_at = Some(now + PACING_SAMPLE_INTERVAL);
        true
    }
}

/// What Session last told the provider, so the lock object is rebuilt only
/// when the lock phase or the topology changed, and sent only when it did.
#[derive(Default)]
pub(super) struct LockPublication {
    key: Option<(crate::session_lock::SessionLockPhase, Option<u64>)>,
    published: Option<LockObject>,
}

impl LockPublication {
    /// The lock object to send, if the phase or topology moved and the
    /// object differs from the last one sent. `snapshot` is asked for only
    /// then, since it copies the topology.
    pub(super) fn update(
        &mut self,
        phase: crate::session_lock::SessionLockPhase,
        topology_epoch: Option<u64>,
        snapshot: impl FnOnce() -> Option<sophia_protocol::OutputAuthoritySnapshot>,
    ) -> Option<LockObject> {
        let key = (phase, topology_epoch);
        if self.key == Some(key) {
            return None;
        }
        self.key = Some(key);
        let object = crate::session_lock_object::session_lock_object(phase, snapshot().as_ref());
        (self.published.as_ref() != Some(&object)).then(|| {
            self.published = Some(object.clone());
            object
        })
    }
}

/// Starts the operator's provider once a topology is published, so its
/// limits follow the real screens. Only a session that can lock gets one.
/// A failure is reported once and the session locks with the fill alone.
pub(super) fn start_session_lock_provider(
    config: &super::PersistentXtermSessionConfig,
    lockable: bool,
    snapshot: &sophia_protocol::OutputAuthoritySnapshot,
    phase: crate::session_lock::SessionLockPhase,
    render_device: Option<sophia_backend_live::LiveRenderDeviceIdentitySnapshot>,
    wake: sophia_wake::Notifier,
) -> Option<LockProvider> {
    let selection = config.lock_provider.as_ref().filter(|_| lockable)?;
    let lock = crate::session_lock_object::session_lock_object(phase, Some(snapshot));
    let limits = crate::session_lock_object::session_lock_file_limits(Some(snapshot));
    match LockProvider::start(
        selection,
        &config.wm_socket_path.with_extension("lock"),
        &config.bubblewrap,
        render_device,
        lock,
        limits,
        Vec::new(),
        wake,
    ) {
        Ok(provider) => {
            crate::session_println!(
                "sophia_live_lock_provider schema=1 status=started executable={}",
                selection.executable.display(),
            );
            Some(provider)
        }
        Err(error) => {
            crate::session_eprintln!(
                "sophia_live_lock_provider schema=1 status=start_failed error={error}"
            );
            None
        }
    }
}

#[path = "../../tests/support/lock_provider_succession.rs"]
mod tests;
