// The independent file-role process has its own supervisor and never
// inherits WM restart or reapply policy.
enum PreparedOutputTransport {
    Files {
        transport: Box<sophia_runtime::OutputFileTransport>,
        supervisor: ProcessSupervisor,
    },
}

impl PreparedOutputTransport {
    fn bind(
        config: &PersistentXtermSessionConfig,
        directory: &std::path::Path,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let uid = rustix::process::geteuid().as_raw();
        let process = config.output_process.as_ref()
            .ok_or("output transport requires an explicitly configured process")?;
        let mut transport = sophia_runtime::OutputFileTransport::bind_for_supervised_uid(
            directory,
            uid,
            1,
            sophia_protocol::output_files::OutputFileLimits::default(),
        )?;
        let domain = sophia_runtime::ProtectionDomainSpec::bubblewrap([
            sophia_runtime::ProtectionDomainRole::OutputAuthority,
        ])?
        .bubblewrap_path(&config.bubblewrap)
        .path(sophia_runtime::ProtectionPath::read_only(directory))?;
        let mut spec = ProcessLaunchSpec::new(process)
            .env(
                sophia_runtime::SOPHIA_OUTPUT_9P_SOCKET_ENV,
                transport.socket_path(),
            )
            .process_group()
            .protection_domain(domain);
        for argument in &config.output_process_args {
            spec = spec.arg(argument)
        }
        let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::OutputAuthority, spec);
        supervisor
            .apply(sophia_runtime::SupervisorCommand::StartProcess {
                process: SupervisedProcessKind::OutputAuthority,
                delay: Duration::ZERO,
            })?
            .ok_or("output supervisor did not start its client")?;
        transport.authorize_supervised_process(&supervisor)?;
        Ok(Self::Files {
            transport: Box::new(transport),
            supervisor,
        })
    }

    fn start(
        self,
        snapshot: sophia_protocol::OutputAuthoritySnapshot,
    ) -> Result<LiveOutputService, std::io::Error> {
        Ok(match self {
            Self::Files {
                transport,
                supervisor,
            } => LiveOutputService::Files {
                service: sophia_runtime::OutputFileService::spawn(*transport, snapshot)?,
                supervisor: Box::new(supervisor),
            },
        })
    }
}

enum LiveOutputService {
    Files {
        service: sophia_runtime::OutputFileService,
        supervisor: Box<ProcessSupervisor>,
    },
}

impl LiveOutputService {
    fn attach_owner_wake(&self, notifier: &sophia_wake::Notifier) {
        let Self::Files { service, .. } = self;
        if !service.owner_wake_attached() {
            service.set_owner_wake(notifier.clone());
        }
    }

    fn command(
        &self,
        command: sophia_runtime::OutputFileServiceCommand,
    ) -> Result<(), sophia_runtime::OutputFileServiceCommand> {
        let Self::Files { service, .. } = self;
        service.command(command)
    }

    fn try_event(
        &mut self,
    ) -> Result<
        Option<sophia_runtime::OutputFileServiceEvent>,
        std::sync::mpsc::TryRecvError,
    > {
        match self {
            Self::Files { service, .. } => service.try_event(),
        }
    }

    /// Reaping and pausing must continue even while rollback holds the event
    /// queue. This observes child exit; a requested termination alone does not
    /// revoke or cancel the transaction.
    fn poll_supervisor(
        &mut self,
    ) -> Result<Option<(u32, std::process::ExitStatus)>, std::sync::mpsc::TryRecvError> {
        use std::os::unix::process::ExitStatusExt;
        let Self::Files {
            service,
            supervisor,
        } = self;
        let peer = supervisor.peer_id();
        if supervisor
            .poll()
            .map_err(|_| std::sync::mpsc::TryRecvError::Disconnected)?
            .is_some()
        {
            let status = supervisor.exit_status()
                .ok_or(std::sync::mpsc::TryRecvError::Disconnected)?;
            let peer_record = peer.map_or_else(|| "none".to_owned(), |value| value.to_string());
            let code = status.code().map_or_else(|| "none".to_owned(), |value| value.to_string());
            let signal = status.signal().map_or_else(|| "none".to_owned(), |value| value.to_string());
            tracing::info!(
                "sophia_live_output_supervisor schema=1 status=exited peer={peer_record} code={code} signal={signal}",
            );
            service
                .request_pause()
                .map_err(|_| std::sync::mpsc::TryRecvError::Disconnected)?;
            tracing::info!(
                "sophia_live_output_supervisor schema=1 status=pause_requested peer={peer_record}"
            );
            return Ok(peer.map(|peer| (peer, status)));
        }
        Ok(None)
    }

    #[cfg(test)]
    fn event_timeout(
        &self,
        timeout: Duration,
    ) -> Result<sophia_runtime::OutputFileServiceEvent, std::sync::mpsc::RecvTimeoutError>
    {
        match self {
            Self::Files { service, .. } => service.event_timeout(timeout),
        }
    }
}
