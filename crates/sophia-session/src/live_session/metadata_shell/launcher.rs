use super::*;
use crate::application_catalog::*;
use sophia_protocol::*;

struct PreparedLauncher {
    transaction: TransactionId,
    candidate: ShellLauncherCandidate,
    outcome: ShellLauncherOutcome,
    targets: Vec<(u16, Rect)>,
    output_generation: u64,
    deadline: Instant,
}
#[derive(Default)]
pub(super) struct LiveLauncherSession {
    worker: Option<ApplicationCatalogWorker>,
    catalog: Option<ApplicationCatalog>,
    descriptors: Option<ShellApplicationCatalog>,
    outgoing: VecDeque<Vec<u8>>,
    refresh: Option<u64>,
    worker_deadline: Option<Instant>,
    open: Option<OutputId>,
    query: String,
    queued: Option<ShellLauncherOperation>,
    request: Option<(TransactionId, ShellLauncherRequest, Instant)>,
    pending: Option<PreparedLauncher>,
    presented: Option<PreparedLauncher>,
    grant: Option<(TransactionId, ShellLauncherActivation, Instant)>,
    launch: Option<(TransactionId, ShellLauncherActivation)>,
    verifying: bool,
    revoked: bool,
    last_candidate: u64,
    measure: sophia_renderer_live::CompositorTextRasterCache,
}
impl LiveMetadataShell {
    pub(in crate::live_session) fn launcher_busy(&self) -> bool {
        let l = &self.launcher;
        l.open.is_some()
            || l.refresh.is_some()
            || l.request.is_some()
            || l.pending.is_some()
            || l.presented.is_some()
            || l.grant.is_some()
            || l.launch.is_some()
    }
    pub(in crate::live_session) fn update_launcher_capture(
        &self,
        capture: &mut sophia_engine::LauncherCapture,
    ) {
        let l = &self.launcher;
        if let Some(p) = l.presented.as_ref().filter(|_| !l.revoked) {
            capture.present(
                Some((p.candidate.output, p.outcome.presentation_epoch)),
                p.candidate.selected,
                &p.targets,
                l.queued.is_some()
                    || l.request.is_some()
                    || l.pending.is_some()
                    || l.grant.is_some()
                    || l.launch.is_some(),
            );
        } else {
            capture.present(None, 0, &[], true);
        }
    }
    pub(in crate::live_session) fn queue_launcher(
        &mut self,
        output: OutputId,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if !self.connected
            || !self.transport.supports_launcher()
            || self.launcher_busy()
            || self.interaction_presented()
        {
            return Ok(false);
        }
        self.cancel_reference()?;
        self.launcher.revoked = false;
        self.launcher.query.clear();
        self.launcher.open = Some(output);
        Ok(true)
    }
    pub(in crate::live_session) fn cancel_launcher(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let l = &mut self.launcher;
        l.revoked = true;
        l.open = None;
        l.queued = None;
        l.presented = None;
        l.refresh = None;
        if let Some(mut p) = l.pending.take() {
            p.outcome.kind = ShellV1CandidateOutcomeKind::Superseded;
            self.transport.send_async(
                encode_shell_launcher_outcome(p.transaction, p.outcome)
                    .map_err(|e| format!("{e:?}"))?,
            )?;
        }
        Ok(())
    }
    pub(in crate::live_session) fn reset_launcher(&mut self) {
        // Keep the bounded worker until its result is drained. A transport
        // restart revokes every grant; it never detaches a background scan.
        let worker = self.launcher.worker.take();
        self.launcher = LiveLauncherSession {
            worker,
            revoked: true,
            ..Default::default()
        };
    }
    pub(in crate::live_session) fn launcher_input(
        &mut self,
        event: &sophia_engine::LauncherInputEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use sophia_engine::LauncherInput as I;
        let l = &mut self.launcher;
        let Some(p) = l.presented.as_ref() else {
            return Ok(());
        };
        if l.revoked
            || (event.output, event.presentation_epoch)
                != (p.candidate.output, p.outcome.presentation_epoch)
            || l.grant.is_some()
            || l.launch.is_some()
        {
            return Ok(());
        }
        let operation = match &event.input {
            I::Text(text) => {
                if l.query.len() + text.len() > SOPHIA_SHELL_MAX_QUERY_BYTES {
                    return Ok(());
                }
                l.query.push_str(text);
                ShellLauncherOperation::Query
            }
            I::Backspace => {
                l.query.pop();
                ShellLauncherOperation::Query
            }
            I::Clear => {
                l.query.clear();
                ShellLauncherOperation::Query
            }
            I::Next => ShellLauncherOperation::Next,
            I::Previous => ShellLauncherOperation::Previous,
            I::Dismiss => ShellLauncherOperation::Dismiss,
            I::Activate(slot) => {
                if l.queued.is_some()
                    || l.request.is_some()
                    || l.pending.is_some()
                    || !p.targets.iter().any(|(s, _)| s == slot)
                {
                    return Ok(());
                }
                let mut grant = ShellLauncherActivation {
                    connection_epoch: p.candidate.connection_epoch,
                    catalog_generation: p.candidate.catalog_generation,
                    request_generation: p.candidate.request_generation,
                    candidate_generation: p.candidate.candidate_generation,
                    presentation_epoch: p.outcome.presentation_epoch,
                    activation: 0,
                    slot: *slot,
                };
                let tx = self.take_transaction()?;
                grant.activation = tx.raw();
                self.transport.send_async(
                    encode_shell_launcher_activation(tx, grant).map_err(|e| format!("{e:?}"))?,
                )?;
                self.launcher.grant = Some((tx, grant, Instant::now() + Duration::from_secs(5)));
                return Ok(());
            }
        };
        if l.queued != Some(ShellLauncherOperation::Dismiss) {
            l.queued = Some(operation);
        }
        Ok(())
    }
    fn launcher_launch_outcome(
        &mut self,
        tx: TransactionId,
        activation: ShellLauncherActivation,
        status: ShellLaunchStatus,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.connected && activation.connection_epoch == self.transport.connection_epoch() {
            self.transport.send_async(
                encode_shell_launch_outcome(tx, ShellLaunchOutcome { activation, status })
                    .map_err(|e| format!("{e:?}"))?,
            )?;
        }
        crate::session_println!("sophia_launcher status=launch_outcome result={status:?}");
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub(in crate::live_session) fn service_launcher(
        &mut self,
        config: &PersistentXtermSessionConfig,
        xauthority: &std::path::Path,
        launches: &mut SessionLaunchQueue,
        children: &mut Vec<ManagedSessionChild>,
        admission_started: &mut Option<Instant>,
        runtime: &mut sophia_backend_live::LiveProductionVisualRuntime,
        scene: &sophia_renderer_live::LiveProductionCpuScene,
        mut native: Option<&mut sophia_backend_live::LiveProductionNativeScanout>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.launcher.worker.is_none()
            && let Some(catalog) = config.application_catalog.clone()
        {
            let registered = config
                .applications
                .applications
                .values()
                .map(|app| RegisteredCatalogApplication {
                    name: app.id.clone(),
                    command: ApplicationLaunchCommand {
                        executable: app.executable.clone(),
                        arguments: app.arguments.clone(),
                        working_directory: None,
                    },
                })
                .collect();
            let environment = ApplicationCatalogEnvironment {
                search_path: std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                    .filter(|p| p.is_absolute())
                    .collect(),
                locale: std::env::var("LC_ALL")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .or_else(|| std::env::var("LC_MESSAGES").ok().filter(|s| !s.is_empty()))
                    .or_else(|| std::env::var("LANG").ok())
                    .unwrap_or_else(|| "C".into()),
                current_desktop: std::env::var("XDG_CURRENT_DESKTOP")
                    .unwrap_or_else(|_| "Sophia".into())
                    .split(':')
                    .map(str::to_owned)
                    .collect(),
            };
            self.launcher.worker = Some(ApplicationCatalogWorker::start(
                catalog,
                registered,
                environment,
            )?);
        }
        if let Some(result) = self
            .launcher
            .worker
            .as_mut()
            .and_then(ApplicationCatalogWorker::poll)
        {
            self.launcher.worker_deadline = None;
            match result {
                ApplicationCatalogWorkerResult::Built(id, result)
                    if self.launcher.refresh == Some(id) =>
                {
                    self.launcher.refresh = None;
                    if !self.launcher.revoked
                        && let Some(output) = self.launcher.open.take()
                    {
                        match result {
                            Ok(catalog) => {
                                let descriptors = ShellApplicationCatalog {
                                    connection_epoch: self.transport.connection_epoch(),
                                    generation: id,
                                    entries: catalog
                                        .entries
                                        .iter()
                                        .map(|e| e.descriptor.clone())
                                        .collect(),
                                };
                                let tx = self.take_transaction()?;
                                self.launcher.outgoing =
                                    encode_shell_application_catalog(tx, &descriptors)
                                        .map_err(|e| format!("{e:?}"))?
                                        .into();
                                self.launcher.catalog = Some(catalog);
                                self.launcher.descriptors = Some(descriptors);
                                self.launcher.open = Some(output);
                                self.launcher.queued = Some(ShellLauncherOperation::Open);
                            }
                            Err(_) => {
                                crate::session_eprintln!(
                                    "sophia_launcher status=unavailable reason=catalog"
                                );
                                self.cancel_launcher()?;
                            }
                        }
                    }
                }
                ApplicationCatalogWorkerResult::Verified(id, result) => {
                    self.launcher.verifying = false;
                    if let Some((tx, grant)) = self.launcher.launch.take() {
                        let current = tx.raw() == id
                            && !self.launcher.revoked
                            && self.connected
                            && grant.connection_epoch == self.transport.connection_epoch()
                            && launches.catalog_admission(tx);
                        let result = if current {
                            result
                        } else {
                            Err("launch revoked".into())
                        };
                        let status = match result {
                            Ok(command) => {
                                // Filesystem work has completed on the worker. There is
                                // no further admission queue between revalidation and exec.
                                match spawn_catalog_child(command, config, xauthority, tx) {
                                    Ok(managed) => {
                                        children.push(managed);
                                        *admission_started = Some(Instant::now());
                                        ShellLaunchStatus::Started
                                    }
                                    Err(_) => {
                                        launches.cancel_catalog(tx);
                                        ShellLaunchStatus::Failed
                                    }
                                }
                            }
                            Err(_) => {
                                launches.cancel_catalog(tx);
                                ShellLaunchStatus::Rejected
                            }
                        };
                        self.launcher_launch_outcome(tx, grant, status)?;
                        if !self.launcher.revoked {
                            runtime.set_descriptor_overlay(None, scene, native.as_deref_mut())?;
                        }
                        self.cancel_launcher()?;
                    }
                }
                ApplicationCatalogWorkerResult::Unavailable => {
                    self.cancel_launcher()?;
                    runtime.set_descriptor_overlay(None, scene, native.as_deref_mut())?;
                }
                _ => {}
            }
        }
        if self
            .launcher
            .worker_deadline
            .is_some_and(|d| Instant::now() > d)
        {
            self.launcher.worker_deadline = None;
            if !self.launcher.revoked {
                runtime.set_descriptor_overlay(None, scene, native.as_deref_mut())?;
            }
            self.cancel_launcher()?;
            crate::session_eprintln!("sophia_launcher status=unavailable reason=worker_timeout");
        }
        if self.launcher.revoked
            && let Some((tx, grant)) = self.launcher.launch.take()
        {
            launches.cancel_catalog(tx);
            self.launcher_launch_outcome(tx, grant, ShellLaunchStatus::Rejected)?;
        }
        if !self.connected
            || !self.transport.supports_launcher()
            || config.application_catalog.is_none()
        {
            return Ok(());
        }
        if let Some(p) = self
            .launcher
            .presented
            .as_ref()
            .or(self.launcher.pending.as_ref())
            && self
                .outputs
                .get(&p.candidate.output)
                .is_none_or(|o| o.generation != p.output_generation || o.descriptor.is_none())
        {
            self.cancel_launcher()?;
            runtime.set_descriptor_overlay(None, scene, native.as_deref_mut())?;
        }
        if let Some(tx) = launches.take_catalog_dispatch() {
            if let Some((expected, grant)) = self.launcher.launch
                && tx == expected
                && !self.launcher.revoked
            {
                let entry = self
                    .launcher
                    .catalog
                    .as_ref()
                    .and_then(|c| c.entries.iter().find(|e| e.descriptor.slot == grant.slot))
                    .cloned();
                self.launcher.verifying = entry.is_some_and(|entry| {
                    self.launcher
                        .worker
                        .as_mut()
                        .is_some_and(|worker| worker.verify(tx.raw(), entry))
                });
                if self.launcher.verifying {
                    self.launcher.worker_deadline = Some(Instant::now() + Duration::from_secs(5));
                }
                if !self.launcher.verifying {
                    launches.cancel_catalog(tx);
                    self.launcher.launch = None;
                    self.launcher_launch_outcome(tx, grant, ShellLaunchStatus::Rejected)?;
                    self.cancel_launcher()?;
                    runtime.set_descriptor_overlay(None, scene, native.as_deref_mut())?;
                }
            } else {
                launches.cancel_catalog(tx);
            }
        }
        if let Some(frame) = self
            .transport
            .poll_kind(IpcMessageKind::ShellLauncherActivationAck)?
        {
            let (tx, ack) =
                decode_shell_launcher_activation_ack(&frame).map_err(|e| format!("{e:?}"))?;
            let (expected, grant, _) = self
                .launcher
                .grant
                .take()
                .ok_or("unsolicited launcher acknowledgement")?;
            if tx != expected || ack.activation != grant {
                return Err("stale launcher acknowledgement".into());
            }
            if !self.launcher.revoked
                && ack.consumed
                && matches!(
                    launches.enqueue_catalog(
                        SessionLaunchIntent {
                            transaction: tx,
                            application: LAUNCHER_APPLICATION_ID,
                            placement_classification: None
                        },
                        children.len()
                    ),
                    SessionLaunchQueueOutcome::Queued { .. }
                )
            {
                self.launcher.launch = Some((tx, grant));
            } else {
                self.launcher_launch_outcome(tx, grant, ShellLaunchStatus::Rejected)?;
                self.cancel_launcher()?;
                runtime.set_descriptor_overlay(None, scene, native.as_deref_mut())?;
            }
        }
        if let Some(mut p) = self.launcher.pending.take() {
            if Instant::now() > p.deadline {
                return Err("launcher presentation timed out".into());
            }
            if let Some(epoch) = runtime.descriptor_overlay_presentation_epoch(
                p.candidate.output,
                p.candidate.candidate_generation,
                p.candidate.visible,
            ) {
                p.outcome.presentation_epoch = epoch;
                p.outcome.kind = ShellV1CandidateOutcomeKind::Presented;
                self.transport.send_async(
                    encode_shell_launcher_outcome(p.transaction, p.outcome)
                        .map_err(|e| format!("{e:?}"))?,
                )?;
                self.launcher.presented = if p.candidate.visible { Some(p) } else { None };
            } else {
                self.launcher.pending = Some(p);
            }
        }
        if let Some(frame) = self
            .transport
            .poll_kind(IpcMessageKind::ShellLauncherCandidate)?
        {
            let (tx, candidate) =
                decode_shell_launcher_candidate(&frame).map_err(|e| format!("{e:?}"))?;
            let (expected, request, _) = self
                .launcher
                .request
                .take()
                .ok_or("unsolicited launcher candidate")?;
            if tx != expected
                || candidate.connection_epoch != request.connection_epoch
                || candidate.catalog_generation != request.catalog_generation
                || candidate.request_generation != request.request_generation
                || candidate.output != request.output
                || candidate.candidate_generation <= self.launcher.last_candidate
            {
                return Err("stale launcher candidate".into());
            }
            self.launcher.last_candidate = candidate.candidate_generation;
            let mut outcome = ShellLauncherOutcome {
                connection_epoch: candidate.connection_epoch,
                request_generation: candidate.request_generation,
                candidate_generation: candidate.candidate_generation,
                presentation_epoch: 0,
                kind: ShellV1CandidateOutcomeKind::Prepared,
            };
            if self.launcher.revoked
                || self.launcher.queued.is_some()
                || request.query != self.launcher.query
                || self.outputs.get(&candidate.output).is_none_or(|o| {
                    o.generation != request.output_generation || o.descriptor.is_none()
                })
            {
                outcome.kind = ShellV1CandidateOutcomeKind::Superseded;
                self.transport.send_async(
                    encode_shell_launcher_outcome(tx, outcome).map_err(|e| format!("{e:?}"))?,
                )?;
            } else {
                if candidate.visible != (request.operation != ShellLauncherOperation::Dismiss) {
                    return Err("launcher visibility disagrees with request".into());
                }
                let outputs = self
                    .outputs
                    .values()
                    .filter_map(|o| o.descriptor)
                    .collect::<Vec<_>>();
                let bounds = wm_output_bounds(&outputs)
                    .into_iter()
                    .find(|(o, _)| *o == candidate.output)
                    .ok_or("launcher output unavailable")?
                    .1;
                let projection = self.take_projection()?;
                let catalog = self
                    .launcher
                    .descriptors
                    .as_ref()
                    .ok_or("launcher catalog unavailable")?;
                let visual = sophia_engine::launcher_projection(
                    &candidate,
                    catalog,
                    &request.query,
                    projection,
                    bounds,
                    |text, size| self.launcher.measure.measure(text, size),
                )?;
                runtime.set_descriptor_overlay(
                    candidate.visible.then_some(visual.overlay),
                    scene,
                    native,
                )?;
                self.transport.send_async(
                    encode_shell_launcher_outcome(tx, outcome).map_err(|e| format!("{e:?}"))?,
                )?;
                self.launcher.pending = Some(PreparedLauncher {
                    transaction: tx,
                    candidate,
                    outcome,
                    targets: visual.targets,
                    output_generation: request.output_generation,
                    deadline: Instant::now() + Duration::from_secs(5),
                });
            }
        }
        if self
            .launcher
            .request
            .as_ref()
            .is_some_and(|(_, _, d)| Instant::now() > *d)
            || self
                .launcher
                .grant
                .as_ref()
                .is_some_and(|(_, _, d)| Instant::now() > *d)
        {
            return Err("launcher peer timed out".into());
        }
        // Large catalogs have a per-pass transfer budget, independent of the
        // rendering cadence and the worker's filesystem bounds.
        for _ in 0..32 {
            let Some(frame) = self.launcher.outgoing.pop_front() else {
                break;
            };
            self.transport.send_async(frame)?;
        }
        if !self.launcher.outgoing.is_empty()
            || self.launcher.request.is_some()
            || self.launcher.pending.is_some()
            || self.launcher.revoked
        {
            return Ok(());
        }
        if let Some(output) = self.launcher.open
            && self.launcher.queued.is_none()
            && self.launcher.refresh.is_none()
        {
            let id = self.take_snapshot_generation()?;
            if self.launcher.worker.as_mut().is_some_and(|w| w.refresh(id)) {
                self.launcher.refresh = Some(id);
                self.launcher.worker_deadline = Some(Instant::now() + Duration::from_secs(5));
            } else if self.launcher.worker.is_none() {
                self.launcher.open = None;
            }
            let _ = output;
            return Ok(());
        }
        let Some(operation) = self.launcher.queued.take() else {
            return Ok(());
        };
        let output = self
            .launcher
            .open
            .take()
            .or_else(|| self.launcher.presented.as_ref().map(|p| p.candidate.output))
            .ok_or("launcher input has no output")?;
        let Some(identity) = self
            .outputs
            .get(&output)
            .filter(|o| o.descriptor.is_some())
            .copied()
        else {
            self.cancel_launcher()?;
            return Ok(());
        };
        let tx = self.take_transaction()?;
        let request = ShellLauncherRequest {
            connection_epoch: self.transport.connection_epoch(),
            catalog_generation: self
                .launcher
                .descriptors
                .as_ref()
                .ok_or("launcher has no catalog")?
                .generation,
            request_generation: tx.raw(),
            output,
            output_generation: identity.generation,
            presentation_epoch: self
                .launcher
                .presented
                .as_ref()
                .map_or(0, |p| p.outcome.presentation_epoch),
            operation,
            query: self.launcher.query.clone(),
        };
        self.transport.send_async(
            encode_shell_launcher_request(tx, &request).map_err(|e| format!("{e:?}"))?,
        )?;
        self.launcher.request = Some((tx, request, Instant::now() + Duration::from_secs(5)));
        Ok(())
    }
}
