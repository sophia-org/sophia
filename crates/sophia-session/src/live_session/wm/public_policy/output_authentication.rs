// Session owns registration and spawning; the output worker owns afids.
#[derive(Default)]
struct OutputLaunchAuthentication {
    registration: Option<crate::launch_authentication::Registration>,
    spawning: Option<crate::launch_authentication::spawn::ProtectedSpawn>,
    lease: Option<crate::launch_authentication::spawn::SpawnLease>,
    endpoint: Option<crate::launch_authentication::files::EndpointBudget>,
    assignee: Option<sophia_runtime::OutputFileAssignee>,
    authorized: bool,
    initial_assignment: bool,
    failed: bool,
}

impl LiveOutputService {
    fn poll_authentication(
        &mut self,
        namespace: Option<sophia_protocol::NamespaceId>,
        launches: &mut crate::launch_authentication::LaunchCoordinator,
        wake: sophia_wake::Notifier,
    ) {
        use crate::launch_authentication::{LaunchPolicy, Phase, spawn::ProtectedSpawn};
        let Self::Files {
            service,
            supervisor,
            authentication,
        } = self;
        let Some(auth) = authentication.as_mut() else {
            return;
        };
        let now = Instant::now();
        let result = (|| -> Result<(), String> {
            // Always reclaim an outstanding spawn, including after cancellation.
            if let Some(spawning) = auth.spawning.as_ref()
                && let Some(result) = spawning.poll()?
            {
                auth.spawning = None;
                *supervisor = Some(Box::new(result.supervisor));
                auth.lease = Some(result.lease);
                result.result?;
            }
            if auth.failed {
                if let Some(supervisor) = supervisor.as_mut() {
                    supervisor
                        .request_termination()
                        .map_err(|e| e.to_string())?;
                }
                return Ok(());
            }
            if auth.registration.is_none() {
                let namespace = namespace.ok_or("output authentication has no policy namespace")?;
                if !launches.ready() {
                    return Ok(());
                }
                auth.endpoint = Some(launches.endpoint());
                auth.registration = Some(
                    launches
                        .reserve(
                            LaunchPolicy {
                                principal: 2,
                                namespace: namespace.raw(),
                                audience: 2,
                                role: sophia_runtime::ProtectionDomainRole::OutputAuthority,
                            },
                            now,
                        )
                        .map_err(|e| format!("register: {e:?}"))?,
                );
            }
            let registration = auth.registration.expect("reserved output launch");
            match launches.phase(registration) {
                Some(Phase::Registering) => {}
                Some(Phase::Ready) => {
                    let channel = launches
                        .take_channel(registration, now)
                        .map_err(|e| format!("handoff: {e:?}"))?;
                    let supervisor = supervisor.take().ok_or("missing output supervisor")?;
                    auth.spawning = Some(ProtectedSpawn::start(*supervisor, channel, wake)?);
                }
                Some(Phase::Spawning | Phase::Activating) if auth.spawning.is_none() => {
                    let supervisor = supervisor.as_mut().ok_or("missing output spawn result")?;
                    if launches
                        .activate(registration, supervisor, now)
                        .map_err(|e| format!("activate: {e:?}"))?
                    {
                        let worker = launches
                            .worker_authority(registration, supervisor, now)
                            .map_err(|e| format!("custody: {e:?}"))?;
                        let factory = auth
                            .endpoint
                            .as_ref()
                            .expect("output endpoint budget")
                            .bind(worker, b"output".to_vec(), b"output-authority".to_vec())
                            .map_err(|e| format!("endpoint: {e:?}"))?;
                        auth.assignee = Some(
                            sophia_runtime::OutputFileAssignee::from_supervisor(supervisor)
                                .map_err(|e| e.to_string())?
                                .with_authentication(Box::new(factory)),
                        );
                    }
                }
                Some(Phase::Spawning) => {}
                Some(Phase::Active) => {
                    if supervisor
                        .as_ref()
                        .is_none_or(|supervisor| supervisor.peer_id().is_none())
                    {
                        return Err("output process departed".into());
                    }
                }
                _ => return Err("output launch authority was retired".into()),
            }
            if let Some(assignee) = auth.assignee.take() {
                match service.command(
                    sophia_runtime::OutputFileServiceCommand::ReplaceSupervisedProcess(assignee),
                ) {
                    Ok(()) => {
                        auth.authorized = true;
                        auth.initial_assignment = true;
                    }
                    Err(sophia_runtime::OutputFileServiceCommand::ReplaceSupervisedProcess(
                        assignee,
                    )) => {
                        auth.assignee = Some(assignee);
                    }
                    Err(_) => unreachable!("assign preserves its command on refusal"),
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            if !auth.failed {
                tracing::error!(
                    "sophia_live_output_authentication schema=1 status=failed error={error}"
                );
            }
            auth.failed = true;
            auth.authorized = false;
            auth.assignee = None;
            if let Some(registration) = auth.registration.take() {
                let _ = launches.revoke(registration, now);
            }
            if let Some(spawning) = auth.spawning.as_ref() {
                spawning.cancel();
            }
            if let Some(supervisor) = supervisor.as_mut() {
                let _ = supervisor.request_termination();
            }
            let _ = service.request_pause();
        }
    }
}
