use super::*;
use sophia_engine::{
    OverviewAuthority, OverviewInput, OverviewInputPresentation, OverviewSelection,
};
use sophia_protocol::*;

#[derive(Default)]
pub(super) struct LiveOverviewSession {
    authority: Option<OverviewAuthority>,
    output: Option<(OutputId, u64)>,
    topology: Vec<(OutputId, u64)>,
    queued: VecDeque<OverviewInput>,
    opening: Option<OutputId>,
    query: Option<(u64, Instant)>,
    request: Option<TransactionId>,
    deadline: Option<Instant>,
    suppress_input: bool,
}

impl LiveMetadataShell {
    pub(in crate::live_session) fn overview_input(&self) -> Option<OverviewInputPresentation> {
        if self.overview.suppress_input {
            return None;
        }
        self.overview
            .authority
            .as_ref()
            .and_then(OverviewAuthority::input)
    }

    pub(in crate::live_session) fn overview_busy(&self) -> bool {
        self.overview.authority.is_some() || self.overview.opening.is_some()
    }

    pub(in crate::live_session) fn queue_overview_toggle(
        &mut self,
        output: OutputId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.connected || !self.transport.supports_overview() {
            return Err("shell does not support overview".into());
        }
        if let Some(input) = self.overview_input() {
            self.queue_overview_input(OverviewInput {
                output: input.output,
                epoch: input.epoch,
                catalog: input.catalog,
                operation: ShellOverviewOperation::Toggle,
                workspace: 0,
                window: 0,
            })?;
        } else if !self.overview_busy() {
            self.overview.opening = Some(output);
        }
        Ok(())
    }

    pub(in crate::live_session) fn queue_overview_input(
        &mut self,
        input: OverviewInput,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(shown) = self.overview_input() else {
            return Ok(());
        };
        if (shown.output, shown.epoch, shown.catalog) != (input.output, input.epoch, input.catalog)
        {
            return Ok(());
        }
        if self.overview.queued.len() >= 32 {
            return Err("overview input queue capacity exceeded".into());
        }
        if matches!(
            input.operation,
            ShellOverviewOperation::Toggle
                | ShellOverviewOperation::Dismiss
                | ShellOverviewOperation::Accept
                | ShellOverviewOperation::Pick
        ) {
            self.overview.suppress_input = true;
        }
        self.overview.queued.push_back(input);
        Ok(())
    }

    pub(in crate::live_session) fn cancel_overview(&mut self) {
        if let Some(authority) = self.overview.authority.as_mut() {
            authority.revoke();
        }
        self.overview = LiveOverviewSession::default();
    }

    pub(in crate::live_session) fn service_overview(
        &mut self,
        wm: Option<&mut LiveWmSession>,
        runtime: &mut sophia_backend_live::LiveProductionVisualRuntime,
        scene: &sophia_renderer_live::LiveProductionCpuScene,
        mut native: Option<&mut sophia_backend_live::LiveProductionNativeScanout>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.overview_busy() {
            return Ok(());
        }
        let Some(wm) = wm else {
            return Err("overview lost its WM".into());
        };
        let publication = wm
            .overview_publication()
            .ok_or("overview WM capability unavailable")?;
        let topology: Vec<_> = self
            .outputs
            .iter()
            .filter(|(_, o)| o.descriptor.is_some())
            .map(|(id, o)| (*id, o.generation))
            .collect();
        if let Some(authority) = &self.overview.authority {
            if authority.publication != publication || topology != self.overview.topology {
                // An outstanding reply is no longer admissible. Reconnect the
                // shell through the existing error path rather than transplant it.
                self.cancel_overview();
                runtime.revoke_descriptor_overlay_interaction();
                runtime.set_descriptor_overlay(None, scene, native.as_deref_mut())?;
                return Err("overview publication or topology invalidated".into());
            }
        }
        if self
            .overview
            .deadline
            .is_some_and(|deadline| Instant::now() > deadline)
        {
            return Err("overview presentation timed out".into());
        }
        if let Some(output) = self.overview.opening {
            if self.overview.query.is_none() {
                if wm.enqueue_overview(None)? != LiveWmRequestAdmission::Admitted {
                    return Err("overview query admission refused".into());
                }
                self.overview.query = Some((
                    publication.generation,
                    Instant::now() + Duration::from_secs(5),
                ));
                return Ok(());
            }
            let (generation, deadline) = self.overview.query.unwrap();
            if publication.generation <= generation || publication.workspaces.is_empty() {
                if Instant::now() > deadline {
                    return Err("overview query timed out".into());
                }
                return Ok(());
            }
            let output_generation = self
                .outputs
                .get(&output)
                .filter(|o| o.descriptor.is_some())
                .ok_or("overview output disappeared")?
                .generation;
            let generation = self.take_snapshot_generation()?;
            let authority =
                OverviewAuthority::new(publication, self.transport.connection_epoch(), generation)?;
            let tx = self.take_transaction()?;
            for frame in encode_shell_overview_catalog(tx, &authority.catalog)
                .map_err(|e| format!("{e:?}"))?
            {
                self.transport.send_async(frame)?;
            }
            self.overview.authority = Some(authority);
            self.overview.output = Some((output, output_generation));
            self.overview.topology = topology;
            self.overview.opening = None;
            self.overview.query = None;
            self.overview.queued.push_back(OverviewInput {
                output,
                epoch: 0,
                catalog: generation,
                operation: ShellOverviewOperation::Toggle,
                workspace: 0,
                window: 0,
            });
        }
        let (output, output_generation) = self.overview.output.ok_or("overview output missing")?;
        if let Some(candidate) = self
            .overview
            .authority
            .as_ref()
            .and_then(|a| a.staged())
            .copied()
        {
            if let Some(epoch) = runtime.descriptor_overlay_presentation_epoch(
                output,
                candidate.candidate_generation,
                candidate.visible,
            ) {
                let selection = self.overview.authority.as_mut().unwrap().retire(epoch)?;
                let tx = self
                    .overview
                    .request
                    .take()
                    .ok_or("overview request missing")?;
                self.transport.send_async(
                    encode_shell_overview_outcome(
                        tx,
                        ShellV1CandidateOutcome {
                            connection_epoch: candidate.connection_epoch,
                            candidate_generation: candidate.candidate_generation,
                            presentation_epoch: epoch,
                            kind: ShellV1CandidateOutcomeKind::Presented,
                        },
                    )
                    .map_err(|e| format!("{e:?}"))?,
                )?;
                self.overview.deadline = None;
                if let Some(selection) = selection {
                    if wm.enqueue_overview(Some(selection))? != LiveWmRequestAdmission::Admitted {
                        return Err("overview selection admission refused".into());
                    }
                }
                if !candidate.visible {
                    self.cancel_overview();
                    return Ok(());
                }
            }
        }
        if let Some(frame) = self
            .transport
            .poll_kind(IpcMessageKind::ShellOverviewCandidate)?
        {
            let (tx, candidate) =
                decode_shell_overview_candidate(&frame).map_err(|e| format!("{e:?}"))?;
            if Some(tx) != self.overview.request {
                return Err("unsolicited overview transaction".into());
            }
            let projection = if candidate.visible {
                let bounds = wm_output_bounds(
                    &self
                        .outputs
                        .values()
                        .filter_map(|o| o.descriptor)
                        .collect::<Vec<_>>(),
                )
                .into_iter()
                .find(|(id, _)| *id == output)
                .ok_or("overview bounds missing")?
                .1;
                let projection_id = self.take_projection()?;
                let authority = self.overview.authority.as_ref().unwrap();
                Some(sophia_engine::overview_projection(
                    &authority.publication,
                    &authority.catalog,
                    &candidate,
                    output,
                    projection_id,
                    bounds,
                )?)
            } else {
                None
            };
            self.overview
                .authority
                .as_mut()
                .unwrap()
                .stage(candidate, projection.clone())?;
            if !candidate.visible {
                runtime.revoke_descriptor_overlay_interaction();
            }
            runtime.set_descriptor_overlay(
                projection.map(|p| p.overlay),
                scene,
                native.as_deref_mut(),
            )?;
            self.transport.send_async(
                encode_shell_overview_outcome(
                    tx,
                    ShellV1CandidateOutcome {
                        connection_epoch: candidate.connection_epoch,
                        candidate_generation: candidate.candidate_generation,
                        presentation_epoch: 0,
                        kind: ShellV1CandidateOutcomeKind::Prepared,
                    },
                )
                .map_err(|e| format!("{e:?}"))?,
            )?;
        }
        if self.overview.authority.as_ref().unwrap().busy() {
            return Ok(());
        }
        while let Some(input) = self.overview.queued.pop_front() {
            let shown = self.overview.authority.as_ref().unwrap().input();
            if input.epoch != shown.as_ref().map_or(0, |p| p.epoch) {
                continue;
            }
            let tx = self.take_transaction()?;
            let request = self.overview.authority.as_mut().unwrap().request(
                input.operation,
                output,
                output_generation,
                tx.raw(),
                (input.operation == ShellOverviewOperation::Pick)
                    .then_some((input.workspace, input.window)),
            )?;
            self.transport.send_async(
                encode_shell_overview_request(tx, request).map_err(|e| format!("{e:?}"))?,
            )?;
            self.overview.request = Some(tx);
            self.overview.deadline = Some(Instant::now() + Duration::from_secs(5));
            break;
        }
        Ok(())
    }
}
