impl PersistentLiveLayout {
    fn new(policy_map_mode: LivePolicyMapMode, output: Size) -> Self {
        Self {
            bypass_policy_admission: policy_map_mode.bypass_engine_admission(),
            engine_owns_initial_placement: policy_map_mode.engine_owns_initial_placement(),
            stage_new_surfaces_offset: policy_map_mode.frontend_deferred(),
            // Without an external WM, the Engine owns initial placement. Keep
            // the first toplevel's extent intact and center the output space
            // available for compositor-owned chrome.
            center_first_surface_in: policy_map_mode
                .engine_owns_initial_placement()
                .then_some(output),
            ..Self::default()
        }
    }

    fn queue_focus_handoff(&mut self, transaction: TransactionId, surface: SurfaceId) {
        self.focus_to_apply = Some((transaction, surface));
    }

    fn observe_authority_batch(
        &mut self,
        batch: &XAuthorityObservedTransactionBatch,
    ) -> LiveAuthorityLayoutObservation {
        let mut output_reservations_changed = false;
        let mut admission_group_error = None;
        let mut admission_group_overflowed = false;
        let mut new_surfaces = BTreeSet::new();
        let mut withdrawn_surfaces = BTreeSet::new();
        let client_route_invalid = self.client_routes.observe(batch).is_err();
        self.observe_presentation_intents(
            batch,
            &mut new_surfaces,
            &mut withdrawn_surfaces,
        );
        for presentation in &batch.surface_presentations {
            if !withdrawn_surfaces.contains(&presentation.surface) {
                let facts = sophia_engine::SurfaceLayoutFacts {
                    surface: presentation.surface,
                    role: presentation.role,
                    kind: presentation.kind,
                    placement_preference: presentation.placement_preference,
                    presentation_owner: presentation.owner,
                    stack_rank: presentation.stack_rank,
                    geometry: presentation.geometry,
                    constraints: presentation.constraints,
                    generation: presentation.generation,
                };
                let previous = self
                    .authority_surface_facts
                    .insert(presentation.surface, facts);
                if previous.as_ref() != Some(&facts) {
                    self.manage_settlements.remove(&presentation.surface);
                }
            }
            let previous_role = self
                .presentation_roles
                .insert(presentation.surface, presentation.role);
            // Mapped state is not part of `SurfaceLayoutFacts`, so a map or unmap
            // is its own re-arm: policy is being shown a different surface set.
            let mapped_changed = if presentation.mapped {
                self.mapped_surfaces.insert(presentation.surface)
            } else {
                self.mapped_surfaces.remove(&presentation.surface)
            };
            if mapped_changed {
                self.manage_settlements.remove(&presentation.surface);
            }
            if let Some(owner) = presentation.owner {
                self.presentation_owners.insert(presentation.surface, owner);
            } else {
                self.presentation_owners.remove(&presentation.surface);
            }
            self.surface_kinds
                .insert(presentation.surface, presentation.kind);
            self.placement_preferences
                .insert(presentation.surface, presentation.placement_preference);
            self.authority_stack_ranks
                .insert(presentation.surface, presentation.stack_rank);
            if previous_role
                == Some(sophia_protocol::SurfacePresentationRole::PolicyManaged)
                && presentation.role
                    == sophia_protocol::SurfacePresentationRole::ClientPositioned
            {
                withdrawn_surfaces.insert(presentation.surface);
                self.admissions.remove(presentation.surface);
                self.planning_surfaces.remove(&presentation.surface);
                self.unmanaged_surfaces.remove(&presentation.surface);
            } else if previous_role
                == Some(sophia_protocol::SurfacePresentationRole::ClientPositioned)
                && presentation.role
                    == sophia_protocol::SurfacePresentationRole::PolicyManaged
                && presentation.mapped
            {
                let intent = sophia_protocol::SurfacePresentationIntent {
                    surface: presentation.surface,
                    kind: sophia_protocol::SurfacePresentationIntentKind::Request,
                    role: presentation.role,
                    surface_kind: presentation.kind,
                    placement_preference: presentation.placement_preference,
                    presentation_owner: presentation.owner,
                    stack_rank: presentation.stack_rank,
                    geometry: presentation.geometry,
                    constraints: presentation.constraints,
                    generation: presentation.generation,
                };
                let facts = sophia_engine::SurfaceLayoutFacts::from(intent);
                self.planning_surfaces.insert(presentation.surface, facts);
                if !self.bypass_policy_admission {
                    self.admissions.observe_intent(intent);
                    self.unmanaged_surfaces.insert(presentation.surface);
                    self.layout_epochs.set_admission(
                        presentation.surface,
                        sophia_engine::SurfaceAdmissionState::Unmanaged,
                    );
                }
                new_surfaces.insert(presentation.surface);
            }
            self.layout_epochs
                .set_declared_constraints(presentation.surface, presentation.constraints);
            output_reservations_changed |= self.output_reservations.observe_presentation(
                presentation.surface,
                presentation.role,
                presentation.mapped,
            );
            if let Some(layer) = self.layers.get_mut(&presentation.surface)
                && presentation.role
                    == sophia_protocol::SurfacePresentationRole::ClientPositioned
            {
                layer.geometry = presentation.geometry;
                layer.stack_rank = (u32::MAX / 2).saturating_add(
                    presentation.stack_rank.min(u32::MAX / 2),
                );
            }
            match presentation.role {
                sophia_protocol::SurfacePresentationRole::PolicyManaged => {
                    if !self.bypass_policy_admission
                        && presentation.mapped
                        && self.layers.contains_key(&presentation.surface)
                    {
                        self.unmanaged_surfaces.insert(presentation.surface);
                    }
                }
                sophia_protocol::SurfacePresentationRole::ClientPositioned => {
                    self.unmanaged_surfaces.remove(&presentation.surface);
                }
            }
        }
        for snapshot in &batch.surface_output_reservations {
            output_reservations_changed |=
                self.output_reservations.observe_reservations(snapshot.clone());
        }
        for registration in &batch.dma_buf_registrations {
            self.dma_buf_sizes
                .insert(registration.descriptor.handle, registration.descriptor.size);
        }
        for update in &batch.cpu_buffer_updates {
            match update {
                sophia_x_authority::XAuthorityCpuBufferUpdate::Replace(buffer) => {
                    self.cpu_buffer_sizes.insert(buffer.handle, buffer.size);
                }
                sophia_x_authority::XAuthorityCpuBufferUpdate::Patch(patch) => {
                    self.cpu_buffer_sizes.insert(patch.handle, patch.size);
                }
                sophia_x_authority::XAuthorityCpuBufferUpdate::PatchBatch(batch) => {
                    self.cpu_buffer_sizes.insert(batch.handle, batch.size);
                }
            }
        }
        match self.observe_pre_admission_groups(batch) {
            Ok(overflowed) => admission_group_overflowed |= overflowed,
            Err(error) => admission_group_error = Some(error),
        }
        for handle in &batch.released_dma_bufs {
            if self.admission_groups_reference_dma_buf(*handle) {
                self.deferred_dma_buf_releases.insert(*handle);
            } else {
                self.dma_buf_sizes.remove(handle);
            }
        }
        for handle in &batch.released_fences {
            if self.admission_groups_reference_fence(*handle) {
                self.deferred_fence_releases.insert(*handle);
            }
        }
        for surface in &batch.removed_surfaces {
            output_reservations_changed |= self.output_reservations.remove_surface(*surface);
        }
        self.remove_surfaces(&batch.removed_surfaces);
        for (index, transaction) in batch.transactions.iter().enumerate() {
            // A frame presented before its window mapped is skipped by
            // production the moment the map is acknowledged; nobody will ever
            // see it. Observing it here would still record its extent as a
            // safe observation, and the launch epoch reads that as pixels it
            // can resize: the surface is then held in the gate instead of
            // being deferred out of it like a window that drew nothing, and
            // the launch waits on a frame the client sent to a window that
            // did not exist yet. Kitty draws exactly one such frame, one
            // request ahead of its MapWindow.
            if self.present_escaped_admission(transaction.surface) {
                continue;
            }
            let observed_size = live_transaction_observed_size(
                transaction,
                &self.dma_buf_sizes,
                &self.cpu_buffer_sizes,
            );
            if !self
                .layout_epochs
                .accept_observation(transaction.surface, observed_size)
            {
                continue;
            }
            let visual_evidence = live_transaction_visual_evidence(transaction, batch);
            let candidate_selected = self.layout_epochs.record_safe_observation(
                transaction.key(),
                observed_size,
                visual_evidence,
            );
            if candidate_selected && self.surface_requires_admission(transaction.surface) {
                crate::session_println!(
                    "sophia_live_visual_candidate schema=1 status=selected transaction={} surface={} width={} height={} evidence={:?}",
                    transaction.transaction.raw(),
                    transaction.surface.index(),
                    observed_size.width,
                    observed_size.height,
                    visual_evidence,
                );
                let (source, buffer) = match transaction.target_buffer() {
                    BufferSource::DmaBuf { handle } => ("dma_buf", handle),
                    BufferSource::CpuBuffer { handle } => ("cpu_buffer", handle),
                    BufferSource::XPixmap { pixmap } => ("x_pixmap", u64::from(pixmap)),
                    BufferSource::None => ("none", 0),
                };
                crate::session_println!(
                    "sophia_live_visual_candidate_identity schema=1 status=selected transaction={} surface={} source={source} buffer={buffer}",
                    transaction.transaction.raw(),
                    transaction.surface.index(),
                );
                // The strongest complete pixels can race the layout that
                // admitted the surface. Keep the temporary constraint aligned
                // with the exact retained candidate until native retirement
                // owns it; candidate-less geometry cannot cross quarantine.
                let admission_extent =
                    self.synchronize_admission_extent(transaction.surface);
                // Arming happens inside a layout commit, and a first frame
                // usually arrives seconds after its launch layout committed.
                // A pending layout can be just as unable to stage it: a
                // pixel-silent launch is deliberately removed from that
                // epoch's requested-size gate and retained as a standing
                // target. Queue recovery unless the live epoch owns this exact
                // measured extent. Selection reports true only when the
                // candidate is new, so this fires once per frame identity, not
                // per observation.
                let candidate_owned_by_pending = self.pending.as_ref().is_some_and(|pending| {
                    pending.requested_sizes.get(&transaction.surface) == Some(&observed_size)
                });
                let candidate_can_drive_admission = matches!(
                    admission_extent,
                    AdmissionRecoveryExtentDecision::Unchanged { .. }
                        | AdmissionRecoveryExtentDecision::Update { .. }
                );
                if candidate_can_drive_admission && !candidate_owned_by_pending {
                    self.constraint_relayout_required = true;
                }
            }
            let standing_recovery_candidate = self.arm_standing_recovery_candidate(
                transaction,
                observed_size,
                visual_evidence,
                candidate_selected,
            );
            if standing_recovery_candidate {
                // Retain the successor's pixels under the currently committed
                // geometry. The queued relayout changes geometry only after
                // the temporary admission constraint has been removed.
                if let Some(layer) = self.layers.get_mut(&transaction.surface) {
                    layer.source = transaction.target_buffer();
                    layer.damage = transaction.damage.clone();
                    layer.generation =
                        transaction.previous_committed_generation.saturating_add(1);
                }
            }
            let resize_owned = self.pending.as_ref().is_some_and(|pending| {
                pending.requested_sizes.contains_key(&transaction.surface)
            }) || self
                .awaiting_visual_commits
                .surface_awaiting(transaction.surface)
                || self
                    .layout_epochs
                    .pending_target(transaction.surface)
                    .is_some();
            let staged_for_resize = self.pending.as_ref().is_some_and(|pending| {
                pending.requested_sizes.get(&transaction.surface) == Some(&observed_size)
            });
            if resize_owned {
                let selected_for_admission = !self.surface_requires_admission(transaction.surface)
                    || self
                        .layout_epochs
                        .safe_observation(transaction.surface)
                        .is_some_and(|selected| {
                            selected.candidate == Some(transaction.key())
                        });
                let evidence_allowed = self
                    .layout_epochs
                    .resize_evidence_allowed(transaction.surface, visual_evidence);
                if staged_for_resize && selected_for_admission && evidence_allowed {
                    let pending = self.pending.as_mut().expect("checked above");
                    pending
                        .staged_transactions
                        .insert(transaction.surface, transaction.clone());
                    if let Some(layer) = pending
                        .layers
                        .iter_mut()
                        .find(|layer| layer.surface == transaction.surface)
                    {
                        layer.source = transaction.target_buffer();
                        layer.damage = transaction.damage.clone();
                        layer.generation =
                            transaction.previous_committed_generation.saturating_add(1);
                    }
                }
                continue;
            }
            if self.surface_requires_admission(transaction.surface) {
                continue;
            }
            self.layout_epochs
                .record_committed(transaction.surface, observed_size);
            let observed_layer = match self.layers.get_mut(&transaction.surface) {
                Some(layer) => {
                    if self.presentation_roles.get(&transaction.surface)
                        == Some(&sophia_protocol::SurfacePresentationRole::ClientPositioned)
                    {
                        layer.geometry = transaction.target_geometry;
                    }
                    layer.source = transaction.target_buffer();
                    layer.damage = transaction.damage.clone();
                    layer.generation = transaction.previous_committed_generation.saturating_add(1);
                    layer.clone()
                }
                None => {
                    new_surfaces.insert(transaction.surface);
                    let policy_managed = self.presentation_roles.get(&transaction.surface)
                        != Some(&sophia_protocol::SurfacePresentationRole::ClientPositioned);
                    if !self.bypass_policy_admission
                        && policy_managed
                        && matches!(
                            self.admissions.state(transaction.surface),
                            sophia_engine::SurfacePresentationAdmissionState::PolicyPending
                        )
                    {
                        self.unmanaged_surfaces.insert(transaction.surface);
                        self.layout_epochs.set_admission(
                            transaction.surface,
                            sophia_engine::SurfaceAdmissionState::Unmanaged,
                        );
                    }
                    let mut geometry = transaction.target_geometry;
                    if policy_managed && self.stage_new_surfaces_offset {
                        geometry.x = geometry.x.saturating_add(80);
                        geometry.y = geometry.y.saturating_add(60);
                    } else if policy_managed
                        && let Some(output) = self.center_first_surface_in.take()
                    {
                        geometry = center_geometry_without_scaling(geometry, output);
                    }
                    let layer = LayerSnapshot {
                        input_region: None,
                        translation: None,
            surface: transaction.surface,
                        authority_local_id: None,
                        // Not a policy placement: this is the layout's own
                        // record of a transaction. The proposal sets the owner.
                        output: None,
                        namespace: None,
                        stack_rank: if policy_managed {
                            u32::try_from(index).unwrap_or(u32::MAX - 1)
                        } else {
                            (u32::MAX / 2).saturating_add(
                                self.authority_stack_ranks
                                    .get(&transaction.surface)
                                    .copied()
                                    .unwrap_or_default()
                                    .min(u32::MAX / 2),
                            )
                        },
                        geometry,
                        source: transaction.target_buffer(),
                        // What the buffer registry measured. `observed_size`
                        // beside it answers a different question -- whether the
                        // surface reached its configured extent -- and reports
                        // the logical size when it has, which is the placement
                        // rather than the raster.
                        source_size: live_transaction_raster_size(
                            transaction,
                            &self.dma_buf_sizes,
                            &self.cpu_buffer_sizes,
                        ),
                        damage: transaction.damage.clone(),
                        opacity: 1.0,
                        crop: None,
                        transform: Transform::IDENTITY,
                        generation: transaction.previous_committed_generation.saturating_add(1),
                        resize_sync: ResizeSyncCapability::ImplicitOnly,
                    };
                    self.layers.insert(transaction.surface, layer.clone());
                    layer
                }
            };
            self.merge_unrequested_observation_into_pending(observed_layer);
        }
        LiveAuthorityLayoutObservation {
            new_surfaces: new_surfaces.into_iter().collect(),
            withdrawn_surfaces: withdrawn_surfaces.into_iter().collect(),
            output_reservations_changed,
            admission_group_error,
            admission_group_overflowed,
            client_route_invalid,
        }
    }
}
