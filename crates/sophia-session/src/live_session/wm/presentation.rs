// Lifecycle notifications have their own bounded storage. A blocked policy
// action cannot hold local withdrawal or input revocation hostage.
const MAX_POLICY_PRESENTATION_RECEIPTS: usize =
    4 * sophia_protocol::POLICY_MAX_PRESENTATION_OUTPUTS;

fn policy_presentation_heads(
    presentation: Option<&sophia_protocol::PolicyPresentation>,
    native: Option<&LiveProductionNativeScanout>,
) -> Vec<sophia_engine::HeadRenderTarget> {
    presentation
        .into_iter()
        .flat_map(|publication| &publication.outputs)
        .flat_map(|output| {
            native
                .into_iter()
                .flat_map(move |native| native.head_render_targets(output.output))
        })
        .collect()
}

impl LiveWmSession {
    fn enqueue_presented_action(
        &mut self,
        action: sophia_engine::PresentedPolicyAction,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(public) = self.public.as_mut() else {
            return Ok(());
        };
        if action.connection_epoch != public.connection_epoch
            || !public.presentation_input.action_is_current(
                action.connection_epoch,
                action.action,
                action.identity,
            )
            || !public.actions.iter().any(|registered| {
                registered.action == action.action && registered.session_operation_slot.is_none()
            })
        {
            return Ok(());
        }
        let affected_outputs = public
            .presentation_input
            .publication()
            .map(|(_, p)| p.outputs.iter().map(|output| output.output).collect())
            .unwrap_or_default();
        let activation_serial = public.mint_transaction()?.raw();
        let admission = public.queue_cause(LivePublicPolicyCause {
            source: LiveWmProposalSource::Action(action.action),
            cause: sophia_protocol::PolicyRequestCause::PresentationAction {
                activation_serial,
                action: action.action,
                identity: action.identity,
            },
            affected_outputs,
        });
        if admission == LiveWmRequestAdmission::RejectedCapacity {
            public.revoke_live_presentation();
        }
        Ok(())
    }
    /// Called at the production boundary before that cycle captures its frame.
    /// Passing no native target installs records without separately queueing a
    /// frame of the previous ordinary layout.
    fn install_committed_policy_presentation(
        &mut self,
        runtime: &mut LiveProductionVisualRuntime,
        scene: &LiveProductionCpuScene,
        native_retirement: bool,
        application_capture_active: bool,
        native: Option<&mut LiveProductionNativeScanout>,
        heads: &[sophia_engine::HeadRenderTarget],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(public) = self.public.as_mut() else {
            return Ok(());
        };
        if !native_retirement && public.reducer.presentation_publication().is_some() {
            public.revoke_live_presentation();
        }
        let desired =
            public
                .reducer
                .presentation_publication()
                .map(
                    |(owner_epoch, presentation)| sophia_backend_live::LivePolicyPresentation {
                        owner_epoch,
                        presentation: presentation.clone(),
                    },
                );
        // Replacement removes application hit layers on retirement. Keep the
        // currently installed scene until existing application sequences settle;
        // their lease eligibility must not be weakened to admit hidden pixels.
        if application_capture_active
            && desired.as_ref().is_some_and(|desired| {
                desired.presentation.outputs.iter().any(|output| {
                    output.mode == sophia_protocol::PolicyPresentationMode::ReplaceApplications
                })
            })
            && desired.as_ref() != runtime.policy_presentation()
        {
            return Ok(());
        }
        if desired.as_ref() != runtime.policy_presentation() {
            if desired.as_ref().is_some_and(|desired| {
                sophia_protocol::validate_policy_presentation_actions(
                    &desired.presentation,
                    &public.actions,
                )
                .is_err()
                    || runtime
                        .validate_policy_presentation_on_heads(desired, heads)
                        .is_err()
            }) {
                public.revoke_live_presentation();
                runtime.set_policy_presentation(None, scene, native)?;
                return Ok(());
            }
            runtime.set_policy_presentation(desired.clone(), scene, native)?;
        }
        if let Some(desired) = desired {
            let receipts = public
                .presentation_input
                .admit(desired.owner_epoch, desired.presentation)?;
            public.retain_presentation_receipts(receipts);
        } else {
            let receipts = public.presentation_input.revoke();
            public.retain_presentation_receipts(receipts);
        }
        public.presentation_withdrawal_pending = false;
        Ok(())
    }

    fn service_presented_policy(
        &mut self,
        runtime: &mut LiveProductionVisualRuntime,
        scene: &LiveProductionCpuScene,
        mut native: Option<&mut LiveProductionNativeScanout>,
        available: bool,
        application_capture_active: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let stopping = self.control_restart.is_some() || self.degraded;
        let Some(public) = self.public.as_mut() else {
            return Ok(());
        };
        public.revalidate_presentation_input();
        if let Some(revoked) = runtime.take_policy_presentation_revocation()
            && public
                .reducer
                .presentation_publication()
                .is_some_and(|(owner, publication)| {
                    owner == revoked.owner_epoch && publication.generation == revoked.generation
                })
        {
            public.revoke_live_presentation();
        }
        if (stopping || !available || public.transport_unavailable)
            && (public.presentation_input.publication().is_some()
                || public.reducer.presentation_publication().is_some())
        {
            public.revoke_live_presentation();
        }
        if public.presentation_withdrawal_pending {
            runtime.set_policy_presentation(None, scene, native.as_deref_mut())?;
            public.presentation_withdrawal_pending = false;
        }
        if available {
            public.settle_presented_withdrawals(runtime.input_projections());
        }
        public.observe_presented_policy(runtime.input_projections());
        // Capture completion need not produce another WM proposal or authority
        // batch. Retry at this regular frame/input service boundary and queue
        // the retained replacement through the existing native owner.
        if available && !stopping {
            let native_retirement = native.is_some();
            let heads = policy_presentation_heads(
                self.public
                    .as_ref()
                    .and_then(|public| public.reducer.presentation_publication().map(|(_, p)| p)),
                native.as_deref(),
            );
            self.install_committed_policy_presentation(
                runtime,
                scene,
                native_retirement,
                application_capture_active,
                native,
                &heads,
            )?;
        }
        Ok(())
    }

    fn preflight_staged_presentation(
        &self,
        runtime: Option<&LiveProductionVisualRuntime>,
        native: Option<&LiveProductionNativeScanout>,
    ) -> bool {
        let heads = policy_presentation_heads(
            self.public
                .as_ref()
                .and_then(|public| public.staged.as_ref())
                .and_then(|staged| staged.presentation_publication().map(|(_, p)| p)),
            native,
        );
        self.preflight_staged_presentation_on_heads(runtime, &heads)
    }

    fn preflight_staged_presentation_on_heads(
        &self,
        runtime: Option<&LiveProductionVisualRuntime>,
        heads: &[sophia_engine::HeadRenderTarget],
    ) -> bool {
        let Some(public) = &self.public else {
            return true;
        };
        let Some(staged) = &public.staged else {
            return true;
        };
        let Some((owner_epoch, presentation)) = staged.presentation_publication() else {
            return true;
        };
        public.native_presentation_capable
            && sophia_protocol::validate_policy_presentation_actions(presentation, &public.actions)
                .is_ok()
            && public.presentation_receipts.len()
                + public.presentation_withdrawals.len()
                + 2 * sophia_protocol::POLICY_MAX_PRESENTATION_OUTPUTS
                <= MAX_POLICY_PRESENTATION_RECEIPTS
            && runtime.is_some_and(|runtime| {
                runtime
                    .validate_policy_presentation_on_heads(
                        &sophia_backend_live::LivePolicyPresentation {
                            owner_epoch,
                            presentation: presentation.clone(),
                        },
                        heads,
                    )
                    .is_ok()
            })
    }

    fn reject_staged_presentation(
        &mut self,
        identity: LivePolicySettlementIdentity,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let public = self
            .public
            .as_mut()
            .ok_or("presentation refusal lost policy owner")?;
        public.staged = None;
        public.prepared = None;
        public.reducer.timeout(identity.request_id);
        let outcome = sophia_protocol::PolicyProjectionOutcome::RejectedInvalid;
        public.submit_or_defer(PolicyTransportCommand::ProjectionOutcome {
            transaction: identity.transaction,
            request_id: identity.request_id,
            scene_generation: public.reducer.scene().generation,
            outcome,
            expect_session_operation: false,
        })?;
        public.settle_public_projection(outcome);
        Ok(())
    }
}

impl LivePublicPolicyState {
    fn observe_presented_policy(
        &mut self,
        projections: &[sophia_backend_live::LivePresentedInputProjection],
    ) {
        for projection in projections {
            if !projection.frame_completed {
                if projection.policy_publication.is_some()
                    || self
                        .presentation_input
                        .output_receipt(projection.output)
                        .is_some()
                {
                    self.revoke_live_presentation();
                }
                continue;
            }
            let Some(stamp) = &projection.policy_publication else {
                if self
                    .presentation_input
                    .output_receipt(projection.output)
                    .is_some()
                {
                    self.revoke_live_presentation();
                }
                continue;
            };
            let Some((owner, publication)) = self.presentation_input.publication() else {
                continue;
            };
            if owner != stamp.owner_epoch || publication.generation != stamp.generation {
                continue;
            }
            // A stamp certifies the publication, while target membership also
            // proves that the captured frame did not omit a required draw.
            let complete_targets = publication
                .instances
                .iter()
                .filter(|i| i.output == stamp.output)
                .all(|i| stamp.instances.contains(&(i.id, i.generation)))
                && publication
                    .regions
                    .iter()
                    .filter(|r| r.output == stamp.output)
                    .all(|r| stamp.regions.contains(&(r.id, r.generation)));
            if !complete_targets {
                self.revoke_live_presentation();
                break;
            }
            if let Some(receipt) =
                self.presentation_input
                    .complete(sophia_engine::PolicyPresentationCompletion {
                        owner_epoch: stamp.owner_epoch,
                        publication_generation: stamp.generation,
                        output: stamp.output,
                        output_generation: stamp.output_generation,
                    })
            {
                self.retain_presentation_receipts([receipt]);
            }
        }
    }
    fn presented_cause_is_current(&self, cause: sophia_protocol::PolicyRequestCause) -> bool {
        match cause {
            sophia_protocol::PolicyRequestCause::PresentationAction {
                action, identity, ..
            } => {
                self.presentation_input
                    .action_is_current(self.connection_epoch, action, identity)
                    && self.actions.iter().any(|registered| {
                        registered.action == action && registered.session_operation_slot.is_none()
                    })
            }
            _ => true,
        }
    }
    fn retain_presentation_receipts(
        &mut self,
        receipts: impl IntoIterator<Item = sophia_protocol::PolicyPresentationReceipt>,
    ) {
        for receipt in receipts {
            let withdrawing =
                receipt.outcome == sophia_protocol::PolicyPresentationOutcome::Revoked;
            if self.presentation_receipts.len()
                + self.presentation_withdrawals.len()
                + 1
                + usize::from(withdrawing)
                > MAX_POLICY_PRESENTATION_RECEIPTS
            {
                self.presentation_input.revoke();
                self.reducer.revoke_presentation();
                self.presentation_withdrawal_pending = true;
                self.transport_unavailable = true;
                self.fence_inspection(0, None);
                return;
            }
            if withdrawing {
                self.presentation_withdrawals.push_back(receipt);
            }
            self.presentation_receipts.push_back(receipt);
        }
    }

    fn settle_presented_withdrawals(
        &mut self,
        projections: &[sophia_backend_live::LivePresentedInputProjection],
    ) {
        let mut unsettled = VecDeque::new();
        while let Some(mut receipt) = self.presentation_withdrawals.pop_front() {
            let replaced = projections
                .iter()
                .find(|projection| projection.output == receipt.output)
                .is_some_and(|projection| {
                    projection.frame_completed
                        && (!projection.policy_visible
                            || projection.policy_publication.as_ref().is_some_and(|stamp| {
                                stamp.owner_epoch != receipt.connection_epoch
                                    || stamp.generation != receipt.publication_generation
                                    || stamp.output_generation != receipt.output_generation
                            }))
                });
            if replaced {
                receipt.outcome = sophia_protocol::PolicyPresentationOutcome::Withdrawn;
                // Moving a pending terminal identity to its delivery queue
                // consumes no additional bounded lifecycle storage.
                self.presentation_receipts.push_back(receipt);
            } else {
                unsettled.push_back(receipt);
            }
        }
        self.presentation_withdrawals = unsettled;
    }

    fn revoke_live_presentation(&mut self) {
        self.presentation_scene_dirty = true;
        self.pending_dirty_outputs
            .extend(self.live_output_ids.iter().copied());
        self.presentation_capture.revoke();
        let receipts = self.presentation_input.revoke();
        self.reducer.revoke_presentation();
        self.presentation_withdrawal_pending = true;
        self.retain_presentation_receipts(receipts);
    }

    fn revalidate_presentation_input(&mut self) {
        let admitted = self.reducer.presentation_publication();
        let current = self.presentation_input.publication();
        let catalog_valid = current.is_none_or(|(_, publication)| {
            sophia_protocol::validate_policy_presentation_actions(publication, &self.actions)
                .is_ok()
        });
        if !catalog_valid {
            self.revoke_live_presentation();
        } else if current.is_some() && current != admitted {
            self.presentation_withdrawal_pending = admitted.is_none();
            if admitted.is_none() {
                self.presentation_scene_dirty = true;
                self.pending_dirty_outputs
                    .extend(self.live_output_ids.iter().copied());
            }
            let receipts = self.presentation_input.revoke();
            self.retain_presentation_receipts(receipts);
        }
    }

    fn flush_presentation_receipts(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        if self.deferred_command.is_some() || self.transport_unavailable {
            return Ok(false);
        }
        let mut submitted = false;
        while let Some(receipt) = self.presentation_receipts.front().copied() {
            if receipt.connection_epoch != self.connection_epoch {
                self.presentation_receipts.pop_front();
                continue;
            }
            let transaction = self.mint_transaction()?;
            let Some(worker) = self.worker.as_ref() else {
                break;
            };
            if worker
                .try_command(PolicyTransportCommand::PresentationReceipt {
                    transaction,
                    receipt,
                })
                .is_err()
            {
                break;
            }
            self.presentation_receipts.pop_front();
            self.note_inspection_event(Some(InspectionEvent::PresentationChanged));
            submitted = true;
        }
        Ok(submitted)
    }
}
