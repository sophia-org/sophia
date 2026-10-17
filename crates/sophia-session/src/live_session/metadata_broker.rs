use super::*;
use crate::broker_file_driver::BrokerFileDriver;
use crate::broker_file_owner::{BrokerCompletion, BrokerFileOwner};
use sophia_protocol::broker_files::{BrokerFileLimits, BrokerFileRequest};
use sophia_runtime::broker_files::{BrokerFileListener, SOPHIA_BROKER_9P_SOCKET_ENV};

/// Session-owned host for the metadata authority and Engine's sanitized table.
///
/// The process owns disclosure policy. This owner knows only the admitted X
/// client route, the reduced candidate, and the sanitized descriptor returned by
/// the broker; raw X properties never enter this process boundary.
pub(super) struct LiveMetadataBroker {
    supervisor: ProcessSupervisor,
    driver: BrokerFileDriver<sophia_x_authority::XServerFrontendClientId>,
    // Includes surfaces retired before admission, which never enter the broker.
    retired: BTreeSet<SurfaceId>,
    connection_epoch: u64,
}

impl LiveMetadataBroker {
    pub(super) fn start(bubblewrap: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        let executable = std::env::current_exe()?;
        Self::start_executable(&executable, bubblewrap)
    }

    fn start_executable(
        executable: &std::path::Path,
        bubblewrap: &std::path::Path,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let directory = std::env::temp_dir().join(format!(
            "sophia-live-metadata-broker-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        let listener = BrokerFileListener::bind(&directory, rustix::process::geteuid().as_raw())?;
        let socket = listener.socket_path().to_path_buf();
        let domain = sophia_runtime::ProtectionDomainSpec::bubblewrap([
            sophia_runtime::ProtectionDomainRole::MetadataBroker,
        ])?
        .bubblewrap_path(bubblewrap)
        .path(sophia_runtime::ProtectionPath::read_only(
            socket
                .parent()
                .expect("metadata broker socket always has a parent"),
        ))?;
        let spec = ProcessLaunchSpec::new(executable)
            .arg("metadata-broker-files-serve")
            .env(SOPHIA_BROKER_9P_SOCKET_ENV, &socket)
            .env("SOPHIA_BROKER_DEFAULT_DISCLOSURE", "class-only")
            .process_group()
            .protection_domain(domain);
        let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::MetadataBroker, spec);
        supervisor.apply(sophia_runtime::SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::MetadataBroker,
            delay: Duration::ZERO,
        })?;
        let ready = (|| -> Result<_, Box<dyn std::error::Error>> {
            // Admission takes the launch record, not the PID read off it: the
            // broker is a metadata-bearing role, so the transport requires
            // evidence that the domain above actually carries MetadataBroker.
            let evidence = supervisor
                .protection_evidence()
                .ok_or("metadata broker supervisor omitted its protection domain")?
                .clone();
            let owner = BrokerFileOwner::new(1)
                .map_err(|error| format!("metadata broker owner: {error:?}"))?;
            let service = listener.accept(
                owner,
                &evidence,
                1,
                1,
                0,
                BrokerFileLimits::default(),
                Duration::from_secs(5),
            )?;
            let mut driver = BrokerFileDriver::new(service);
            driver.negotiate(Duration::from_secs(5))?;
            Ok((evidence.peer_pid, driver))
        })();
        let (peer_pid, driver) = match ready {
            Ok(ready) => ready,
            Err(error) => {
                let _ = supervisor.terminate();
                return Err(error);
            }
        };
        crate::session_println!(
            "sophia_live_metadata_broker schema=1 status=ready protected=true peer_pid={} revision=2 transport=9p",
            peer_pid,
        );
        Ok(Self {
            supervisor,
            driver,
            retired: Default::default(),
            connection_epoch: 1,
        })
    }

    pub(super) fn poll(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.supervisor.poll()?.is_some() {
            return Err("protected metadata broker exited".into());
        }
        self.driver.poll()?;
        Ok(())
    }

    pub(super) fn drain_candidates(
        &mut self,
        receiver: &Receiver<sophia_x_authority::XAuthorityClientMetadataCandidate>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            match receiver.try_recv() {
                Ok(delivery) => self.apply_candidate(delivery)?,
                Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(()),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err("X frontend reduced metadata route disconnected".into());
                }
            }
        }
    }

    pub(super) fn admit_surface(
        &mut self,
        client: sophia_x_authority::XServerFrontendClientId,
        surface: SurfaceId,
        profile: NamespaceProfile,
    ) -> Result<Option<XAuthorityClientControlCommand>, Box<dyn std::error::Error>> {
        if self.retired.contains(&surface) {
            return Ok(None);
        }
        if let Some(existing) = self.driver.owner().route(surface) {
            if existing == client {
                return Ok(None);
            }
            return Err("metadata surface changed owning X client without retirement".into());
        }
        let response = self.driver.request(
            BrokerFileRequest::SurfaceAdmitted { surface, profile },
            client,
            Duration::from_secs(5),
        )?;
        let BrokerCompletion::Applied {
            request,
            rule: Some(rule),
            retired: None,
        } = response
        else {
            return Err(format!("metadata broker rejected surface admission: {response:?}").into());
        };
        if rule.surface != surface {
            return Err("metadata broker returned a rule for another surface".into());
        }
        Ok(Some(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::PublishMetadataRule {
                transaction: TransactionId::from_raw(request),
                surface,
                rule,
            },
        }))
    }

    pub(super) fn apply_candidate(
        &mut self,
        delivery: sophia_x_authority::XAuthorityClientMetadataCandidate,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.retired.contains(&delivery.candidate.surface) {
            return Ok(());
        }
        if self.driver.owner().route(delivery.candidate.surface) != Some(delivery.client) {
            return Err("reduced metadata arrived outside its admitted X client route".into());
        }
        let candidate = &delivery.candidate;
        let response = self.driver.request(
            BrokerFileRequest::CandidateReduced {
                surface: candidate.surface,
                disclosure: candidate.disclosure,
                label: candidate.label.as_ref().map(|label| label.text.as_str()),
                redacted: candidate.label.as_ref().is_some_and(|label| label.redacted),
                generation: candidate.generation,
            },
            delivery.client,
            Duration::from_secs(5),
        )?;
        match response {
            BrokerCompletion::Applied { .. } => {
                crate::session_println!(
                    "sophia_live_metadata_broker schema=1 status=descriptor_committed surface={} content=redacted",
                    candidate.surface.index(),
                );
                Ok(())
            }
            BrokerCompletion::NoChange { .. } => Ok(()),
            response => {
                Err(format!("metadata broker rejected reduced candidate: {response:?}").into())
            }
        }
    }

    pub(super) fn retire_surface(
        &mut self,
        surface: SurfaceId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.retired.insert(surface) {
            return Ok(());
        }
        let Some(client) = self.driver.owner().route(surface) else {
            return Ok(());
        };
        let response = self.driver.request(
            BrokerFileRequest::SurfaceRemoved { surface },
            client,
            Duration::from_secs(5),
        )?;
        match response {
            BrokerCompletion::Applied {
                retired: Some(retired),
                ..
            } if retired == surface => Ok(()),
            response => {
                Err(format!("metadata broker rejected surface retirement: {response:?}").into())
            }
        }
    }
}

impl MetadataBrokerView for LiveMetadataBroker {
    fn connection_epoch(&self) -> u64 {
        self.connection_epoch
    }

    fn descriptors(&self) -> &sophia_engine::ChromeDescriptorTable {
        self.driver.owner().descriptors()
    }
    fn grants(&self) -> &BTreeMap<SurfaceId, sophia_protocol::BrokerToplevelActionGrant> {
        self.driver.owner().grants()
    }
}

/// Read-only sanitized metadata consumed by shells. Presentation tests can
/// supply a table without constructing a fake broker process or transport.
pub(super) trait MetadataBrokerView {
    fn connection_epoch(&self) -> u64;
    fn descriptors(&self) -> &sophia_engine::ChromeDescriptorTable;
    fn grants(&self) -> &BTreeMap<SurfaceId, sophia_protocol::BrokerToplevelActionGrant>;

    fn shell_sources(
        &self,
        activation_surfaces: &BTreeSet<SurfaceId>,
    ) -> Vec<LiveShellDescriptorSource> {
        self.grants()
            .iter()
            .filter(|(surface, _)| activation_surfaces.contains(surface))
            .filter_map(|(surface, grant)| {
                self.descriptors()
                    .get(*surface)
                    .filter(|descriptor| descriptor.generation == grant.target_generation)
                    .cloned()
                    .map(|descriptor| LiveShellDescriptorSource {
                        surface: *surface,
                        descriptor,
                        grant: *grant,
                    })
            })
            .collect()
    }

    /// Resolves only the exact current broker grant. Shell slot and recipient
    /// validation happen in the shell owner before this issuer-side check.
    fn resolve_toplevel_action(
        &self,
        action: sophia_protocol::ToplevelActionCapabilityRef,
    ) -> Option<SurfaceId> {
        resolve_live_broker_toplevel_action(
            self.connection_epoch(),
            self.grants(),
            self.descriptors(),
            action,
        )
    }
}

pub(super) fn resolve_live_broker_toplevel_action(
    connection_epoch: u64,
    grants: &BTreeMap<SurfaceId, sophia_protocol::BrokerToplevelActionGrant>,
    descriptors: &sophia_engine::ChromeDescriptorTable,
    action: sophia_protocol::ToplevelActionCapabilityRef,
) -> Option<SurfaceId> {
    (action.issuer_epoch == connection_epoch).then_some(())?;
    grants.iter().find_map(|(surface, grant)| {
        (grant.token == action.token
            && grant.revocation_epoch == action.issuer_revocation_epoch
            && grant.target_generation == action.target_generation
            && descriptors
                .get(*surface)
                .is_some_and(|descriptor| descriptor.generation == action.target_generation))
        .then_some(*surface)
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LiveShellDescriptorSource {
    pub(super) surface: SurfaceId,
    pub(super) descriptor: sophia_protocol::ChromeDescriptor,
    pub(super) grant: sophia_protocol::BrokerToplevelActionGrant,
}

impl Drop for LiveMetadataBroker {
    fn drop(&mut self) {
        let process_stopped = self.supervisor.terminate().is_ok();
        let transport_stopped = self.driver.close().is_ok();
        if transport_stopped && process_stopped {
            crate::session_println!(
                "sophia_live_metadata_broker schema=1 status=stopped transport=disconnected process=terminated"
            );
        } else {
            crate::session_eprintln!(
                "sophia_live_metadata_broker schema=1 status=failed stage=shutdown transport={} process={}",
                if transport_stopped {
                    "disconnected"
                } else {
                    "failed"
                },
                if process_stopped {
                    "terminated"
                } else {
                    "failed"
                },
            );
        }
    }
}

#[path = "../../tests/support/descriptor_broker_fixture.rs"]
pub(super) mod descriptor_fixture;

#[path = "../../tests/support/live_broker_files.rs"]
mod live_broker_files;
