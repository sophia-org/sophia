//! The session lock's provider process (t294): the renderer the operator
//! selected for what a locked session shows.
//!
//! It runs in a protection domain of its own (no network, its socket
//! directory and its config file read-only, a render node only when the
//! profile grants it) and is resident from session start, so its first image
//! is ready soon after a lock begins. A provider that exits is restarted with
//! a growing delay and never given up for good; each replacement connects
//! under a fresh epoch. None of this can hold a lock open or end one: without
//! a provider, or while it is down, every head shows Engine's fill.
use std::path::Path;
use std::time::{Duration, Instant};

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
}

impl LockProvider {
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
        let mut transport = LockFileTransport::bind_for_supervised_uid(
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
        let base = base.protection_domain(domain);
        let (spec, _) = ShellGpuLaunchPolicy::new(selection.gpu, gpu_device)?.prepare(&base, 1)?;
        let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::LockProvider, spec);
        supervisor
            .apply(SupervisorCommand::StartProcess {
                process: SupervisedProcessKind::LockProvider,
                delay: Duration::ZERO,
            })?
            .ok_or("the lock provider did not start")?;
        transport.authorize_supervised_process(&supervisor)?;
        let service = LockFileService::spawn(transport, lock, reserved_chords)?;
        service.set_owner_wake(wake.clone());
        Ok(Self {
            service,
            supervisor,
            restart_at: None,
            backoff: FIRST_RESTART,
            owner_wake: wake,
            drops: Default::default(),
            pacing_sample_at: crate::session_lock_frames::session_lock_pacing_enabled(
                std::env::var(SOPHIA_DIAGNOSTIC_LOCK_PACING_ENV)
                    .ok()
                    .as_deref(),
            )
            .then(Instant::now),
        })
    }

    /// Takes what the provider sent, notices its exit and restarts it when
    /// its delay has passed. Never blocks.
    pub(super) fn poll(&mut self, now: Instant) -> Vec<LockFileServiceEvent> {
        let mut events = Vec::new();
        // A provider cannot extend the owner pass by refilling its handoff.
        // Every publication rings the owner; another pass drains the remainder.
        for _ in 0..8 {
            let Ok(Some(event)) = self.service.try_event() else {
                break;
            };
            if matches!(event, LockFileServiceEvent::Connected { .. }) {
                // A provider that got as far as negotiating restarts promptly
                // if it fails later.
                self.backoff = FIRST_RESTART;
            }
            events.push(event);
        }
        if events.len() == 8 {
            self.owner_wake.notify();
        }
        if self.restart_at.is_none() && matches!(self.supervisor.poll(), Ok(Some(_))) {
            crate::session_eprintln!(
                "sophia_live_lock_provider schema=1 status=exited restart_after_ms={}",
                self.backoff.as_millis(),
            );
            self.restart_at = Some(now + self.backoff);
            self.backoff = (self.backoff * 2).min(LONGEST_RESTART);
        }
        if self.restart_at.is_some_and(|at| now >= at) {
            self.restart_at = None;
            let started = self
                .supervisor
                .apply(SupervisorCommand::StartProcess {
                    process: SupervisedProcessKind::LockProvider,
                    delay: Duration::ZERO,
                })
                .map_err(|error| error.to_string())
                .and_then(|_| {
                    LockFileAssignee::from_supervisor(&self.supervisor)
                        .map_err(|error| error.to_string())
                });
            match started {
                Ok(assignee) => {
                    if self
                        .service
                        .command(LockFileServiceCommand::ReplaceSupervisedProcess(assignee))
                        .is_err()
                    {
                        crate::session_eprintln!(
                            "sophia_live_lock_provider schema=1 status=replacement_queue_full"
                        );
                    }
                }
                Err(error) => {
                    crate::session_eprintln!(
                        "sophia_live_lock_provider schema=1 status=restart_failed error={error}"
                    );
                    self.restart_at = Some(now + self.backoff);
                    self.backoff = (self.backoff * 2).min(LONGEST_RESTART);
                }
            }
        }
        events
    }

    // No owner deadline is needed for a restart: the owner never sleeps
    // longer than its 25 ms maintenance budget, and restarts wait seconds.

    /// Session's lock state, entries, permits and outcomes. A full queue
    /// drops the command: the provider only renders. A dropped permit or
    /// outcome leaves that allocation waiting, so every drop is counted and
    /// the first and each power of two of a kind is recorded.
    pub(super) fn command(&self, command: LockFileServiceCommand) {
        use crate::session_lock_frames::SessionLockCommandKind as Kind;
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
    // Only a direct grant carries a device; a denied one must not.
    let gpu_device = render_device.filter(|_| selection.gpu == sophia_config::ShellGpuMode::Direct);
    let lock = crate::session_lock_object::session_lock_object(phase, Some(snapshot));
    let limits = crate::session_lock_object::session_lock_file_limits(Some(snapshot));
    match LockProvider::start(
        selection,
        &config.wm_socket_path.with_extension("lock"),
        &config.bubblewrap,
        // The admitted device at start. A later device change is not yet
        // followed: restarts reuse the grant made at start.
        gpu_device,
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
