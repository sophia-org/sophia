//! Reservation, protected spawn and activation of the selected provider.
use super::*;

impl LockProvider {
    pub(super) fn revoke_registration(&mut self, launches: &mut LaunchCoordinator, now: Instant) {
        self.authorized = false;
        if let Some(registration) = self.registration.take() {
            // Stale means its shared connection has already been retired.
            let _ = launches.revoke(registration, now);
        }
        if let Some(spawn) = self.spawning.as_ref() {
            spawn.cancel();
        }
    }

    pub(super) fn reserve_launch(
        &mut self,
        regrant: bool,
        now: Instant,
        launches: &mut LaunchCoordinator,
    ) -> Result<(), String> {
        let supervisor = self
            .supervisor
            .as_mut()
            .ok_or("launch worker still owns the supervisor")?;
        if regrant {
            let epoch = self
                .grant_epoch
                .checked_add(1)
                .ok_or("lock grant epoch exhausted")?;
            let policy = ShellGpuLaunchPolicy::new(self.gpu, self.device.clone())?;
            let (spec, _) = policy.prepare(&self.base, epoch)?;
            supervisor
                .replace_launch_spec(spec)
                .map_err(|e| e.to_string())?;
            self.grant_epoch = epoch;
            self.succession.regranted();
        }
        self.registration = Some(
            launches
                .reserve(self.policy, now)
                .map_err(|e| format!("register: {e:?}"))?,
        );
        Ok(())
    }

    fn authentication_failed(
        &mut self,
        reason: &str,
        now: Instant,
        launches: &mut LaunchCoordinator,
    ) {
        crate::session_eprintln!(
            "sophia_live_lock_provider schema=1 status=authentication_failed error={reason}"
        );
        self.replace_process(now, launches);
        self.restart_at = Some(now + self.backoff);
        self.backoff = (self.backoff * 2).min(LONGEST_RESTART);
    }

    pub(super) fn poll_spawn(&mut self, now: Instant, launches: &mut LaunchCoordinator) {
        let Some(spawn) = self.spawning.as_ref() else {
            return;
        };
        match spawn.poll() {
            Ok(None) => {}
            Ok(Some(result)) => {
                self.spawning = None;
                self.supervisor = Some(result.supervisor);
                self.spawn_lease = Some(result.lease);
                match result.result {
                    Ok(()) => {
                        self.succession.child_started();
                        // A device change, timeout or owner failure may have
                        // revoked the reservation while the spawn was working.
                        if self.registration.is_none() || self.succession.failed() {
                            self.succession.child_terminating();
                            let _ = self.supervisor.as_mut().unwrap().request_termination();
                        }
                    }
                    Err(error) => self.authentication_failed(&error, now, launches),
                }
            }
            Err(error) => {
                self.spawning = None;
                self.fail(&error, launches);
            }
        }
    }

    pub(super) fn progress_authentication(
        &mut self,
        now: Instant,
        launches: &mut LaunchCoordinator,
    ) {
        if self.succession.failed() || self.authorized {
            return;
        }
        let Some(registration) = self.registration else {
            return;
        };
        let result = (|| -> Result<(), String> {
            match launches.phase(registration) {
                Some(Phase::Registering) => {}
                Some(Phase::Ready) => {
                    let channel = launches
                        .take_channel(registration, now)
                        .map_err(|e| format!("handoff: {e:?}"))?;
                    let supervisor = self.supervisor.take().ok_or("missing launch supervisor")?;
                    match ProtectedSpawn::start(supervisor, channel, self.owner_wake.clone()) {
                        Ok(spawn) => self.spawning = Some(spawn),
                        Err(error) => {
                            // The failed thread constructor consumed the idle
                            // supervisor. Do not manufacture a replacement owner.
                            self.fail(&error, launches);
                            return Ok(());
                        }
                    }
                }
                Some(Phase::Spawning | Phase::Activating) if self.spawning.is_none() => {
                    let supervisor = self
                        .supervisor
                        .as_mut()
                        .ok_or("missing spawned supervisor")?;
                    if launches
                        .activate(registration, supervisor, now)
                        .map_err(|e| format!("activate: {e:?}"))?
                    {
                        let worker = launches
                            .worker_authority(registration, supervisor, now)
                            .map_err(|e| format!("custody: {e:?}"))?;
                        let factory = self
                            .endpoint
                            .bind(worker, b"lock".to_vec(), b"lock-provider".to_vec())
                            .map_err(|e| format!("endpoint: {e:?}"))?;
                        let assignee = LockFileAssignee::from_supervisor(supervisor)
                            .map_err(|e| e.to_string())?;
                        self.assignee = Some(assignee.with_authentication(Box::new(factory)));
                        self.authorized = true;
                    }
                }
                Some(Phase::Spawning) => {}
                _ => return Err("launch registration lost before authorization".into()),
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.authentication_failed(&error, now, launches);
        }
    }
}
