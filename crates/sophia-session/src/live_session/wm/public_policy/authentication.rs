// Startup runs before the owner loop, but must service the same launch book.
// Replacement uses the nonblocking Startup transaction on subsequent turns.
struct WmStartupContext<'a> {
    launches: &'a mut crate::launch_authentication::LaunchCoordinator,
    namespace: sophia_protocol::NamespaceId,
    wake: &'a OwnerWake,
    registration: Option<crate::launch_authentication::Registration>,
}

struct WmLaunchAuthentication {
    namespace: sophia_protocol::NamespaceId,
    endpoint: crate::launch_authentication::files::EndpointBudget,
    registration: Option<crate::launch_authentication::Registration>,
    lease: Option<crate::launch_authentication::spawn::SpawnLease>,
    pending: Option<WmAuthenticatedReplacement>,
}

struct WmAuthenticatedReplacement {
    supervisor: Option<ProcessSupervisor>,
    command: sophia_runtime::SupervisorCommand,
    transport: Option<PublicPolicyTransport>,
    epoch: u64,
    startup: Option<crate::launch_authentication::startup::Startup>,
}

struct AuthenticatedWmProcess {
    supervisor: ProcessSupervisor,
    authentication: WmLaunchAuthentication,
    factory: Box<dyn sophia_runtime::authenticated_files::FileAuthenticationFactory>,
}

fn wm_launch_recipe(
    namespace: sophia_protocol::NamespaceId,
) -> crate::launch_authentication::startup::Recipe {
    crate::launch_authentication::startup::Recipe {
        policy: crate::launch_authentication::LaunchPolicy {
            principal: 3,
            namespace: namespace.raw(),
            audience: 3,
            role: sophia_runtime::ProtectionDomainRole::SpatialPolicy,
        },
        uname: b"wm".to_vec(),
        aname: b"spatial-policy".to_vec(),
    }
}

impl WmStartupContext<'_> {
    fn start(
        &mut self,
        supervisor: ProcessSupervisor,
        command: sophia_runtime::SupervisorCommand,
    ) -> Result<AuthenticatedWmProcess, Box<dyn std::error::Error>> {
        use crate::launch_authentication::startup::Startup;
        let endpoint = self.launches.endpoint();
        let mut startup = Startup::new(
            supervisor,
            command,
            endpoint.clone(),
            wm_launch_recipe(self.namespace),
            Instant::now(),
        )?;
        loop {
            self.wake.begin_pass()?;
            let now = Instant::now();
            if let Err(error) = self.launches.poll(now) {
                startup.cancel(
                    self.launches,
                    now,
                    format!("WM launch coordinator: {error:?}"),
                );
            }
            if let Some(started) = startup.poll(self.launches, self.wake.notifier(), now)? {
                self.registration = Some(started.registration);
                return Ok(AuthenticatedWmProcess {
                    supervisor: started.supervisor,
                    authentication: WmLaunchAuthentication {
                        namespace: self.namespace,
                        endpoint,
                        registration: Some(started.registration),
                        lease: Some(started.lease),
                        pending: None,
                    },
                    factory: started.factory,
                });
            }
            self.wake
                .wait_for_service(Duration::from_millis(10), Vec::new(), None)?;
        }
    }

    fn wait_negotiated(&mut self, worker: &PolicyTransportWorker) -> Option<String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Err(error) = self.launches.poll(Instant::now()) {
                return Some(format!("WM profile authentication failed: {error:?}"));
            }
            match worker.event_timeout(Duration::from_millis(5)) {
                Ok(PolicyTransportEvent::Negotiated) => return None,
                Ok(PolicyTransportEvent::Failed(error)) => return Some(error),
                Ok(_) => {
                    return Some(
                        "policy worker emitted normal traffic before profile admission".into(),
                    );
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Some("policy profile admission worker disconnected".into());
                }
                Err(RecvTimeoutError::Timeout) if Instant::now() >= deadline => {
                    return Some("policy profile admission timed out".into());
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

impl LiveWmSession {
    fn authenticated_restart_pending(&self) -> bool {
        self.authentication
            .as_ref()
            .is_some_and(|auth| auth.pending.is_some())
    }

    fn poll_wm_authentication(
        &mut self,
        launches: &mut crate::launch_authentication::LaunchCoordinator,
        wake: sophia_wake::Notifier,
        layout: &mut PersistentLiveLayout,
        output: sophia_engine::HeadlessOutput,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::launch_authentication::{Phase, startup::Startup};
        let Some(auth) = self.authentication.as_mut() else {
            return Ok(());
        };
        let now = Instant::now();
        let Some(pending) = auth.pending.as_mut() else {
            if !self.degraded
                && auth
                    .registration
                    .is_some_and(|r| launches.phase(r) != Some(Phase::Active))
            {
                self.request_transport_restart("wm_launch_authority_retired", None);
            }
            return Ok(());
        };
        let result = (|| -> Result<_, Box<dyn std::error::Error>> {
            if pending.startup.is_none() {
                if let Some(registration) = auth.registration.take() {
                    let _ = launches.revoke(registration, now);
                }
                // The old process has been reaped before the restart transaction
                // is installed; only now may its spawning thread go away.
                auth.lease.take();
                pending.startup = Some(Startup::new(
                    pending
                        .supervisor
                        .take()
                        .ok_or("missing replacement WM supervisor")?,
                    pending.command,
                    auth.endpoint.clone(),
                    wm_launch_recipe(auth.namespace),
                    now,
                )?);
            }
            let Some(started) = pending
                .startup
                .as_mut()
                .expect("WM startup")
                .poll(launches, wake, now)?
            else {
                return Ok(None);
            };
            let mut transport = pending
                .transport
                .take()
                .ok_or("missing replacement WM endpoint")?;
            self.supervisor = started.supervisor;
            auth.registration = Some(started.registration);
            auth.lease = Some(started.lease);
            transport.authorize(&self.supervisor)?;
            let public = self.public.as_ref().ok_or("missing replacement WM state")?;
            let worker = start_public_policy_worker_authenticated(
                transport,
                pending.epoch,
                public.profile_key,
                public.native_presentation_capable,
                &self.supervisor,
                public.wm_filesystem_qids.clone(),
                Some(started.factory),
            )?;
            Ok(Some((worker, pending.epoch)))
        })();
        match result {
            Ok(None) => {}
            Ok(Some((worker, epoch))) => {
                auth.pending = None;
                let (state, _) = update_supervisor(
                    self.supervisor_state.clone(),
                    SupervisorEvent::ProcessStarted,
                    self.restart_policy,
                );
                self.supervisor_state = state;
                let public = self.public.take().ok_or("missing replacement WM state")?;
                self.finish_public_restart(public, worker, epoch, layout, output)?;
            }
            Err(error) => {
                auth.pending = None;
                if let Some(registration) = auth.registration.take() {
                    let _ = launches.revoke(registration, now);
                }
                self.supervisor.terminate()?;
                auth.lease.take();
                if self.desktop_reload.is_some() {
                    self.pending_policy_launch_spec = self.rollback_desktop_reload();
                    self.force_transport_restart = true;
                } else if self.committed == 0 {
                    return Err(error);
                } else {
                    self.degraded = true;
                    tracing::error!(
                        "sophia_live_wm schema=4 status=degraded reason=authenticated_restart_failed preserved_layout=true error={error}"
                    );
                }
            }
        }
        Ok(())
    }
}
