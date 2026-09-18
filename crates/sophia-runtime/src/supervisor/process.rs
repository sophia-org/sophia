use super::*;
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
}

#[derive(Debug)]
struct ManagedChild {
    child: Child,
    peer_pid: u32,
    protection: Option<ProtectionDomainEvidence>,
}

impl ProcessSupervisor {
    pub fn new(process: SupervisedProcessKind, spec: ProcessLaunchSpec) -> Self {
        Self {
            process,
            spec,
            child: None,
            termination_deadline: None,
        }
    }

    pub const fn process(&self) -> SupervisedProcessKind {
        self.process
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

    pub fn protection_evidence(&self) -> Option<&ProtectionDomainEvidence> {
        self.child
            .as_ref()
            .and_then(|child| child.protection.as_ref())
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
            Ok(Some(_status)) => {
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
        let exited = managed
            .child
            .try_wait()
            .map_err(|error| ProcessSupervisorError::WaitFailed {
                process: self.process,
                message: error.to_string(),
            })?
            .is_some();
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
        if self.child.is_some() {
            return Err(ProcessSupervisorError::AlreadyRunning {
                process: self.process,
            });
        }

        if !delay.is_zero() {
            std::thread::sleep(delay);
        }

        let managed = if let Some(domain) = &self.spec.protection_domain {
            let protected =
                spawn_bubblewrap(self.process, &self.spec, domain).map_err(|error| {
                    ProcessSupervisorError::ProtectionDomainFailed {
                        process: self.process,
                        message: error.to_string(),
                    }
                })?;
            ManagedChild {
                peer_pid: protected.evidence.peer_pid,
                child: protected.child,
                protection: Some(protected.evidence),
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
