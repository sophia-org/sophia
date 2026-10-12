use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessLaunchSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub environment: Vec<(OsString, OsString)>,
    pub process_group: bool,
    pub protection_domain: Option<ProtectionDomainSpec>,
}

impl ProcessLaunchSpec {
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            environment: Vec::new(),
            process_group: false,
            protection_domain: None,
        }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn process_group(mut self) -> Self {
        self.process_group = true;
        self
    }

    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.environment.push((key.into(), value.into()));
        self
    }

    pub fn protection_domain(mut self, protection_domain: ProtectionDomainSpec) -> Self {
        self.protection_domain = Some(protection_domain);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessSupervisorError {
    LaunchChannelRequired,
    StaleLaunchGeneration,
    ProtectedLaunchRequired,
    HostLaunchDomainConflict,
    WrongProcess {
        expected: SupervisedProcessKind,
        actual: SupervisedProcessKind,
    },
    AlreadyRunning {
        process: SupervisedProcessKind,
    },
    SpawnFailed {
        process: SupervisedProcessKind,
        message: String,
    },
    WaitFailed {
        process: SupervisedProcessKind,
        message: String,
    },
    ProtectionDomainFailed {
        process: SupervisedProcessKind,
        message: String,
    },
}

impl fmt::Display for ProcessSupervisorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LaunchChannelRequired => f.write_str("a fresh launch channel is required"),
            Self::StaleLaunchGeneration => f.write_str("launch generation was already spent"),
            Self::ProtectedLaunchRequired => {
                f.write_str("launch channel requires a protection domain")
            }
            Self::HostLaunchDomainConflict => {
                f.write_str("host namespace launch cannot replace a required protection domain")
            }
            Self::WrongProcess { expected, actual } => write!(
                f,
                "supervisor command for {:?} cannot be applied to {:?}",
                actual, expected
            ),
            Self::AlreadyRunning { process } => {
                write!(f, "{process:?} process is already running")
            }
            Self::SpawnFailed { process, message } => {
                write!(f, "failed to spawn {process:?}: {message}")
            }
            Self::WaitFailed { process, message } => {
                write!(f, "failed to wait for {process:?}: {message}")
            }
            Self::ProtectionDomainFailed { process, message } => {
                write!(f, "failed to isolate {process:?}: {message}")
            }
        }
    }
}

impl std::error::Error for ProcessSupervisorError {}

impl SophiaErrorExt for ProcessSupervisorError {
    fn kind(&self) -> SophiaErrorKind {
        SophiaErrorKind::ExternalProcess
    }
}

#[derive(Debug)]
pub struct ProcessSupervisor {
    process: SupervisedProcessKind,
    spec: ProcessLaunchSpec,
    child: Option<ManagedChild>,
    termination_deadline: Option<Instant>,
    exit_status: Option<std::process::ExitStatus>,
    last_launch_generation: u64,
}

#[derive(Debug)]
struct ManagedChild {
    child: Child,
    peer_pid: u32,
    protection: Option<ProtectionDomainEvidence>,
    launch_reservation: Option<LaunchReservation>,
    custody_active: Arc<AtomicBool>,
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        self.custody_active.store(false, Ordering::Release);
    }
}

impl ProcessSupervisor {
    pub fn new(process: SupervisedProcessKind, spec: ProcessLaunchSpec) -> Self {
        Self {
            process,
            spec,
            child: None,
            termination_deadline: None,
            exit_status: None,
            last_launch_generation: 0,
        }
    }

    pub const fn process(&self) -> SupervisedProcessKind {
        self.process
    }

    /// Take the most recently reaped child's status once. For protected
    /// processes this is the wrapper's status, not a domain-operation outcome.
    /// A new launch attempt clears any unconsumed status from its predecessor.
    pub fn take_exit_status(&mut self) -> Option<std::process::ExitStatus> {
        self.exit_status.take()
    }

    /// Inspect the last reap for diagnostics without consuming it. Protected
    /// processes report the wrapper status; a new launch clears this value.
    pub fn exit_status(&self) -> Option<std::process::ExitStatus> {
        self.exit_status
    }

    pub fn launch_spec(&self) -> &ProcessLaunchSpec {
        &self.spec
    }

    /// Replace the next launch specification while no child exists.
    ///
    /// Restarting a protected role may require fresh capability epochs and
    /// revalidated resource bindings. A running child keeps the exact spec it
    /// was launched with.
    pub fn replace_launch_spec(
        &mut self,
        spec: ProcessLaunchSpec,
    ) -> Result<(), ProcessSupervisorError> {
        if self.child.is_some() {
            return Err(ProcessSupervisorError::AlreadyRunning {
                process: self.process,
            });
        }
        self.spec = spec;
        Ok(())
    }

    pub fn child_id(&self) -> Option<u32> {
        self.child.as_ref().map(|child| child.child.id())
    }

    pub fn peer_id(&self) -> Option<u32> {
        self.child.as_ref().map(|child| child.peer_pid)
    }

    /// Capture the peer identity before authorization. Bubblewrap reaps its
    /// own child, so check the parent relationship after opening the pidfd.
    #[cfg(target_os = "linux")]
    pub(crate) fn peer_pidfd(&self) -> std::io::Result<Option<std::os::fd::OwnedFd>> {
        let Some(managed) = &self.child else {
            return Ok(None);
        };
        let pid = rustix::process::Pid::from_raw(managed.peer_pid as i32)
            .ok_or_else(|| std::io::Error::other("invalid supervised peer PID"))?;
        let pidfd = match rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty()) {
            Ok(pidfd) => pidfd,
            Err(rustix::io::Errno::SRCH) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if managed.protection.is_some() {
            // The wrapper is our unreaped child. If it has lost this peer,
            // the fd may name a recycled PID and must never authorize it.
            let wrapper = managed.child.id();
            let children =
                std::fs::read_to_string(format!("/proc/{wrapper}/task/{wrapper}/children"))?;
            if super::protection::parse_single_child_pid(&children) != Some(managed.peer_pid) {
                return Ok(None);
            }
        }
        Ok(Some(pidfd))
    }

    pub fn protection_evidence(&self) -> Option<&ProtectionDomainEvidence> {
        self.child
            .as_ref()
            .and_then(|child| child.protection.as_ref())
    }

    /// The generation consumed by this child's own successful protected spawn.
    /// Session compares it with its reservation before activating the relay.
    pub fn launch_generation(&self) -> Option<u64> {
        self.child.as_ref().and_then(|child| {
            child
                .launch_reservation
                .as_ref()
                .map(LaunchReservation::generation)
        })
    }

    /// Nonblocking custody check immediately before admission. try_wait caches
    /// an observed exit in Child; the ordinary poll still reports/reaps it once.
    /// A termination request withdraws custody before the process actually exits.
    pub fn has_live_launch(
        &mut self,
        reservation: &LaunchReservation,
        role: ProtectionDomainRole,
    ) -> Result<bool, ProcessSupervisorError> {
        if self.termination_deadline.is_some() {
            return Ok(false);
        }
        let Some(child) = self.child.as_mut() else {
            return Ok(false);
        };
        let role_matches = match &child.protection {
            Some(proof) => proof.roles.contains(&role),
            // Host application custody is deliberately weaker than protected
            // desktop-role custody. Only the explicit host handoff below can
            // install a reservation without protection evidence.
            None => {
                self.process == SupervisedProcessKind::NamespaceClient
                    && role == ProtectionDomainRole::NamespaceClient
            }
        };
        if child.launch_reservation.as_ref() != Some(reservation) || !role_matches {
            return Ok(false);
        }
        let live = child
            .child
            .try_wait()
            .map(|status| status.is_none())
            .map_err(|error| ProcessSupervisorError::WaitFailed {
                process: self.process,
                message: error.to_string(),
            });
        if !matches!(live, Ok(true)) {
            child.custody_active.store(false, Ordering::Release);
        }
        live
    }

    /// Hand current custody to a role worker without handing it the supervisor.
    /// The wrapper is our unreaped child when its pidfd is opened; peer_pidfd
    /// separately checks the protected child's parent relationship. Neither
    /// handle is obtained from a connecting socket or a peer's identity claim.
    pub fn launch_custody(
        &mut self,
        reservation: &LaunchReservation,
        role: ProtectionDomainRole,
    ) -> std::io::Result<Option<LaunchCustody>> {
        if !self
            .has_live_launch(reservation, role)
            .map_err(std::io::Error::other)?
        {
            return Ok(None);
        }
        let Some(peer) = self.peer_pidfd()? else {
            return Ok(None);
        };
        let managed = self.child.as_ref().expect("live child");
        let pid = rustix::process::Pid::from_raw(managed.child.id() as i32)
            .ok_or_else(|| std::io::Error::other("invalid supervised wrapper PID"))?;
        let wrapper = rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty())?;
        let custody = LaunchCustody::new(managed.custody_active.clone(), wrapper, peer);
        Ok(custody.is_live()?.then_some(custody))
    }

    /// Consume a new channel, including on failure. Once used, ordinary start
    /// commands cannot silently launch a replacement without authentication.
    pub fn start_with_launch_channel(
        &mut self,
        channel: LaunchChannel,
    ) -> Result<SupervisorEvent, ProcessSupervisorError> {
        if channel.generation <= self.last_launch_generation {
            return Err(ProcessSupervisorError::StaleLaunchGeneration);
        }
        self.last_launch_generation = channel.generation;
        self.start_after_with_channel(Duration::ZERO, Some(channel))
    }

    /// Launch an explicitly trusted-host application without claiming a
    /// protection domain. This path cannot authorize a desktop role, and an
    /// ordinary unauthenticated start cannot manufacture its reservation.
    pub fn start_host_namespace_client(
        &mut self,
        channel: LaunchChannel,
    ) -> Result<SupervisorEvent, ProcessSupervisorError> {
        self.ensure_process(SupervisedProcessKind::NamespaceClient)?;
        if channel.generation <= self.last_launch_generation {
            return Err(ProcessSupervisorError::StaleLaunchGeneration);
        }
        self.last_launch_generation = channel.generation;
        if self.spec.protection_domain.is_some() {
            return Err(ProcessSupervisorError::HostLaunchDomainConflict);
        }
        if self.child.is_some() {
            return Err(ProcessSupervisorError::AlreadyRunning {
                process: self.process,
            });
        }
        if self
            .spec
            .environment
            .iter()
            .any(|(key, _)| key == LAUNCH_AUTH_FD_ENV)
        {
            return Err(ProcessSupervisorError::SpawnFailed {
                process: self.process,
                message: "reserved launch-channel environment".into(),
            });
        }
        use std::os::fd::AsRawFd;
        let reservation = channel.reservation();
        let mut command = Command::new(&self.spec.program);
        command
            .args(&self.spec.args)
            .envs(self.spec.environment.iter().cloned());
        command.env(LAUNCH_AUTH_FD_ENV, channel.child.as_raw_fd().to_string());
        if self.spec.process_group {
            std::os::unix::process::CommandExt::process_group(&mut command, 0);
        }
        sophia_linux_peer::inherit_one_fd_on_exec(&mut command, channel.child.into()).map_err(
            |error| ProcessSupervisorError::SpawnFailed {
                process: self.process,
                message: error.to_string(),
            },
        )?;
        let child = command
            .spawn()
            .map_err(|error| ProcessSupervisorError::SpawnFailed {
                process: self.process,
                message: error.to_string(),
            })?;
        drop(command);
        self.exit_status = None;
        self.child = Some(ManagedChild {
            peer_pid: child.id(),
            child,
            protection: None,
            launch_reservation: Some(reservation),
            custody_active: Arc::new(AtomicBool::new(true)),
        });
        Ok(SupervisorEvent::ProcessStarted)
    }

    pub fn apply(
        &mut self,
        command: SupervisorCommand,
    ) -> Result<Option<SupervisorEvent>, ProcessSupervisorError> {
        match command {
            SupervisorCommand::None => Ok(None),
            SupervisorCommand::GiveUp { process } => {
                self.ensure_process(process)?;
                Ok(None)
            }
            SupervisorCommand::StartProcess { process, delay } => {
                self.ensure_process(process)?;
                self.start_after(delay).map(Some)
            }
        }
    }

    pub fn poll(&mut self) -> Result<Option<SupervisorEvent>, ProcessSupervisorError> {
        if self.termination_deadline.is_some() {
            return self
                .poll_termination()
                .map(|done| done.then_some(SupervisorEvent::ProcessExited));
        }
        let Some(child) = self.child.as_mut() else {
            return Ok(None);
        };

        match child.child.try_wait() {
            Ok(Some(status)) => {
                self.exit_status = Some(status);
                self.child = None;
                Ok(Some(SupervisorEvent::ProcessExited))
            }
            Ok(None) => Ok(None),
            Err(error) => Err(ProcessSupervisorError::WaitFailed {
                process: self.process,
                message: error.to_string(),
            }),
        }
    }

    /// Signal once without sleeping or transferring the child out of custody.
    pub fn request_termination(&mut self) -> Result<(), ProcessSupervisorError> {
        if self.termination_deadline.is_some() || self.child.is_none() {
            return Ok(());
        }
        self.termination_deadline = Some(Instant::now() + Duration::from_secs(2));
        let managed = self.child.as_mut().expect("checked child");
        managed.custody_active.store(false, Ordering::Release);
        if managed
            .child
            .try_wait()
            .map_err(|error| ProcessSupervisorError::WaitFailed {
                process: self.process,
                message: error.to_string(),
            })?
            .is_some()
        {
            return Ok(());
        }
        if self.spec.process_group {
            let pid =
                rustix::process::Pid::from_raw(managed.child.id() as i32).ok_or_else(|| {
                    ProcessSupervisorError::WaitFailed {
                        process: self.process,
                        message: "supervised process PID is invalid".into(),
                    }
                })?;
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::TERM);
        } else {
            managed
                .child
                .kill()
                .map_err(|error| ProcessSupervisorError::WaitFailed {
                    process: self.process,
                    message: error.to_string(),
                })?;
        }
        Ok(())
    }

    /// One nonblocking reap visit. A pending or failed visit retains the child
    /// and prevents launch-spec replacement. This does not prove GPU cleanup.
    pub fn poll_termination(&mut self) -> Result<bool, ProcessSupervisorError> {
        let Some(managed) = self.child.as_mut() else {
            self.termination_deadline = None;
            return Ok(true);
        };
        let Some(deadline) = self.termination_deadline else {
            return Ok(false);
        };
        let status =
            managed
                .child
                .try_wait()
                .map_err(|error| ProcessSupervisorError::WaitFailed {
                    process: self.process,
                    message: error.to_string(),
                })?;
        let exited = status.is_some();
        if self.spec.process_group && (exited || Instant::now() >= deadline) {
            let pid =
                rustix::process::Pid::from_raw(managed.child.id() as i32).ok_or_else(|| {
                    ProcessSupervisorError::WaitFailed {
                        process: self.process,
                        message: "supervised process PID is invalid".into(),
                    }
                })?;
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        } else if !exited && Instant::now() >= deadline {
            managed
                .child
                .kill()
                .map_err(|error| ProcessSupervisorError::WaitFailed {
                    process: self.process,
                    message: error.to_string(),
                })?;
        }
        if exited {
            self.exit_status = status;
            self.child = None;
            self.termination_deadline = None;
        }
        Ok(exited)
    }

    pub fn terminate(&mut self) -> Result<(), ProcessSupervisorError> {
        self.request_termination()?;
        while !self.poll_termination()? {
            std::thread::sleep(Duration::from_millis(25));
        }
        Ok(())
    }

    fn start_after(&mut self, delay: Duration) -> Result<SupervisorEvent, ProcessSupervisorError> {
        self.start_after_with_channel(delay, None)
    }

    fn start_after_with_channel(
        &mut self,
        delay: Duration,
        channel: Option<LaunchChannel>,
    ) -> Result<SupervisorEvent, ProcessSupervisorError> {
        if self.child.is_some() {
            return Err(ProcessSupervisorError::AlreadyRunning {
                process: self.process,
            });
        }

        if self.last_launch_generation != 0 && channel.is_none() {
            return Err(ProcessSupervisorError::LaunchChannelRequired);
        }
        if channel.is_some() && self.spec.protection_domain.is_none() {
            return Err(ProcessSupervisorError::ProtectedLaunchRequired);
        }

        self.exit_status = None;

        if !delay.is_zero() {
            std::thread::sleep(delay);
        }

        let managed = if let Some(domain) = &self.spec.protection_domain {
            let result = match channel {
                Some(channel) => {
                    spawn_bubblewrap_with_channel(self.process, &self.spec, domain, Some(channel))
                }
                None => spawn_bubblewrap(self.process, &self.spec, domain),
            };
            let protected =
                result.map_err(|error| ProcessSupervisorError::ProtectionDomainFailed {
                    process: self.process,
                    message: error.to_string(),
                })?;
            ManagedChild {
                peer_pid: protected.evidence.peer_pid,
                child: protected.child,
                protection: Some(protected.evidence),
                launch_reservation: protected.launch_reservation,
                custody_active: Arc::new(AtomicBool::new(true)),
            }
        } else {
            let mut command = Command::new(&self.spec.program);
            command.args(&self.spec.args);
            command.envs(self.spec.environment.iter().cloned());
            #[cfg(unix)]
            if self.spec.process_group {
                std::os::unix::process::CommandExt::process_group(&mut command, 0);
            }
            let child = command
                .spawn()
                .map_err(|error| ProcessSupervisorError::SpawnFailed {
                    process: self.process,
                    message: error.to_string(),
                })?;
            ManagedChild {
                peer_pid: child.id(),
                child,
                protection: None,
                launch_reservation: None,
                custody_active: Arc::new(AtomicBool::new(true)),
            }
        };
        self.child = Some(managed);
        Ok(SupervisorEvent::ProcessStarted)
    }

    fn ensure_process(&self, process: SupervisedProcessKind) -> Result<(), ProcessSupervisorError> {
        if process == self.process {
            Ok(())
        } else {
            Err(ProcessSupervisorError::WrongProcess {
                expected: self.process,
                actual: process,
            })
        }
    }
}

impl Drop for ProcessSupervisor {
    fn drop(&mut self) {
        // Startup can fail after the child exists but before the caller receives a
        // fully constructed role transport. Process ownership must remain with the
        // supervisor even on that partial path.
        let _ = self.terminate();
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "../../tests/support/output_peer_identity.rs"]
mod output_peer_identity;
