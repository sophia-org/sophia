//! Authentication advances alongside process custody, never inside a blocking
//! spawn or negotiation loop. The Session owner services the shared coordinator.
use super::*;
use crate::launch_authentication::files::EndpointBudget;
use crate::launch_authentication::spawn::SpawnLease;
use crate::launch_authentication::startup::{Recipe, Startup};
use crate::launch_authentication::{LaunchCoordinator, LaunchPolicy, Phase, Registration};
use std::time::Instant;

pub(super) struct ComponentAuthentication {
    recipe: Recipe,
    endpoint: EndpointBudget,
    pending: Option<Startup>,
    registration: Option<Registration>,
    lease: Option<SpawnLease>,
    policy: ShellContentAdmissionPolicy,
    pub(super) stopping: bool,
}

#[cfg(test)]
#[path = "../../tests/support/authenticated_shell_process.rs"]
mod tests;

impl ComponentAuthentication {
    pub(super) fn retained(&self) -> bool {
        self.pending.is_some() || self.registration.is_some() || self.lease.is_some()
    }

    pub(super) fn needs_visit(&self) -> bool {
        self.pending.is_some() || (self.stopping && self.retained())
    }

    pub(super) fn start(
        &mut self,
        spec: ProcessLaunchSpec,
        policy: ShellContentAdmissionPolicy,
    ) -> Result<(), String> {
        if self.retained() {
            return Err("component authentication custody remains".into());
        }
        self.policy = policy;
        self.stopping = false;
        self.pending = Some(Startup::new(
            ProcessSupervisor::new(SupervisedProcessKind::Shell, spec),
            SupervisorCommand::StartProcess {
                process: SupervisedProcessKind::Shell,
                delay: Duration::ZERO,
            },
            self.endpoint.clone(),
            Recipe {
                policy: self.recipe.policy,
                uname: self.recipe.uname.clone(),
                aname: self.recipe.aname.clone(),
            },
            Instant::now(),
        )?);
        Ok(())
    }
}

impl ShellComponentProcesses {
    /// Session-selected policy, fixed before the first attempt. Replacements
    /// reuse the endpoint budget but always reserve a fresh launch generation.
    pub(crate) fn authenticate(
        &mut self,
        slot: usize,
        policy: LaunchPolicy,
        role_name: &str,
        endpoint: EndpointBudget,
    ) -> Result<(), String> {
        let slot = self.slots.get_mut(slot).ok_or("unknown shell slot")?;
        if slot.key.is_some() || slot.authentication.is_some() {
            return Err("shell authentication must be selected before launch".into());
        }
        slot.authentication = Some(ComponentAuthentication {
            recipe: Recipe {
                policy,
                uname: b"shell".to_vec(),
                aname: role_name.as_bytes().to_vec(),
            },
            endpoint,
            pending: None,
            registration: None,
            lease: None,
            policy: ShellContentAdmissionPolicy::Denied,
            stopping: false,
        });
        Ok(())
    }

    pub(crate) fn visit_authenticated(
        &mut self,
        bytes: usize,
        launches: &mut LaunchCoordinator,
        wake: sophia_wake::Notifier,
        now: Instant,
    ) -> ComponentProcessVisit {
        let mut failures = Vec::new();
        for index in 0..self.slots.len() {
            let slot = &mut self.slots[index];
            let (Some(key), Some(auth)) = (slot.key, slot.authentication.as_mut()) else {
                continue;
            };
            if auth
                .registration
                .is_some_and(|r| launches.phase(r) != Some(Phase::Active))
                && !auth.stopping
            {
                auth.stopping = true;
                failures.push(ComponentProcessEvent::Failed(
                    key,
                    "shell launch authority revoked".into(),
                ));
            }
            if auth.stopping {
                if let Some(registration) = auth.registration.take() {
                    let _ = launches.revoke(registration, now);
                }
                if let Some(pending) = auth.pending.as_mut() {
                    pending.cancel(launches, now, "component launch cancelled".into());
                }
                let _ = self.connections.close(key);
                if let Some(process) = slot.process.as_mut()
                    && let Err(error) = process.request_termination()
                {
                    failures.push(ComponentProcessEvent::Failed(key, error.to_string()));
                }
            }
            if let Some(pending) = auth.pending.as_mut() {
                match pending.poll(launches, wake.clone(), now) {
                    Ok(None) => {}
                    Ok(Some(started)) => {
                        auth.pending = None;
                        auth.registration = Some(started.registration);
                        auth.lease = Some(started.lease);
                        let process = slot.process.insert(started.supervisor);
                        let result = process
                            .protection_evidence()
                            .ok_or_else(|| "missing authenticated shell custody".to_string())
                            .and_then(|evidence| {
                                self.connections
                                    .begin_authenticated_negotiation(
                                        key,
                                        evidence,
                                        Duration::from_secs(5),
                                        auth.policy,
                                        started.factory,
                                    )
                                    .map_err(|e| e.to_string())
                            });
                        if let Err(error) = result {
                            auth.stopping = true;
                            let _ = launches.revoke(started.registration, now);
                            let _ = process.request_termination();
                            failures.push(ComponentProcessEvent::Failed(key, error));
                        }
                    }
                    Err(error) => {
                        auth.pending = None;
                        auth.stopping = true;
                        let _ = self.connections.close(key);
                        failures.push(ComponentProcessEvent::Failed(key, error));
                    }
                }
            }
        }
        let mut visit = self.visit(bytes);
        // A failed startup has no supervisor for visit() to report. Preserve
        // failures of other slots, including negotiation and reap failures.
        for failure in failures {
            let key = |event: &ComponentProcessEvent| match event {
                ComponentProcessEvent::Failed(key, _)
                | ComponentProcessEvent::ProcessRetired(key, _) => *key,
            };
            if visit
                .processes
                .iter()
                .flatten()
                .any(|event| key(event) == key(&failure))
            {
                continue;
            }
            if let Some(empty) = visit.processes.iter_mut().find(|e| e.is_none()) {
                *empty = Some(failure);
            }
        }
        for slot in &mut self.slots {
            let Some(auth) = slot.authentication.as_mut() else {
                continue;
            };
            if slot.process.is_none() && auth.pending.is_none() {
                if let Some(registration) = auth.registration.take() {
                    let _ = launches.revoke(registration, now);
                }
                auth.lease.take();
            }
        }
        visit
    }
}
