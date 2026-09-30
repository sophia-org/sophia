// The independent file-role process has its own supervisor and never
// inherits WM restart or reapply policy.
enum PreparedOutputTransport {
    Socket(sophia_runtime::OutputSessionTransport),
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
        let Some(process) = &config.output_process else {
            return Ok(Self::Socket(
                sophia_runtime::OutputSessionTransport::bind_for_supervised_uid(directory, uid)?,
            ));
        };
        let mut transport = sophia_runtime::OutputFileTransport::bind_for_supervised_uid(
            directory,
            uid,
            1,
            sophia_protocol::output_files::OutputFileLimits::default(),
        )?;
        let domain = sophia_runtime::ProtectionDomainSpec::bubblewrap([
            sophia_runtime::ProtectionDomainRole::OutputAuthority,
        ])?
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

    fn wm_socket_path(&self) -> Option<&std::path::Path> {
        match self {
            Self::Socket(transport) => Some(transport.socket_path()),
            Self::Files { .. } => None,
        }
    }

    fn authorize_wm(&mut self, pid: u32) -> Result<(), sophia_runtime::OutputTransportError> {
        match self {
            Self::Socket(transport) => transport.authorize_supervised_pid(pid),
            Self::Files { .. } => Ok(()),
        }
    }

    fn start(
        self,
        snapshot: sophia_protocol::OutputAuthoritySnapshot,
    ) -> Result<LiveOutputService, std::io::Error> {
        Ok(match self {
            Self::Socket(transport) => {
                LiveOutputService::Socket(sophia_runtime::OutputTransportService::spawn(
                    transport,
                    1,
                    TransactionId::from_raw(1),
                    snapshot,
                )?)
            }
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
    Socket(sophia_runtime::OutputTransportService),
    Files {
        service: sophia_runtime::OutputFileService,
        supervisor: Box<ProcessSupervisor>,
    },
}

impl LiveOutputService {
    fn assigned_to_wm(&self) -> bool {
        matches!(self, Self::Socket(_))
    }

    fn command(
        &self,
        command: sophia_runtime::OutputTransportServiceCommand,
    ) -> Result<(), sophia_runtime::OutputTransportServiceCommand> {
        use sophia_runtime::{
            OutputFileServiceCommand as Files, OutputTransportServiceCommand as Old,
        };
        let Self::Files { service, .. } = self else {
            let Self::Socket(service) = self else {
                unreachable!()
            };
            return service.command(command);
        };
        let retained = command.clone();
        let command = match command {
            Old::PublishSnapshot { snapshot, .. } => Files::PublishSnapshot(snapshot),
            Old::Settle {
                transaction,
                outcome,
            } => Files::Settle {
                transaction,
                outcome,
            },
            // This terminal already belongs to the atomic replacement batch.
            Old::Reply { outcome, .. }
                if outcome.kind == sophia_protocol::OutputV1OutcomeKind::Stale =>
            {
                return Ok(());
            }
            _ => return Err(retained),
        };
        service.command(command).map_err(|_| retained)
    }

    fn try_event(
        &mut self,
    ) -> Result<
        Option<sophia_runtime::OutputTransportServiceEvent>,
        sophia_runtime::OutputTransportServiceDisconnected,
    > {
        match self {
            Self::Socket(service) => service.try_event(),
            Self::Files { service, .. } => service
                .try_event()
                .map_err(|_| sophia_runtime::OutputTransportServiceDisconnected),
        }
    }

    /// Reaping and pausing must continue even while rollback holds the event
    /// queue. This observes child exit; a requested termination alone does not
    /// revoke or cancel the transaction.
    fn poll_supervisor(
        &mut self,
    ) -> Result<Option<(u32, std::process::ExitStatus)>, sophia_runtime::OutputTransportServiceDisconnected> {
        use std::os::unix::process::ExitStatusExt;
        let Self::Files {
            service,
            supervisor,
        } = self
        else {
            return Ok(None);
        };
        let peer = supervisor.peer_id();
        if supervisor
            .poll()
            .map_err(|_| sophia_runtime::OutputTransportServiceDisconnected)?
            .is_some()
        {
            let status = supervisor.exit_status()
                .ok_or(sophia_runtime::OutputTransportServiceDisconnected)?;
            let peer_record = peer.map_or_else(|| "none".to_owned(), |value| value.to_string());
            let code = status.code().map_or_else(|| "none".to_owned(), |value| value.to_string());
            let signal = status.signal().map_or_else(|| "none".to_owned(), |value| value.to_string());
            tracing::info!(
                "sophia_live_output_supervisor schema=1 status=exited peer={peer_record} code={code} signal={signal}",
            );
            service
                .request_pause()
                .map_err(|_| sophia_runtime::OutputTransportServiceDisconnected)?;
            tracing::info!(
                "sophia_live_output_supervisor schema=1 status=pause_requested peer={peer_record}"
            );
            return Ok(peer.map(|peer| (peer, status)));
        }
        Ok(None)
    }

    fn pause_acceptance(
        &self,
        timeout: Duration,
    ) -> Result<Vec<sophia_runtime::AdmittedOutputProposal>, &'static str> {
        match self {
            Self::Socket(service) => service.pause_acceptance(timeout),
            Self::Files { service, .. } => service.pause_acceptance(timeout),
        }
    }

    #[cfg(test)]
    fn event_timeout(
        &self,
        timeout: Duration,
    ) -> Result<sophia_runtime::OutputTransportServiceEvent, std::sync::mpsc::RecvTimeoutError>
    {
        match self {
            Self::Socket(service) => service.event_timeout(timeout),
            Self::Files { service, .. } => service.event_timeout(timeout),
        }
    }
}
