/// Re-drive attempts against one standing target before the session says so.
///
/// A standing target is re-injected until the surface commits at exactly that
/// size, so a surface that commits at any other size loops forever. The loop is
/// idempotent and cheap, which is why it went unnoticed; this is the point at
/// which repeating stops being indistinguishable from converging. Reporting, not
/// a cap: bounding the loop before knowing why it does not converge would trade a
/// visible symptom for an invisible one.
const STANDING_TARGET_REDRIVE_REPORT_THRESHOLD: u32 = 8;

struct PendingLiveWmLayout {
    transaction: TransactionId,
    layers: Vec<LayerSnapshot>,
    requested_sizes: BTreeMap<SurfaceId, Size>,
    presentation_states: BTreeMap<SurfaceId, sophia_protocol::PolicyPresentationState>,
    presentation_settlements: BTreeSet<SurfaceId>,
    configure_deliveries: usize,
    focus: Option<SurfaceId>,
    deadline: Instant,
    update: WmTransactionUpdate,
    moved_surfaces: usize,
    staged_transactions: BTreeMap<SurfaceId, SurfaceTransaction>,
    admission_surfaces: BTreeSet<SurfaceId>,
    source: Option<LiveWmProposalSource>,
    policy_settlement: Option<LivePolicySettlementIdentity>,
}

struct LiveAuthorityLayoutObservation {
    new_surfaces: Vec<SurfaceId>,
    withdrawn_surfaces: Vec<SurfaceId>,
    output_reservations_changed: bool,
    admission_group_error: Option<&'static str>,
    admission_group_overflowed: bool,
    client_route_invalid: bool,
}

enum LiveLayoutProgress {
    Blocked,
    DeferredReady,
    Committed(LiveWmCommitResult),
}

/// Whether an escaped Present names the same content as an admission candidate.
///
/// The buffer is part of the comparison because one transaction and surface can
/// carry more than one source, and a backing snapshot must not be read as the
/// client's Present.
fn escaped_key_names_candidate(
    key: sophia_protocol::DmaBufPresentKey,
    candidate: sophia_protocol::SurfaceTransactionKey,
) -> bool {
    candidate.transaction == key.transaction
        && candidate.surface == key.surface
        && matches!(
            candidate.target_buffer,
            sophia_protocol::BufferSource::DmaBuf { handle, .. }
                if handle == key.buffer.raw()
        )
}

const PRE_ADMISSION_GROUP_CAPACITY: usize = 256;

/// How many escaped pre-admission Presents are remembered at once.
///
/// One per escaped Present, not per window, held until production skips it or
/// it stops being eligible. A client outstanding more than this many unskipped
/// pre-map frames at once is not a case worth carrying unbounded state for.
const ESCAPED_PRE_ADMISSION_CAPACITY: usize = 64;

/// A Present that reached production without passing through admission,
/// because its surface was inactive when the batch carrying it was observed.
///
/// A client that presents its first frame before mapping its window produces
/// one of these: `surface_requires_admission` is false while the surface is
/// inactive, so the frame is never quarantined, and a later map does not
/// replay the batch that carried it. Production owns the frame, admission has
/// no claim on it, and nothing waiting on presentation order can release it.
///
/// "Inactive at intake" rather than "never admitted": withdrawal clears
/// admission state, so inactive is a fact about that moment and not a history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EscapedPreAdmissionPresent {
    /// The exact Present, buffer included. A transaction and surface pair is
    /// not identity here -- several sources can share one, and the buffer is
    /// what stops a backing snapshot being mistaken for this frame.
    key: sophia_protocol::DmaBufPresentKey,
    /// Set when the authority confirms the map for this surface, which is the
    /// point the client can be expected to draw again after a skip.
    ready: bool,
}

/// One committed policy answer that placed nothing.
///
/// A blind window manager may decline a surface indefinitely -- a monocle layout
/// places one window however many it is shown. Recording which facts the answer
/// was given against is what separates "policy already said no" from "ask again",
/// and keeps the session from re-asking a settled question every owner-loop turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LiveManageSettlement {
    connection_epoch: u64,
    scene_generation: u64,
}

#[derive(Default)]
struct PersistentLiveLayout {
    layers: BTreeMap<SurfaceId, LayerSnapshot>,
    planning_surfaces: BTreeMap<SurfaceId, sophia_engine::SurfaceLayoutFacts>,
    authority_surface_facts: BTreeMap<SurfaceId, sophia_engine::SurfaceLayoutFacts>,
    admissions: sophia_engine::SurfaceAdmissionTable,
    dma_buf_sizes: BTreeMap<sophia_protocol::BufferHandle, Size>,
    cpu_buffer_sizes: BTreeMap<u64, Size>,
    deferred_dma_buf_releases: BTreeSet<sophia_protocol::BufferHandle>,
    deferred_fence_releases: BTreeSet<sophia_protocol::FenceHandle>,
    layout_epochs: LayoutEpochCoordinator,
    client_routes: XAuthorityClientSurfaceRoutes,
    presentation_roles: BTreeMap<SurfaceId, sophia_protocol::SurfacePresentationRole>,
    presentation_owners: BTreeMap<SurfaceId, SurfaceId>,
    surface_kinds: BTreeMap<SurfaceId, sophia_protocol::LayoutNodeKind>,
    placement_preferences:
        BTreeMap<SurfaceId, sophia_protocol::SurfacePlacementPreference>,
    authority_stack_ranks: BTreeMap<SurfaceId, u32>,
    mapped_surfaces: BTreeSet<SurfaceId>,
    pre_admission_groups: VecDeque<LiveAdmissionAuthorityGroup>,
    escaped_pre_admission: VecDeque<EscapedPreAdmissionPresent>,
    released_admission_groups: VecDeque<LiveAdmissionAuthorityGroup>,
    output_reservations: sophia_engine::SurfaceOutputReservationState,
    unmanaged_surfaces: BTreeSet<SurfaceId>,
    admission_retries: BTreeMap<SurfaceId, u8>,
    /// Surfaces whose admission this coordinator gave up on, awaiting
    /// whoever was waiting on them. Drained rather than read, so an
    /// abandoned launch is told once and not once per owner-loop turn.
    withdrawn_admissions: Vec<SurfaceId>,
    /// Surfaces whose `Manage` request the window manager answered by placing
    /// nothing. Keyed by the facts it answered against, so a fact change retires
    /// the entry rather than any timer.
    manage_settlements: BTreeMap<SurfaceId, LiveManageSettlement>,
    pending: Option<PendingLiveWmLayout>,
    focus_to_apply: Option<(TransactionId, SurfaceId)>,
    retirement_focus:
        BTreeMap<SurfaceId, (sophia_protocol::SurfaceTransactionKey, TransactionId)>,
    bypass_policy_admission: bool,
    /// The Engine, not an external policy client, owns initial placement --
    /// true exactly in the Direct policy-map mode (no external window manager).
    /// A surface then has no policy-assigned output owner, so it must route to
    /// an output by its geometry instead.
    engine_owns_initial_placement: bool,
    stage_new_surfaces_offset: bool,
    center_first_surface_in: Option<Size>,
    constraint_relayout_required: bool,
    awaiting_visual_commits: ResizeVisualCommitTracker,
    committed_policy_presentations:
        BTreeMap<SurfaceId, sophia_protocol::PolicyPresentationState>,
}

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

    fn active_output_reservations(&self) -> Vec<sophia_protocol::SurfaceOutputReservations> {
        self.output_reservations.active_reservations()
    }

    fn merge_unrequested_observation_into_pending(&mut self, observed: LayerSnapshot) {
        let geometry_authority = if self.is_client_positioned(observed.surface) {
            PendingLayoutGeometryAuthority::Observation
        } else {
            PendingLayoutGeometryAuthority::Layout
        };
        let Some(pending) = self.pending.as_mut() else {
            return;
        };
        let _ = merge_unrequested_layout_observation(
            &mut pending.layers,
            &pending.requested_sizes,
            observed,
            geometry_authority,
        );
    }

    fn remove_surfaces(&mut self, removed_surfaces: &[SurfaceId]) {
        if removed_surfaces.is_empty() {
            return;
        }
        self.layers
            .retain(|surface, _| !removed_surfaces.contains(surface));
        self.planning_surfaces
            .retain(|surface, _| !removed_surfaces.contains(surface));
        self.authority_surface_facts
            .retain(|surface, _| !removed_surfaces.contains(surface));
        for surface in removed_surfaces {
            self.layout_epochs.remove(*surface);
            self.awaiting_visual_commits.remove_surface(*surface);
            self.admissions.remove(*surface);
            self.admission_retries.remove(surface);
            self.manage_settlements.remove(surface);
        }
        self.unmanaged_surfaces
            .retain(|surface| !removed_surfaces.contains(surface));
        self.presentation_roles
            .retain(|surface, _| !removed_surfaces.contains(surface));
        self.committed_policy_presentations
            .retain(|surface, _| !removed_surfaces.contains(surface));
        // Preserve a surviving transient's attachment to a removed owner.  A
        // stale owner is deliberately non-visible until the client publishes
        // a new ownership snapshot; dropping the relation here would promote
        // the transient to an unattached, visible client-positioned surface.
        self.presentation_owners
            .retain(|surface, _| !removed_surfaces.contains(surface));
        self.surface_kinds
            .retain(|surface, _| !removed_surfaces.contains(surface));
        self.placement_preferences
            .retain(|surface, _| !removed_surfaces.contains(surface));
        self.authority_stack_ranks
            .retain(|surface, _| !removed_surfaces.contains(surface));
        self.mapped_surfaces
            .retain(|surface| !removed_surfaces.contains(surface));
        self.retirement_focus
            .retain(|surface, _| !removed_surfaces.contains(surface));
        for surface in removed_surfaces {
            self.remove_admission_groups(*surface);
        }
        if self
            .focus_to_apply
            .is_some_and(|(_, surface)| removed_surfaces.contains(&surface))
        {
            self.focus_to_apply = None;
        }
        if let Some(pending) = self.pending.as_mut() {
            pending
                .layers
                .retain(|layer| !removed_surfaces.contains(&layer.surface));
            pending
                .requested_sizes
                .retain(|surface, _| !removed_surfaces.contains(surface));
            if pending
                .focus
                .is_some_and(|surface| removed_surfaces.contains(&surface))
            {
                pending.focus = None;
            }
        }
    }

    /// Forgets every settled policy answer, so the next owner turn asks again.
    fn rearm_manage_settlements(&mut self) {
        if !self.manage_settlements.is_empty() {
            crate::session_println!(
                "sophia_live_wm schema=1 status=manage_rearmed surfaces={} reason=wm_restart",
                self.manage_settlements.len()
            );
            self.manage_settlements.clear();
        }
    }

    /// The surface the owner should offer policy next, if any.
    ///
    /// A settled surface reports none: nothing is in flight and nothing will be
    /// sent until the facts change, so an idle admission pipeline is the honest
    /// answer rather than a pending one.
    fn next_unmanaged_surface(&self) -> Option<SurfaceId> {
        if self.layout_epochs.rollback_surfaces().next().is_some() {
            return None;
        }
        self.unmanaged_surfaces.iter().copied().find(|surface| {
            self.knows_surface(*surface)
                && self.admission_retries.get(surface).copied().unwrap_or(0) <= 1
                && !self.manage_settlements.contains_key(surface)
        })
    }

    fn is_client_positioned(&self, surface: SurfaceId) -> bool {
        self.presentation_roles.get(&surface)
            == Some(&sophia_protocol::SurfacePresentationRole::ClientPositioned)
    }

    /// Whether this surface reaches an output by its geometry rather than by a
    /// policy-assigned owner.
    ///
    /// A client-positioned surface always does: it carries its own coordinates
    /// and no policy places it. In a session with no external window manager
    /// (the Direct policy-map mode) nothing assigns any surface an output owner
    /// and the Engine owns placement, so every surface routes by geometry there
    /// too. Without this a policy-managed window in a no-WM session matches
    /// neither routing arm and reaches no output at all.
    fn surface_is_geometry_routed(&self, surface: SurfaceId) -> bool {
        self.is_client_positioned(surface) || self.engine_owns_initial_placement
    }

    fn top_client_positioned_surface(&self) -> Option<SurfaceId> {
        self.layers
            .values()
            .filter(|layer| {
                self.is_client_positioned(layer.surface)
                    && self.mapped_surfaces.contains(&layer.surface)
            })
            .max_by_key(|layer| (layer.stack_rank, layer.surface))
            .map(|layer| layer.surface)
    }

    fn release_recovery_extent(&mut self, surface: SurfaceId, reason: &'static str) -> bool {
        if !self.layout_epochs.clear_recovery_extent(surface) {
            return false;
        }
        self.constraint_relayout_required = true;
        crate::session_println!(
            "sophia_live_resize_epoch schema=2 status=recovery_extent_cleared surface={} reason={reason}",
            surface.index(),
        );
        true
    }

    fn complete_visual_commit(
        &mut self,
        visual_candidate: sophia_protocol::SurfaceTransactionKey,
        size: Size,
    ) -> bool {
        if let Some(candidate) = self
            .awaiting_visual_commits
            .complete(visual_candidate, size)
        {
            let surface = candidate.candidate.surface;
            self.layout_epochs
                .record_committed(surface, candidate.layout_size);
            crate::session_println!(
                "sophia_live_resize_epoch schema=3 status=visual_committed transaction={} surface={} width={} height={}",
                candidate.candidate.transaction.raw(),
                candidate.candidate.surface.index(),
                candidate.layout_size.width,
                candidate.layout_size.height,
            );
            return true;
        }

        // Unarmed Presents may update retained content, but they cannot mutate
        // logical layout state. Every recovery successor is selected above and
        // must match its exact native-retirement identity.
        false
    }

    fn take_withdrawn_admissions(&mut self) -> Vec<SurfaceId> {
        std::mem::take(&mut self.withdrawn_admissions)
    }

    fn constraint_relayout_required(&self) -> bool {
        self.constraint_relayout_required
    }

    fn acknowledge_constraint_relayout(&mut self) {
        self.constraint_relayout_required = false;
    }

    fn recovery_extent_count(&self) -> usize {
        self.layout_epochs.recovery_extent_count()
    }

    fn standing_target_count(&self) -> usize {
        self.layout_epochs.pending_target_count()
    }

    fn client_positioned_mapped(&self, surface: SurfaceId) -> bool {
        self.is_client_positioned(surface) && self.mapped_surfaces.contains(&surface)
    }

    fn presentation_owner(&self, surface: SurfaceId) -> Option<SurfaceId> {
        self.presentation_owners.get(&surface).copied()
    }

    fn stage(
        &mut self,
        mut proposal: LiveWmProposal,
        session_controls: &mut SessionControlQueue,
    ) -> Result<Option<LiveWmCommitResult>, Box<dyn std::error::Error>> {
        if self.pending.is_some() {
            crate::session_println!(
                "sophia_live_wm schema=1 status=proposal_busy transaction={} preserved_layout=true",
                proposal.transaction.raw()
            );
            return Ok(None);
        }
        // A standing launch target is an obligation, not immutable policy.
        // A new work area can replace it before the recovery frame retires.
        // Keep it only when the WM is echoing the temporary recovery constraint.
        for (&surface, &target) in &proposal.requested_sizes {
            if self.layout_epochs.pending_target(surface).is_some()
                && self.layout_epochs.recovery_extent(surface) != Some(target)
            {
                self.layout_epochs.set_pending_target(surface, target);
            }
        }
        for layer in proposal
            .layers
            .iter()
            .filter(|layer| self.surface_awaits_visual_candidate(layer.surface))
        {
            proposal.requested_sizes.entry(layer.surface).or_insert(Size {
                width: layer.geometry.width,
                height: layer.geometry.height,
            });
        }
        // Drive a standing target left by an aborted launch epoch through this
        // resize transaction rather than out of band. A first-launch client is
        // fenced and admitted at whatever extent it first mapped; its blind-WM
        // target is retained as an obligation and injected here so the same
        // ConfigureSurface, exact-size epoch gate, and record_committed run as
        // one transaction. That keeps the client, the Engine committed size,
        // and the WM layer in agreement, so the resized frame is accepted (not
        // rejected as a stale surface) and a denied reactive client configure
        // is answered with the target rather than the welded launch size.
        let standing_targets = self
            .layout_epochs
            .pending_target_surfaces()
            .filter(|(surface, target)| {
                self.layout_epochs.recovery_extent(*surface).is_none()
                    && self.layout_epochs.committed_size(*surface) != Some(*target)
            })
            .collect::<Vec<_>>();
        for (surface, target) in standing_targets {
            if let Some(layer) = proposal
                .layers
                .iter_mut()
                .find(|layer| layer.surface == surface)
            {
                layer.geometry.width = target.width;
                layer.geometry.height = target.height;
                proposal.requested_sizes.insert(surface, target);
                let attempts = self.layout_epochs.note_standing_redrive(surface);
                // An obligation re-driven this often is not converging, and the
                // reconfigures are identical, so nothing else in the run would
                // say so. Reported once at the boundary rather than every cycle:
                // the loop is cheap, and a line per frame would bury it.
                if attempts == STANDING_TARGET_REDRIVE_REPORT_THRESHOLD {
                    let committed = self.layout_epochs.committed_size(surface);
                    tracing::warn!(
                        "sophia_live_wm_standing_target schema=1 status=not_converging surface={} attempts={} target={}x{} committed={} action=continue",
                        surface.index(),
                        attempts,
                        target.width,
                        target.height,
                        committed.map_or_else(
                            || "none".to_owned(),
                            |size| format!("{}x{}", size.width, size.height)
                        ),
                    );
                }
            }
        }
        for (surface, size) in &proposal.requested_sizes {
            if !self.layout_epochs.request_allowed(*surface, *size)
                && let Some(committed) = self.layout_epochs.committed_size(*surface)
                && let Some(layer) = proposal
                    .layers
                    .iter_mut()
                    .find(|layer| layer.surface == *surface)
            {
                layer.geometry.width = committed.width;
                layer.geometry.height = committed.height;
            }
        }
        proposal.requested_sizes.retain(|surface, size| {
            let installed_visual_target = self
                .layers
                .get(surface)
                .is_some_and(|layer| {
                    layer.geometry.width == size.width && layer.geometry.height == size.height
                })
                && self
                    .awaiting_visual_commits
                    .surface_layout_awaiting(*surface, *size);
            self.surface_awaits_visual_candidate(*surface)
                || (!installed_visual_target
                    && self.layout_epochs.request_allowed(*surface, *size)
                    && self.layout_epochs.committed_size(*surface) != Some(*size))
        });
        let mut staged_transactions = BTreeMap::new();
        for (surface, size) in &proposal.requested_sizes {
            let Some(transaction) = self.selected_pre_admission_transaction(*surface, *size)
            else {
                continue;
            };
            staged_transactions.insert(*surface, transaction.clone());
            if let Some(layer) = proposal
                .layers
                .iter_mut()
                .find(|layer| layer.surface == *surface)
            {
                layer.source = transaction.target_buffer();
                layer.damage = transaction.damage.clone();
                layer.generation = transaction.previous_committed_generation.saturating_add(1);
            }
        }
        // X position feedback is required even when committed pixels remain
        // reusable. Keep it separate from the resize-only readiness map.
        let changed_geometries = proposal
            .layers
            .iter()
            .filter_map(|layer| {
                let moved = self
                    .layers
                    .get(&layer.surface)
                    .is_none_or(|current| current.geometry != layer.geometry);
                // A recovery reseed can retain Engine geometry while late
                // pixels from the aborted target leave the client at another
                // size. Its resize obligation must reassert the same rectangle.
                (moved
                    || proposal.requested_sizes.contains_key(&layer.surface)
                    || self.surface_awaits_visual_candidate(layer.surface))
                    .then_some((layer.surface, layer.geometry))
            })
            .collect::<BTreeMap<_, _>>();
        proposal.moved_surfaces = proposal
            .layers
            .iter()
            .filter(|layer| {
                self.layers
                    .get(&layer.surface)
                    .is_none_or(|current| current.geometry != layer.geometry)
            })
            .count();
        let mut admission_surfaces = BTreeSet::new();
        for (surface, geometry) in changed_geometries {
            let stage =
                self.stage_surface_control(proposal.transaction, surface, geometry)?;
            if stage.admission_owned {
                admission_surfaces.insert(surface);
            }
            let Some(command) = stage.command else {
                continue;
            };
            proposal.configure_deliveries = proposal.configure_deliveries.saturating_add(1);
            let client = self
                .client_routes
                .client_for_surface(surface)
                .ok_or("live WM configure has no X11 client route for its surface")?;
            session_controls
                .enqueue(
                    XAuthorityClientControlCommand { client, command },
                    Instant::now(),
                )
                .map_err(|error| {
                    format!("failed to queue WM configure control: {error:?}")
                })?;
        }
        let presentation_settlements = proposal
            .presentation_states
            .iter()
            .filter_map(|(surface, state)| {
                (self.committed_policy_presentations.get(surface) != Some(state))
                    .then_some(*surface)
            })
            .collect::<BTreeSet<_>>();
        for surface in &presentation_settlements {
            let state = proposal.presentation_states[surface];
            let client = self
                .client_routes
                .client_for_surface(*surface)
                .ok_or("live WM presentation state has no X11 client route")?;
            session_controls
                .enqueue(
                    XAuthorityClientControlCommand {
                        client,
                        command: XAuthorityControlCommand::SetPresentationState {
                            transaction: proposal.transaction,
                            surface: *surface,
                            state,
                        },
                    },
                    Instant::now(),
                )
                .map_err(|error| {
                    format!("failed to queue WM presentation-state control: {error:?}")
                })?;
        }
        // A surface that has never presented cannot answer this epoch's gate.
        // The gate admits a surface on an exact-size frame, and a surface with
        // no safe observation has no pixels to resize: it can only pass by
        // drawing its first frame at precisely the requested extent, inside a
        // deadline a blind WM sizes for settled clients. A cold browser needs
        // seconds; the deadline is a few hundred milliseconds. Holding one in
        // the gate does not make it answer faster — it fails the epoch and
        // takes every sibling's resize down with it.
        //
        // Defer it to a standing obligation instead. Its ConfigureSurface has
        // already gone out above, so the client still learns its target; what
        // changes here is only whether the epoch waits on it. The obligation is
        // driven by the standing-target injection at the head of this function
        // once the surface has pixels to resize.
        let deferred_surfaces = proposal
            .requested_sizes
            .keys()
            .copied()
            .filter(|surface| self.layout_epochs.safe_size(*surface).is_none())
            .collect::<Vec<_>>();
        for surface in &deferred_surfaces {
            if let Some(target) = proposal.requested_sizes.remove(surface) {
                self.layout_epochs.set_pending_target(*surface, target);
            }
        }
        let ready = proposal
            .requested_sizes
            .iter()
            .all(|(surface, size)| {
                self.layout_epochs.committed_size(*surface) == Some(*size)
                    && !admission_surfaces.contains(surface)
            })
            && presentation_settlements.is_empty();
        if ready {
            return Ok(Some(self.commit_proposal(proposal)));
        }
        let timeout_msec = proposal.timeout.as_millis();
        self.pending = Some(PendingLiveWmLayout {
            transaction: proposal.transaction,
            layers: proposal.layers,
            requested_sizes: proposal.requested_sizes,
            presentation_states: proposal.presentation_states,
            presentation_settlements,
            configure_deliveries: proposal.configure_deliveries,
            focus: proposal.focus,
            deadline: Instant::now() + proposal.timeout,
            update: proposal.update,
            moved_surfaces: proposal.moved_surfaces,
            staged_transactions,
            admission_surfaces,
            source: proposal.source,
            policy_settlement: proposal.policy_settlement,
        });
        // `surfaces` counts what the epoch actually waits on, so `deferred`
        // must name what was held back: a reader cannot otherwise tell a
        // one-surface epoch from a two-surface one that excused a sibling.
        // The deadline is reported with them because it is the blind WM's
        // choice, and a gate that expires early is indistinguishable from a
        // client that never answered unless both are on the record.
        crate::session_println!(
            "sophia_live_resize_epoch schema=2 status=held transaction={} surfaces={} deferred={} timeout_msec={}",
            self.pending
                .as_ref()
                .expect("pending layout was just installed")
                .transaction
                .raw(),
            self.pending
                .as_ref()
                .expect("pending layout was just installed")
                .requested_sizes
                .len(),
            deferred_surfaces.len(),
            timeout_msec,
        );
        Ok(None)
    }

    fn resolve_pending(&mut self) -> Option<LiveWmCommitResult> {
        if !self.pending_is_ready() {
            return None;
        }
        let pending = self.pending.take().expect("checked above");
        Some(self.commit_pending(pending))
    }

    fn pending_is_ready(&self) -> bool {
        let Some(pending) = self.pending.as_ref() else {
            return false;
        };
        pending.presentation_settlements.is_empty()
            && pending.requested_sizes.iter().all(|(surface, size)| {
            let staged_matches = pending
                .staged_transactions
                .get(surface)
                .is_some_and(|transaction| {
                    live_transaction_observed_size(
                        transaction,
                        &self.dma_buf_sizes,
                        &self.cpu_buffer_sizes,
                    )
                        == *size
                });
            let retained_matches = self.layout_epochs.committed_size(*surface) == Some(*size)
                && pending
                    .layers
                    .iter()
                    .find(|layer| layer.surface == *surface)
                    .is_some_and(|layer| layer.source != BufferSource::None);
            let admission_ready = !pending.admission_surfaces.contains(surface)
                || matches!(
                    self.admissions.state(*surface),
                    sophia_engine::SurfacePresentationAdmissionState::AwaitingPixels { .. }
                        | sophia_engine::SurfacePresentationAdmissionState::Managed
                );
            let pixels_ready = if pending.admission_surfaces.contains(surface) {
                staged_matches
            } else {
                staged_matches || retained_matches
            };
            pixels_ready && admission_ready
            })
    }

    fn acknowledge_presentation_control(
        &mut self,
        transaction: TransactionId,
        surface: SurfaceId,
    ) -> bool {
        let Some(pending) = self.pending.as_mut() else {
            return false;
        };
        pending.transaction == transaction && pending.presentation_settlements.remove(&surface)
    }

    fn force_pending_timeout(&mut self) -> bool {
        let Some(pending) = self.pending.as_mut() else {
            return false;
        };
        pending.deadline = Instant::now();
        true
    }

    fn expire_pending(
        &mut self,
        session_controls: &mut SessionControlQueue,
    ) -> Result<Option<LiveWmCommitResult>, Box<dyn std::error::Error>> {
        if self
            .pending
            .as_ref().is_none_or(|pending| Instant::now() < pending.deadline)
        {
            return Ok(None);
        }
        let pending = self.pending.take().expect("checked above");
        let admission_surfaces = pending
            .layers
            .iter()
            .map(|layer| layer.surface)
            .filter(|surface| self.unmanaged_surfaces.contains(surface))
            .collect::<Vec<_>>();
        let terminal_admissions = admission_surfaces
            .iter()
            .copied()
            .filter(|surface| self.admission_retries.get(surface).copied().unwrap_or(0) >= 1)
            .collect::<BTreeSet<_>>();
        for surface in admission_surfaces
            .iter()
            .copied()
            .filter(|surface| !terminal_admissions.contains(surface))
        {
            self.synchronize_admission_extent(surface);
        }
        let recoverable_admissions = admission_surfaces
            .iter()
            .copied()
            .filter(|surface| {
                !terminal_admissions.contains(surface)
                    && self.surface_awaits_visual_candidate(*surface)
                    && self
                        .layout_epochs
                        .recovery_extent(*surface)
                        .is_some_and(|extent| {
                            self.selected_pre_admission_transaction(*surface, extent)
                                .is_some()
                        })
            })
            .collect::<Vec<_>>();
        // A first-launch admission with retained pixels is fenced through a
        // fixed recovery extent, not rolled back. Candidate-less geometry and
        // pixel-silent admission are expected states: they keep the standing
        // target and bounded retry but cannot claim an extent whose exact
        // transaction is unavailable at the quarantine boundary.
        // A recovery request needs a committed extent to configure back to. A
        // surface the coordinator no longer knows — one withdrawn while its
        // resize was outstanding — has none, and naming it fails the whole
        // recovery for every surface that could still be recovered. The fixed
        // set above already applies this test; requests must apply it too.
        let recoverable_requests = pending
            .requested_sizes
            .iter()
            .filter(|(surface, _)| {
                !terminal_admissions.contains(surface)
                    && !admission_surfaces.contains(surface)
                    && self.layout_epochs.safe_size(**surface).is_some()
            })
            .map(|(surface, size)| (*surface, *size))
            .collect::<Vec<_>>();
        let rollback = self
            .layout_epochs
            .begin_recovery(recoverable_requests, recoverable_admissions)?;
        // Retain each fenced admission surface's blind-WM target as a standing
        // obligation. Once its temporary recovery extent clears it is driven to
        // that size rather than staying welded to the extent it first mapped at.
        for surface in &admission_surfaces {
            if terminal_admissions.contains(surface) {
                continue;
            }
            if let Some(target) = pending.requested_sizes.get(surface) {
                self.layout_epochs.set_pending_target(*surface, *target);
            }
        }
        let rollback_transaction = rollback
            .first()
            .map(|request| request.transaction)
            .unwrap_or(pending.transaction);
        let mut rollback_configures = rollback.len();
        let mut rollback_withdrawn = 0usize;
        for request in rollback {
            let surface = request.surface;
            let size = request.size;
            // A surface can be withdrawn between the proposal and the rollback
            // that undoes it -- a window closing while a layout is in flight is
            // ordinary, not exceptional. There is nothing to restore it to and
            // nobody left to tell, so it is skipped and counted.
            //
            // This used to end the session. A workspace switch with a dock
            // present was enough to reach it, and killing a desktop because one
            // window stopped existing is never the right answer.
            let Some(geometry) = self.layers.get(&surface).map(|layer| Rect {
                width: size.width,
                height: size.height,
                ..layer.geometry
            }) else {
                rollback_configures = rollback_configures.saturating_sub(1);
                rollback_withdrawn += 1;
                continue;
            };
            let Some(client) = self.client_routes.client_for_surface(surface) else {
                rollback_configures = rollback_configures.saturating_sub(1);
                rollback_withdrawn += 1;
                continue;
            };
            session_controls.enqueue(XAuthorityClientControlCommand {
                client,
                command: XAuthorityControlCommand::ConfigureSurface {
                    transaction: rollback_transaction,
                    surface,
                    geometry,
                },
            }, Instant::now()).map_err(|error| {
                format!("failed to queue WM rollback control: {error:?}")
            })?;
        }
        for (surface, desired) in &pending.presentation_states {
            let previous = self
                .committed_policy_presentations
                .get(surface)
                .copied()
                .unwrap_or_default();
            if previous == *desired || terminal_admissions.contains(surface) {
                continue;
            }
            // Withdrawn during the cycle, as above.
            let Some(client) = self.client_routes.client_for_surface(*surface) else {
                rollback_withdrawn += 1;
                continue;
            };
            session_controls
                .enqueue(
                    XAuthorityClientControlCommand {
                        client,
                        command: XAuthorityControlCommand::RestorePresentationState {
                            transaction: rollback_transaction,
                            surface: *surface,
                            state: previous,
                        },
                    },
                    Instant::now(),
                )
                .map_err(|error| {
                    format!("failed to queue WM presentation rollback: {error:?}")
                })?;
        }
        for surface in &terminal_admissions {
            // The most likely of the three to be gone: these are surfaces
            // already on their way out, so losing the route before the
            // withdrawal is delivered is the expected ending, not a fault.
            let Some(client) = self.client_routes.client_for_surface(*surface) else {
                rollback_withdrawn += 1;
                continue;
            };
            session_controls
                .enqueue(
                    XAuthorityClientControlCommand {
                        client,
                        command: XAuthorityControlCommand::WithdrawSurface {
                            transaction: pending.transaction,
                            surface: *surface,
                        },
                    },
                    Instant::now(),
                )
                .map_err(|error| {
                    format!("failed to queue terminal WM admission withdrawal: {error:?}")
                })?;
            // Giving up on a surface is a decision, and it was silent: the
            // coordinator erased an admission with nothing on the record, so a
            // launch that owned it went on waiting for a surface that no longer
            // existed. Say so, and keep the surface for whoever is waiting.
            crate::session_println!(
                "sophia_live_surface_admission schema=1 status=withdrawn transaction={} surface={} reason=retries_exhausted",
                pending.transaction.raw(),
                surface.index(),
            );
            self.withdrawn_admissions.push(*surface);
            self.admissions.remove(*surface);
            self.planning_surfaces.remove(surface);
            self.authority_surface_facts.remove(surface);
            self.unmanaged_surfaces.remove(surface);
            self.admission_retries.remove(surface);
            self.manage_settlements.remove(surface);
            self.layout_epochs.remove(*surface);
            self.remove_admission_groups(*surface);
        }
        let resize_state = pending
            .requested_sizes
            .iter()
            .map(|(surface, expected)| {
                let observed = self
                    .layout_epochs
                    .committed_size(*surface)
                    .unwrap_or(Size {
                    width: 0,
                    height: 0,
                });
                format!(
                    "{}x{}:{}x{}",
                    expected.width, expected.height, observed.width, observed.height
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        crate::session_println!(
            "sophia_live_wm schema=1 status=layout_timeout transaction={} preserved_layout=true rollback_transaction={} rollback_configures={} rollback_withdrawn={rollback_withdrawn} resize_state={}",
            pending.transaction.raw(),
            rollback_transaction.raw(),
            rollback_configures,
            resize_state,
        );
        // A retry is spent by a surface that was asked and did not answer. A
        // surface deferred out of the gate was never asked, so an expiry it
        // took no part in must not count against it — two of those retire an
        // admission, and a launching client would be withdrawn for a deadline
        // that was never its to meet. `stage` and this bound read the same
        // fact so they cannot come apart.
        for surface in admission_surfaces
            .into_iter()
            .filter(|surface| !terminal_admissions.contains(surface))
            .filter(|surface| pending.requested_sizes.contains_key(surface))
        {
            let attempts = self.admission_retries.entry(surface).or_default();
            *attempts = attempts.saturating_add(1);
            self.manage_settlements.remove(&surface);
        }
        crate::session_println!(
            "sophia_live_resize_epoch schema=1 status=aborted transaction={} rejected_surfaces={}",
            pending.transaction.raw(),
            pending.requested_sizes.len(),
        );
        Ok(Some(LiveWmCommitResult {
            update: WmTransactionUpdate {
                commit: TransactionCommit {
                    transaction: pending.transaction,
                    outcome: TransactionOutcome::TimedOut,
                    applied_surfaces: Vec::new(),
                },
            },
            // The external WM has already applied the request that produced
            // this proposal. Retain its source so the owner can restart and
            // reseed that speculative peer after rejecting the layout.
            source: pending.source,
            policy_settlement: pending.policy_settlement,
        }))
    }

}
include!("layout/admission.rs");
include!("layout/commit.rs");
