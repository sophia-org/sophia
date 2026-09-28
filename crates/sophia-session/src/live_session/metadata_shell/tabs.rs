use super::*;
use sophia_protocol::*;

#[derive(Default)]
pub(super) struct LiveTabSession {
    groups: Vec<PolicyTabGroup>,
    policy_connection: Option<u64>,
    output_generations: Vec<(OutputId, u64)>,
    last_candidate: u64,
    cancelled_activations: Vec<TransactionId>,
    snapshot: Option<ShellTabSnapshot>,
    slots: BTreeMap<(OutputId, u64, SurfaceId), u16>,
    group_slots: BTreeMap<(OutputId, u64), u64>,
    next_group: u64,
    transaction: Option<TransactionId>,
    candidate: Option<ShellTabCandidate>,
    bars: Vec<sophia_engine::TabBarProjection>,
    presented: bool,
    activation: Option<(TransactionId, ShellV1Activation, SurfaceId, OutputId)>,
}

impl LiveMetadataShell {
    pub(in crate::live_session) fn is_tab_action(
        &self,
        action: ToplevelActionCapabilityRef,
    ) -> bool {
        self.tabs.presented
            && self
                .tabs
                .bars
                .iter()
                .any(|b| b.targets.iter().any(|t| t.action == action))
    }

    pub(in crate::live_session) fn queue_tab_action(
        &mut self,
        action: ToplevelActionCapabilityRef,
        activation: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.tabs.activation.is_some() {
            return Ok(());
        }
        let bar = self
            .tabs
            .bars
            .iter()
            .find(|b| b.targets.iter().any(|t| t.action == action))
            .ok_or("tab action not presented")?;
        let output = bar.output;
        let surface = self
            .tabs
            .slots
            .iter()
            .find(|(_, slot)| **slot == action.target_slot)
            .map(|(key, _)| key.2)
            .ok_or("tab slot missing")?;
        let c = self
            .tabs
            .candidate
            .as_ref()
            .ok_or("tab candidate missing")?;
        let event = ShellV1Activation {
            connection_epoch: c.connection_epoch,
            candidate_generation: c.candidate_generation,
            presentation_epoch: c.snapshot_generation,
            activation,
            action,
        };
        let tx = self.take_transaction()?;
        self.transport.queue_tab_activation(tx, event)?;
        self.tabs.activation = Some((tx, event, surface, output));
        Ok(())
    }

    pub(in crate::live_session) fn service_tabs(
        &mut self,
        publication: Option<sophia_engine::PolicyIndicatorPublication>,
        broker: &LiveMetadataBroker,
        runtime: &mut sophia_backend_live::LiveProductionVisualRuntime,
        scene: &sophia_backend_live::LiveProductionCpuScene,
        native: Option<&mut sophia_backend_live::LiveProductionNativeScanout>,
    ) -> Result<Vec<(SurfaceId, OutputId)>, Box<dyn std::error::Error>> {
        if !self.connected || !self.transport.supports_tabs() {
            return Ok(Vec::new());
        }
        self.transport.poll_io()?;
        let policy_connection = publication.as_ref().and_then(|p| p.connection_epoch);
        let groups = publication.map_or_else(Vec::new, |p| p.tab_groups);
        let output_generations = groups
            .iter()
            .map(|g| {
                (
                    g.output,
                    self.outputs.get(&g.output).map_or(0, |o| o.generation),
                )
            })
            .collect::<Vec<_>>();
        let members = groups
            .iter()
            .flat_map(|g| g.members.iter().copied())
            .collect::<BTreeSet<_>>();
        let sources = broker
            .shell_sources(&members)
            .into_iter()
            .map(|s| (s.surface, s))
            .collect::<BTreeMap<_, _>>();
        let mut snapshot = ShellTabSnapshot {
            connection_epoch: self.transport.connection_epoch(),
            generation: 1,
            groups: Vec::new(),
        };
        self.tabs.slots.retain(|(o, g, s), _| {
            groups
                .iter()
                .any(|group| group.output == *o && group.group == *g && group.members.contains(s))
        });
        self.tabs.group_slots.retain(|(o, g), _| {
            groups
                .iter()
                .any(|group| group.output == *o && group.group == *g)
        });
        let mut complete = true;
        for g in &groups {
            let key = (g.output, g.group);
            let slot = if let Some(slot) = self.tabs.group_slots.get(&key) {
                *slot
            } else {
                self.tabs.next_group = self
                    .tabs
                    .next_group
                    .checked_add(1)
                    .ok_or("tab group identity exhausted")?;
                self.tabs.group_slots.insert(key, self.tabs.next_group);
                self.tabs.next_group
            };
            let mut group = ShellTabGroup {
                slot,
                output: g.output,
                focused: g.focused,
                selected_slot: None,
                entries: Vec::new(),
            };
            for surface in &g.members {
                let Some(source) = sources.get(surface) else {
                    complete = false;
                    continue;
                };
                let key = (g.output, g.group, *surface);
                let slot = if let Some(slot) = self.tabs.slots.get(&key) {
                    *slot
                } else {
                    let slot = self.next_slot;
                    self.next_slot = slot
                        .checked_add(1)
                        .filter(|s| *s != u16::MAX)
                        .ok_or("tab slot identity exhausted")?;
                    self.tabs.slots.insert(key, slot);
                    slot
                };
                if g.selected == Some(*surface) {
                    group.selected_slot = Some(slot);
                }
                group.entries.push(ShellV1Descriptor {
                    slot,
                    generation: source.descriptor.generation,
                    label: source.descriptor.label.clone(),
                    trust_level: source.descriptor.trust_level,
                    attention: source.descriptor.attention,
                    action: ToplevelActionCapabilityRef {
                        token: source.grant.token,
                        issuer_epoch: broker.connection_epoch(),
                        issuer_revocation_epoch: source.grant.revocation_epoch,
                        recipient_epoch: self.transport.connection_epoch(),
                        target_slot: slot,
                        target_generation: source.grant.target_generation,
                    },
                });
            }
            snapshot.groups.push(group);
        }
        let changed = self.tabs.policy_connection != policy_connection
            || self.tabs.output_generations != output_generations
            || self.tabs.groups != groups
            || self.tabs.snapshot.as_ref().is_none_or(|old| {
                old.groups != snapshot.groups || old.connection_epoch != snapshot.connection_epoch
            });
        if changed {
            // New scene, labels, issuer, or connection revokes captured actions
            // immediately, before asynchronous shell work can finish.
            runtime.revoke_tab_interaction();
            let was_presented = self.tabs.presented;
            self.tabs.presented = false;
            if let Some((tx, _, _, _)) = self.tabs.activation.take() {
                if self.tabs.cancelled_activations.len() >= SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS {
                    return Err("cancelled tab acknowledgement bound exceeded".into());
                }
                self.tabs.cancelled_activations.push(tx);
            }
            if let (Some(tx), Some(c)) = (self.tabs.transaction, self.tabs.candidate.take())
                && !was_presented
            {
                // Presented already ended the response obligation; scene
                // invalidation must not emit a second terminal outcome.
                self.transport.send_tabs_outcome(
                    tx,
                    ShellV1CandidateOutcome {
                        connection_epoch: c.connection_epoch,
                        candidate_generation: c.candidate_generation,
                        presentation_epoch: 0,
                        kind: ShellV1CandidateOutcomeKind::Superseded,
                    },
                )?;
            }
            self.tabs.policy_connection = policy_connection;
            self.tabs.output_generations = output_generations;
            self.tabs.groups = groups;
            self.tabs.bars.clear();
            if complete {
                snapshot.generation = self.take_snapshot_generation()?;
                let tx = self.take_transaction()?;
                self.transport.publish_tabs(tx, &snapshot)?;
                self.tabs.transaction = Some(tx);
                self.tabs.snapshot = Some(snapshot);
            } else {
                snapshot.generation = 0;
                self.tabs.snapshot = Some(snapshot);
                self.tabs.transaction = None;
            }
        }
        while let Some((tx, c)) = self.transport.poll_tabs_candidate()? {
            let valid = self.tabs.transaction == Some(tx)
                && self.tabs.snapshot.as_ref().is_some_and(|s| {
                    s.connection_epoch == c.connection_epoch
                        && s.generation == c.snapshot_generation
                        && s.groups.iter().map(|g| g.slot).collect::<Vec<_>>() == c.groups
                })
                && c.candidate_generation > self.tabs.last_candidate
                && c.candidate_generation < (1 << 63);
            if !valid {
                self.transport.send_tabs_outcome(
                    tx,
                    ShellV1CandidateOutcome {
                        connection_epoch: self.transport.connection_epoch(),
                        candidate_generation: c.candidate_generation,
                        presentation_epoch: 0,
                        kind: ShellV1CandidateOutcomeKind::Superseded,
                    },
                )?;
                continue;
            }
            let snapshot = self.tabs.snapshot.as_ref().unwrap();
            self.tabs.bars = self
                .tabs
                .groups
                .iter()
                .zip(&snapshot.groups)
                .map(|(g, d)| {
                    sophia_engine::tab_bar_projection(
                        g,
                        c.candidate_generation | (1 << 63),
                        Some(d),
                    )
                })
                .collect();
            self.transport.send_tabs_outcome(
                tx,
                ShellV1CandidateOutcome {
                    connection_epoch: c.connection_epoch,
                    candidate_generation: c.candidate_generation,
                    presentation_epoch: 0,
                    kind: ShellV1CandidateOutcomeKind::Prepared,
                },
            )?;
            self.tabs.last_candidate = c.candidate_generation;
            self.tabs.candidate = Some(c);
        }
        runtime.set_tab_bars(self.tabs.bars.clone(), scene, native)?;
        if !self.tabs.presented
            && let Some(c) = self.tabs.candidate.as_ref()
            && runtime.tab_bars_presented(&self.tabs.bars)
        {
            self.transport.send_tabs_outcome(
                self.tabs.transaction.unwrap(),
                ShellV1CandidateOutcome {
                    connection_epoch: c.connection_epoch,
                    candidate_generation: c.candidate_generation,
                    presentation_epoch: c.snapshot_generation,
                    kind: ShellV1CandidateOutcomeKind::Presented,
                },
            )?;
            self.tabs.presented = true;
        }
        let mut focus = Vec::new();
        let mut cancelled = Vec::new();
        for tx in self.tabs.cancelled_activations.drain(..) {
            if self.transport.poll_tab_activation_ack(tx)?.is_none() {
                cancelled.push(tx);
            }
        }
        self.tabs.cancelled_activations = cancelled;
        if let Some((pending_tx, _, _, _)) = self.tabs.activation
            && let Some(ack) = self.transport.poll_tab_activation_ack(pending_tx)?
        {
            let Some((expected, event, surface, output)) = self.tabs.activation.take() else {
                return Ok(focus);
            };
            if pending_tx != expected {
                self.tabs.activation = Some((expected, event, surface, output));
                return Ok(focus);
            }
            if ack.connection_epoch != event.connection_epoch || ack.activation != event.activation
            {
                return Err("tab activation acknowledgement mismatch".into());
            }
            if ack.disposition == ShellV1ActivationDisposition::Consumed
                && self.is_tab_action(event.action)
                && broker.resolve_toplevel_action(event.action) == Some(surface)
            {
                focus.push((surface, output));
            }
        }
        Ok(focus)
    }
}
