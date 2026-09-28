use super::*;
use sophia_protocol::*;

#[derive(Default)]
pub(super) struct LiveReferenceSession {
    catalog: Option<ShellShortcutCatalog>,
    catalog_source: Option<(sophia_config::ConfigGeneration, sophia_config::ConfigDigest)>,
    request: Option<(TransactionId, ShellReferenceRequest, Instant)>,
    pending: Option<(
        TransactionId,
        ShellReferenceCandidate,
        ShellReferenceOutcome,
    )>,
    presented: Option<(ShellReferenceCandidate, ShellReferenceOutcome, u64)>,
    presentation_deadline: Option<Instant>,
    queued: Option<(ShellReferenceOperation, OutputId)>,
    last_candidate: u64,
    startup_done: bool,
    cancelled_request: bool,
    measure: sophia_renderer_live::CompositorTextRasterCache,
}

impl LiveMetadataShell {
    pub(in crate::live_session) fn reference_input(&self) -> Option<(OutputId, u64)> {
        self.reference
            .presented
            .as_ref()
            .map(|(c, o, _)| (c.output, o.presentation_epoch))
    }

    pub(in crate::live_session) fn queue_reference(
        &mut self,
        operation: ShellReferenceOperation,
        output: OutputId,
    ) {
        if !self.connected || !self.transport.supports_reference() {
            return;
        }
        if operation == ShellReferenceOperation::Toggle {
            self.reference.startup_done = true;
        }
        if self
            .reference
            .queued
            .is_none_or(|(op, _)| op != ShellReferenceOperation::Dismiss)
        {
            self.reference.queued = Some((operation, output));
        }
    }

    pub(in crate::live_session) fn reference_busy(&self) -> bool {
        self.reference.presented.is_some()
            || self.reference.pending.is_some()
            || self.reference.request.is_some()
            || self.reference.queued.is_some()
    }

    pub(in crate::live_session) fn cancel_reference(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.reference.startup_done = true;
        self.reference.presented = None;
        self.reference.queued = None;
        self.reference.presentation_deadline = None;
        if let Some((tx, _, mut outcome)) = self.reference.pending.take() {
            outcome.kind = ShellV1CandidateOutcomeKind::Superseded;
            self.transport.send_reference_outcome(tx, outcome)?;
        }
        if self.reference.request.is_some() {
            self.reference.cancelled_request = true;
            self.transport.cancel_reference_request();
        } else {
            self.reference.catalog = None;
        }
        Ok(())
    }

    pub(in crate::live_session) fn reset_reference(&mut self) {
        let startup_done = self.reference.startup_done;
        self.reference = LiveReferenceSession {
            startup_done,
            ..Default::default()
        };
    }

    pub(in crate::live_session) fn service_reference(
        &mut self,
        shortcuts: Option<&sophia_config::DesktopShortcutCandidate>,
        output: OutputId,
        runtime: &mut sophia_backend_live::LiveProductionVisualRuntime,
        scene: &sophia_renderer_live::LiveProductionCpuScene,
        mut native: Option<&mut sophia_backend_live::LiveProductionNativeScanout>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if (self.launcher_busy()
            && self.reference.request.is_none()
            && self.reference.pending.is_none())
            || !self.connected
            || !self.transport.supports_shortcut_catalog()
        {
            return Ok(());
        }
        // An empty desktop can have a retired frame without a semantic input
        // epoch change. Native readiness comes from that actual frame.
        let output_presented = native.as_ref().map_or_else(
            || {
                runtime
                    .input_projections()
                    .iter()
                    .any(|p| p.output == output && p.epoch > 0)
            },
            |native| native.presented_output_frame(output).is_some(),
        );
        let Some(shortcuts) = shortcuts else {
            if self.reference_busy() {
                self.cancel_reference()?;
                runtime.set_descriptor_overlay(None, scene, native.as_deref_mut())?;
            }
            return Ok(());
        };
        if self
            .reference
            .presentation_deadline
            .is_some_and(|deadline| Instant::now() > deadline)
        {
            return Err("reference presentation timed out".into());
        }
        let epoch = self.transport.connection_epoch();
        let source = (shortcuts.generation, shortcuts.digest);
        let changed =
            self.reference.catalog.is_none() || self.reference.catalog_source != Some(source);
        let output_stale = self.reference.presented.as_ref().is_some_and(|(c, _, g)| {
            self.outputs
                .get(&c.output)
                .is_none_or(|o| o.generation != *g || o.descriptor.is_none())
        });
        if output_stale || changed && self.reference.catalog.is_some() {
            runtime.set_descriptor_overlay(None, scene, native.as_deref_mut())?;
            self.reference.presented = None;
            if self.reference.request.is_some() || self.reference.pending.is_some() {
                return Err("reference invalidated while pending".into());
            }
        }
        if changed {
            let generation = self.take_snapshot_generation()?;
            let catalog = ShellShortcutCatalog {
                connection_epoch: epoch,
                generation,
                entries: shortcut_rows(shortcuts),
            };
            let tx = self.take_transaction()?;
            self.transport.publish_shortcuts(tx, &catalog)?;
            self.reference.catalog = Some(catalog);
            self.reference.catalog_source = Some(source);
        }
        if !self.transport.supports_reference() {
            return Ok(());
        }
        if let Some((tx, candidate, mut outcome)) = self.reference.pending.take() {
            if let Some(presentation_epoch) = runtime.descriptor_overlay_presentation_epoch(
                candidate.output,
                candidate.candidate_generation,
                candidate.visible,
            ) {
                self.reference.presentation_deadline = None;
                outcome.presentation_epoch = presentation_epoch;
                outcome.kind = ShellV1CandidateOutcomeKind::Presented;
                self.transport.send_reference_outcome(tx, outcome)?;
                self.reference.presented = if candidate.visible {
                    Some((
                        candidate.clone(),
                        outcome,
                        self.outputs[&candidate.output].generation,
                    ))
                } else {
                    None
                };
                crate::session_println!(
                    "sophia_reference status=presented visible={} page={} pages={} generation={}",
                    candidate.visible,
                    outcome.page,
                    outcome.pages,
                    candidate.candidate_generation
                );
            } else {
                self.reference.pending = Some((tx, candidate, outcome));
            }
        }
        let event = self.transport.poll_reference_candidate()?;
        if let Some(sophia_runtime::ShellReferenceCandidateEvent::Refused(tx)) = &event
            && self
                .reference
                .request
                .as_ref()
                .is_some_and(|(expected, _, _)| expected == tx)
        {
            self.reference.request = None;
            if self.reference.cancelled_request {
                self.reference.cancelled_request = false;
                self.reference.catalog = None;
            }
            return Ok(());
        }
        if let Some(sophia_runtime::ShellReferenceCandidateEvent::Candidate(tx, candidate)) = event
        {
            let (expected, request, _) = self
                .reference
                .request
                .take()
                .ok_or("unsolicited reference candidate")?;
            if tx != expected
                || candidate.connection_epoch != epoch
                || candidate.catalog_generation != request.catalog_generation
                || candidate.request_generation != request.request_generation
                || candidate.output != request.output
                || candidate.candidate_generation <= self.reference.last_candidate
                || self.outputs.get(&candidate.output).is_none_or(|o| {
                    o.generation != request.output_generation || o.descriptor.is_none()
                })
            {
                self.transport.send_reference_outcome(
                    tx,
                    ShellReferenceOutcome {
                        connection_epoch: epoch,
                        catalog_generation: candidate.catalog_generation,
                        request_generation: candidate.request_generation,
                        candidate_generation: candidate.candidate_generation,
                        presentation_epoch: 0,
                        page: 0,
                        pages: 1,
                        kind: ShellV1CandidateOutcomeKind::Rejected,
                    },
                )?;
                return Ok(());
            }
            self.reference.last_candidate = candidate.candidate_generation;
            if self.reference.cancelled_request {
                self.reference.cancelled_request = false;
                self.reference.catalog = None;
                let outcome = ShellReferenceOutcome {
                    connection_epoch: epoch,
                    catalog_generation: request.catalog_generation,
                    request_generation: request.request_generation,
                    candidate_generation: candidate.candidate_generation,
                    presentation_epoch: 0,
                    page: 0,
                    pages: 1,
                    kind: ShellV1CandidateOutcomeKind::Superseded,
                };
                self.transport.send_reference_outcome(tx, outcome)?;
                return Ok(());
            }
            let bounds = self.outputs[&candidate.output]
                .descriptor
                .ok_or("reference output removed")?;
            let all = self
                .outputs
                .values()
                .filter_map(|o| o.descriptor)
                .collect::<Vec<_>>();
            let bounds = wm_output_bounds(&all)
                .into_iter()
                .find(|(o, _)| *o == bounds.id)
                .ok_or("reference bounds missing")?
                .1;
            let projection_id = self.take_projection()?;
            let catalog = self
                .reference
                .catalog
                .as_ref()
                .ok_or("reference catalog missing")?;
            let projection = sophia_engine::reference_sheet_projection(
                &candidate,
                catalog,
                projection_id,
                bounds,
                |text, size| self.reference.measure.measure(text, size),
            );
            let (projection, page, pages) = match projection {
                Ok(value) => value,
                Err(_) => {
                    self.transport.send_reference_outcome(
                        tx,
                        ShellReferenceOutcome {
                            connection_epoch: epoch,
                            catalog_generation: candidate.catalog_generation,
                            request_generation: candidate.request_generation,
                            candidate_generation: candidate.candidate_generation,
                            presentation_epoch: 0,
                            page: 0,
                            pages: 1,
                            kind: ShellV1CandidateOutcomeKind::Rejected,
                        },
                    )?;
                    return Ok(());
                }
            };
            runtime.set_descriptor_overlay(
                candidate.visible.then_some(projection),
                scene,
                native,
            )?;
            let outcome = ShellReferenceOutcome {
                connection_epoch: epoch,
                catalog_generation: request.catalog_generation,
                request_generation: request.request_generation,
                candidate_generation: candidate.candidate_generation,
                presentation_epoch: 0,
                page,
                pages,
                kind: ShellV1CandidateOutcomeKind::Prepared,
            };
            self.transport.send_reference_outcome(tx, outcome)?;
            self.reference.pending = Some((tx, candidate, outcome));
            self.reference.presentation_deadline = Some(Instant::now() + Duration::from_secs(5));
        }
        if self
            .reference
            .request
            .as_ref()
            .is_some_and(|(_, _, deadline)| Instant::now() > *deadline)
        {
            return Err("reference candidate timed out".into());
        }
        if self.reference.request.is_some()
            || self.reference.pending.is_some()
            || self.interaction_presented()
        {
            return Ok(());
        }
        if !self.reference.startup_done && output_presented {
            self.reference.startup_done = true;
            if self.reference.queued.is_none() {
                self.reference.queued = Some((ShellReferenceOperation::Startup, output));
            }
        }
        let Some((operation, requested_output)) = self.reference.queued.take() else {
            return Ok(());
        };
        let output = self
            .reference
            .presented
            .as_ref()
            .map_or(requested_output, |(c, _, _)| c.output);
        if self
            .outputs
            .get(&output)
            .is_none_or(|o| o.descriptor.is_none())
        {
            return Ok(());
        }
        if matches!(
            operation,
            ShellReferenceOperation::Next
                | ShellReferenceOperation::Previous
                | ShellReferenceOperation::Dismiss
        ) && self.reference.presented.is_none()
        {
            return Ok(());
        }
        let tx = self.take_transaction()?;
        let request = ShellReferenceRequest {
            connection_epoch: epoch,
            catalog_generation: self.reference.catalog.as_ref().unwrap().generation,
            request_generation: tx.raw(),
            output,
            output_generation: self.outputs[&output].generation,
            presentation_epoch: self
                .reference
                .presented
                .as_ref()
                .map_or(0, |(_, o, _)| o.presentation_epoch),
            operation,
        };
        self.transport.begin_reference_request(tx, request)?;
        self.reference.request = Some((tx, request, Instant::now() + Duration::from_secs(5)));
        Ok(())
    }
}

fn shortcut_rows(candidate: &sophia_config::DesktopShortcutCandidate) -> Vec<ShellShortcut> {
    use sophia_config::{DesktopShortcutModifiers as M, DesktopShortcutTarget as T};
    candidate
        .bindings
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let mut parts = Vec::new();
            for (bit, name) in [
                (M::SUPER, "Super"),
                (M::CONTROL, "Ctrl"),
                (M::SHIFT, "Shift"),
                (M::ALT, "Alt"),
            ] {
                if b.chord.modifiers.bits() & bit.bits() != 0 {
                    parts.push(name.to_owned());
                }
            }
            let trigger = match b.chord.trigger.as_str() {
                "slash" => "/".to_owned(),
                "return" => "Enter".to_owned(),
                t if b.chord.kind == sophia_config::DesktopShortcutBindingKind::Pointer => {
                    format!("Mouse {t}")
                }
                t => {
                    let mut c = t.chars();
                    c.next().map_or(String::new(), |first| {
                        first.to_uppercase().collect::<String>() + c.as_str()
                    })
                }
            };
            parts.push(trigger);
            let action = match &b.target {
                T::PolicyAction(n) => format!("policy:{n}"),
                T::Session(op) => format!("session:{}", op.profile_name()),
                T::LaunchApplication(name) => format!("application:{name}"),
            };
            ShellShortcut {
                slot: (i + 1) as u16,
                chord: parts.join("+"),
                action,
                label: b.label.clone(),
                group: b.group.clone(),
            }
        })
        .collect()
}
