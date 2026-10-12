//! Descriptor-shell startup and recovery share the bounded role launch owner.
use super::*;
use crate::launch_authentication::files::EndpointBudget;
use crate::launch_authentication::spawn::SpawnLease;
use crate::launch_authentication::startup::{Recipe, Startup};
use crate::launch_authentication::{LaunchCoordinator, LaunchPolicy, Phase, Registration};

pub(super) struct DescriptorAuthentication {
    namespace: sophia_protocol::NamespaceId,
    endpoint: EndpointBudget,
    pending: Option<Startup>,
    registration: Option<Registration>,
    lease: Option<SpawnLease>,
    epoch: Option<u64>,
    gpu: Option<gpu::ShellGpuLaunchEvidence>,
    pub(super) negotiating: bool,
    stopping: bool,
}

impl DescriptorAuthentication {
    fn retained(&self) -> bool {
        self.pending.is_some()
            || self.registration.is_some()
            || self.lease.is_some()
            || self.epoch.is_some()
    }
    pub(super) fn needs_visit(&self) -> bool {
        self.pending.is_some() || self.negotiating || (self.stopping && self.retained())
    }
}

#[cfg(test)]
#[path = "../../../tests/support/authenticated_descriptor_shell.rs"]
mod tests;

impl LiveMetadataShell {
    pub(in crate::live_session) fn authenticate(
        &mut self,
        namespace: sophia_protocol::NamespaceId,
        launches: &LaunchCoordinator,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.authentication.is_some()
            || self.supervisor.child_id().is_some()
            || self.connected
            || self.next_connection_epoch != 1
            || self.transport.connection_epoch() != 0
        {
            return Err("descriptor authentication must be selected before launch".into());
        }
        self.authentication = Some(DescriptorAuthentication {
            namespace,
            endpoint: launches.endpoint(),
            pending: None,
            registration: None,
            lease: None,
            epoch: None,
            gpu: None,
            negotiating: false,
            stopping: false,
        });
        Ok(())
    }

    /// Stop local authority immediately; the next owner visit retains and reaps
    /// any pending protected spawn before releasing its generation or lease.
    pub(super) fn stop_process(&mut self) -> Result<(), sophia_runtime::ProcessSupervisorError> {
        if let Some(auth) = self.authentication.as_mut() {
            auth.stopping = true;
            self.supervisor.request_termination()
        } else {
            self.supervisor.terminate()
        }
    }

    pub(in crate::live_session) fn poll_authenticated(
        &mut self,
        launches: &mut LaunchCoordinator,
        wake: sophia_wake::Notifier,
    ) -> Result<LiveMetadataShellPoll, Box<dyn std::error::Error>> {
        let Some(mut auth) = self.authentication.take() else {
            return self.poll();
        };
        let now = Instant::now();
        let result = self.advance_authentication(&mut auth, launches, wake, now);
        let result = match result {
            Ok(result) => Ok(result),
            Err(error) => {
                auth.stopping = true;
                self.connected = false;
                if let Some(registration) = auth.registration.take() {
                    let _ = launches.revoke(registration, now);
                }
                if let Some(pending) = auth.pending.as_mut() {
                    pending.cancel(launches, now, error.to_string());
                }
                let retirement = self.retire_connection_state("authentication_failure");
                let termination = self.supervisor.request_termination();
                crate::session_eprintln!(
                    "sophia_live_metadata_shell schema=1 status=unavailable reason=authentication error={error}"
                );
                retirement
                    .and_then(|()| termination.map_err(Into::into))
                    .map(|()| LiveMetadataShellPoll::Unavailable)
            }
        };
        self.authentication = Some(auth);
        result
    }

    fn advance_authentication(
        &mut self,
        auth: &mut DescriptorAuthentication,
        launches: &mut LaunchCoordinator,
        wake: sophia_wake::Notifier,
        now: Instant,
    ) -> Result<LiveMetadataShellPoll, Box<dyn std::error::Error>> {
        if auth.stopping {
            if let Some(registration) = auth.registration.take() {
                let _ = launches.revoke(registration, now);
            }
            if let Some(pending) = auth.pending.as_mut() {
                pending.cancel(launches, now, "descriptor launch cancelled".into());
                match pending.poll(launches, wake, now) {
                    Ok(None) => return Ok(LiveMetadataShellPoll::Unavailable),
                    Ok(Some(_)) => return Err("cancelled descriptor launch became active".into()),
                    Err(_) => auth.pending = None,
                }
            }
            self.supervisor.request_termination()?;
            if !self.supervisor.poll_termination()? {
                return Ok(LiveMetadataShellPoll::Unavailable);
            }
            auth.lease.take();
            auth.gpu.take();
            auth.negotiating = false;
            if let Some(epoch) = auth.epoch.take()
                && self.next_connection_epoch == epoch
            {
                self.next_connection_epoch =
                    epoch.checked_add(1).ok_or("descriptor epoch exhausted")?;
            }
            auth.stopping = false;
            self.reconnect_at = Some(now + SHELL_RECONNECT_RETRY_DELAY);
            return Ok(LiveMetadataShellPoll::Unavailable);
        }
        if self.presentation_paused {
            return Ok(LiveMetadataShellPoll::Unavailable);
        }
        if let Some(registration) = auth.registration
            && launches.phase(registration) != Some(Phase::Active)
        {
            return Err("descriptor launch authority revoked".into());
        }
        if self.connected {
            if self.supervisor.poll()?.is_some() {
                return Err("authenticated descriptor process exited".into());
            }
            self.transport.service_owner_turn()?;
            return Ok(LiveMetadataShellPoll::Healthy);
        }
        if auth.negotiating {
            if self.supervisor.poll()?.is_some() {
                return Err("descriptor exited before negotiation".into());
            }
            if let Some(welcome) = self.transport.poll_file_negotiation(65536)? {
                let peer = self
                    .supervisor
                    .protection_evidence()
                    .ok_or("missing descriptor custody")?
                    .peer_pid;
                auth.negotiating = false;
                return self.finish_negotiation(
                    peer,
                    welcome.selected_revision,
                    auth.epoch.ok_or("missing descriptor epoch")?,
                    "authenticated_launch",
                );
            }
            return Ok(LiveMetadataShellPoll::Unavailable);
        }
        if auth.pending.is_none() {
            if self.reconnect_at.is_some_and(|deadline| now < deadline) {
                return Ok(LiveMetadataShellPoll::Unavailable);
            }
            let epoch = self.next_connection_epoch;
            auth.epoch = Some(epoch);
            let (spec, gpu) = self.gpu.prepare(&self.base_launch_spec, epoch)?;
            self.supervisor.replace_launch_spec(spec)?;
            auth.gpu = gpu;
            let supervisor = std::mem::replace(
                &mut self.supervisor,
                ProcessSupervisor::new(SupervisedProcessKind::Shell, self.base_launch_spec.clone()),
            );
            auth.pending = Some(Startup::new(
                supervisor,
                sophia_runtime::SupervisorCommand::StartProcess {
                    process: SupervisedProcessKind::Shell,
                    delay: Duration::ZERO,
                },
                auth.endpoint.clone(),
                Recipe {
                    policy: LaunchPolicy {
                        principal: 7,
                        namespace: auth.namespace.raw(),
                        audience: 7,
                        role: sophia_runtime::ProtectionDomainRole::MetadataShell,
                    },
                    uname: b"shell".to_vec(),
                    aname: b"descriptor".to_vec(),
                },
                now,
            )?);
        }
        if let Some(started) = auth
            .pending
            .as_mut()
            .expect("prepared startup")
            .poll(launches, wake, now)?
        {
            auth.pending = None;
            auth.registration = Some(started.registration);
            auth.lease = Some(started.lease);
            self.supervisor = started.supervisor;
            let evidence = self
                .supervisor
                .protection_evidence()
                .ok_or("missing descriptor custody")?;
            crate::diagnostics::capture_process_identity(
                "shell",
                evidence.peer_pid,
                auth.epoch.expect("prepared epoch"),
            );
            launch::record_gpu_launch(evidence.peer_pid, auth.gpu.take());
            self.transport
                .authorize_authenticated_peer(evidence, started.factory)?;
            self.transport.begin_descriptor_file_negotiation(
                auth.epoch.expect("prepared epoch"),
                Duration::from_secs(5),
                self.content.admission_policy(),
            )?;
            auth.negotiating = true;
        }
        Ok(LiveMetadataShellPoll::Unavailable)
    }

    pub(in crate::live_session) fn finish_authentication_shutdown(
        &mut self,
        launches: &mut LaunchCoordinator,
        wake: sophia_wake::Notifier,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.stop_for_session_shutdown()?;
        let deadline = Instant::now() + Duration::from_secs(3);
        while self
            .authentication
            .as_ref()
            .is_some_and(DescriptorAuthentication::retained)
        {
            let _ = launches.poll(Instant::now());
            self.poll_authenticated(launches, wake.clone())?;
            if Instant::now() >= deadline {
                return Err("descriptor authentication cleanup deadline".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }
}
