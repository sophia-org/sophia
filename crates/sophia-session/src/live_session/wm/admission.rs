struct LiveSurfaceControlStage {
    admission_owned: bool,
    command: Option<XAuthorityControlCommand>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LiveAdmissionAuthorityGroup {
    transaction: TransactionId,
    transactions: Vec<SurfaceTransaction>,
    cpu_buffer_updates: Vec<sophia_x_authority::XAuthorityCpuBufferUpdate>,
    present_submissions: Vec<sophia_x_authority::XAuthorityPresentSubmission>,
    software_present_submissions: Vec<sophia_x_authority::XAuthoritySoftwarePresentSubmission>,
    superseded: bool,
}

impl LiveAdmissionAuthorityGroup {
    fn validate(&self) -> Result<(), &'static str> {
        if !self.transaction.is_valid() {
            return Err("pre-admission authority group has an invalid transaction");
        }
        if self
            .transactions
            .iter()
            .any(|transaction| transaction.transaction != self.transaction)
        {
            return Err("pre-admission authority group contains a mismatched transaction");
        }
        for update in &self.cpu_buffer_updates {
            let matches = self
                .transactions
                .iter()
                .filter(|transaction| {
                    transaction.content.variants().iter().any(|variant| {
                        matches!(
                            variant.source,
                            BufferSource::CpuBuffer { handle } if handle == update.handle()
                        )
                    })
                })
                .count();
            if matches != 1 {
                return Err("pre-admission CPU update has no exact surface transaction");
            }
        }
        if self
            .present_submissions
            .iter()
            .any(|submission| submission.transaction != self.transaction)
        {
            return Err("pre-admission authority group contains a mismatched Present");
        }
        if self
            .software_present_submissions
            .iter()
            .any(|submission| submission.transaction != self.transaction)
        {
            return Err("pre-admission authority group contains a mismatched software Present");
        }
        let present_keys = self
            .present_submissions
            .iter()
            .map(|submission| sophia_protocol::DmaBufPresentKey {
                transaction: submission.transaction,
                surface: submission.surface,
                buffer: submission.buffer,
            })
            .collect::<Vec<_>>();
        if !sophia_protocol::dma_buf_present_pairs_are_exact(&self.transactions, &present_keys) {
            return Err("pre-admission DMA-BUF transactions and Presents are not exact pairs");
        }
        for (index, submission) in self.software_present_submissions.iter().enumerate() {
            if self.software_present_submissions[..index]
                .iter()
                .any(|prior| prior.surface == submission.surface)
            {
                return Err("pre-admission authority group contains a duplicate software Present");
            }
            let matches = self
                .transactions
                .iter()
                .filter(|transaction| {
                    transaction.transaction == submission.transaction
                        && transaction.surface == submission.surface
                        && matches!(transaction.target_buffer(), BufferSource::CpuBuffer { .. })
                })
                .count();
            if matches != 1 {
                return Err(
                    "pre-admission CPU transactions and software Presents are not exact pairs",
                );
            }
        }
        Ok(())
    }

    fn contains_surface(&self, surface: SurfaceId) -> bool {
        self.transactions
            .iter()
            .any(|transaction| transaction.surface == surface)
            || self
                .present_submissions
                .iter()
                .any(|submission| submission.surface == surface)
            || self
                .software_present_submissions
                .iter()
                .any(|submission| submission.surface == surface)
    }

    fn contains_any_surface(&self, surfaces: &BTreeSet<SurfaceId>) -> bool {
        self.transactions
            .iter()
            .any(|transaction| surfaces.contains(&transaction.surface))
            || self
                .present_submissions
                .iter()
                .any(|submission| surfaces.contains(&submission.surface))
            || self
                .software_present_submissions
                .iter()
                .any(|submission| surfaces.contains(&submission.surface))
    }

    fn reproject_surface(&mut self, surface: SurfaceId, geometry: Rect) {
        for transaction in &mut self.transactions {
            if transaction.surface == surface {
                transaction.target_geometry = geometry;
            }
        }
    }

    fn dma_bufs(&self) -> impl Iterator<Item = sophia_protocol::BufferHandle> + '_ {
        self.present_submissions
            .iter()
            .map(|submission| submission.buffer)
    }

    fn fences(&self) -> impl Iterator<Item = sophia_protocol::FenceHandle> + '_ {
        self.present_submissions
            .iter()
            .flat_map(|submission| [submission.acquire_fence, submission.idle_fence])
            .chain(
                self.software_present_submissions
                    .iter()
                    .flat_map(|submission| [submission.acquire_fence, submission.idle_fence]),
            )
            .flatten()
    }
}

impl PersistentLiveLayout {
    fn surface_awaits_visual_candidate(&self, surface: SurfaceId) -> bool {
        matches!(
            self.admissions.state(surface),
            sophia_engine::SurfacePresentationAdmissionState::PolicyPending
                | sophia_engine::SurfacePresentationAdmissionState::ControlPending { .. }
                | sophia_engine::SurfacePresentationAdmissionState::AwaitingPixels { .. }
        )
    }

    fn stage_surface_control(
        &mut self,
        transaction: TransactionId,
        surface: SurfaceId,
        geometry: Rect,
    ) -> Result<LiveSurfaceControlStage, Box<dyn std::error::Error>> {
        use sophia_engine::SurfacePresentationAdmissionState::{
            AwaitingPixels, AwaitingRetirement, ControlPending, Inactive, Managed, PolicyPending,
        };

        let stage = match self.admissions.state(surface) {
            PolicyPending => {
                if !self
                    .admissions
                    .begin_control(surface, transaction, geometry)
                {
                    return Err("Engine rejected live WM admission transition".into());
                }
                LiveSurfaceControlStage {
                    admission_owned: true,
                    command: Some(XAuthorityControlCommand::AdmitSurface {
                        transaction,
                        surface,
                        geometry,
                    }),
                }
            }
            ControlPending { .. } => LiveSurfaceControlStage {
                admission_owned: true,
                command: None,
            },
            AwaitingPixels { .. } | AwaitingRetirement { .. } => LiveSurfaceControlStage {
                admission_owned: true,
                command: Some(XAuthorityControlCommand::ConfigureSurface {
                    transaction,
                    surface,
                    geometry,
                }),
            },
            Inactive | Managed => LiveSurfaceControlStage {
                admission_owned: false,
                command: Some(XAuthorityControlCommand::ConfigureSurface {
                    transaction,
                    surface,
                    geometry,
                }),
            },
        };
        Ok(stage)
    }

    /// Whether this surface's Present escaped admission because the surface
    /// was inactive when the batch arrived.
    ///
    /// Affirmative rather than inferred. `surface_requires_admission` is also
    /// false for a surface this session does not manage, for one already
    /// managed, and when policy admission is bypassed entirely; none of those
    /// escaped anything and none may take the skip. Only an inactive
    /// policy-managed surface did.
    fn present_escaped_admission(&self, surface: SurfaceId) -> bool {
        !self.bypass_policy_admission
            && self.presentation_roles.get(&surface)
                == Some(&sophia_protocol::SurfacePresentationRole::PolicyManaged)
            && matches!(
                self.admissions.state(surface),
                sophia_engine::SurfacePresentationAdmissionState::Inactive
            )
    }

    fn record_escaped_pre_admission(&mut self, batch: &XAuthorityObservedTransactionBatch) {
        for submission in &batch.present_submissions {
            if self.surface_requires_admission(submission.surface)
                || !self.present_escaped_admission(submission.surface)
            {
                continue;
            }
            let key = sophia_protocol::DmaBufPresentKey {
                transaction: submission.transaction,
                surface: submission.surface,
                buffer: submission.buffer,
            };
            if self
                .escaped_pre_admission
                .iter()
                .any(|escaped| escaped.key == key)
            {
                continue;
            }
            if self.escaped_pre_admission.len() >= ESCAPED_PRE_ADMISSION_CAPACITY {
                self.escaped_pre_admission.pop_front();
            }
            self.escaped_pre_admission
                .push_back(EscapedPreAdmissionPresent { key, ready: false });
        }
    }

    /// Whether an escaped Present may still be skipped.
    ///
    /// Revalidated at the moment of acting, never inferred from the state that
    /// recorded it. Between escaping and being skipped the surface can change
    /// role, be admitted outright, or have this very frame selected as its
    /// visual candidate, and each of those makes the frame somebody else's to
    /// settle.
    fn escaped_present_eligible(&self, key: sophia_protocol::DmaBufPresentKey) -> bool {
        if self.bypass_policy_admission
            || self.presentation_roles.get(&key.surface)
                != Some(&sophia_protocol::SurfacePresentationRole::PolicyManaged)
        {
            // A surface the client now positions for itself, or one admission
            // no longer governs, is not ours to skip.
            return false;
        }
        // A candidate already staged into the pending layout belongs to
        // admission, which arms retirement from exactly this key when the
        // layout commits. Both tests are needed: membership says admission owns
        // the surface this cycle, and the staged key says it owns *this* frame
        // rather than some other buffer the surface also presented.
        if self.pending.as_ref().is_some_and(|pending| {
            pending.admission_surfaces.contains(&key.surface)
                && pending
                    .staged_transactions
                    .get(&key.surface)
                    .is_some_and(|transaction| {
                        escaped_key_names_candidate(key, transaction.key())
                    })
        }) {
            return false;
        }
        match self.admissions.state(key.surface) {
            // Fully admitted: there is nothing left to rescue, and the record
            // is only occupying space that an actionable one could use.
            sophia_engine::SurfacePresentationAdmissionState::Managed => false,
            // Admission selected this exact frame after it escaped. Observed
            // pixels are not a claim, but a selected visual candidate is, and
            // settling it here would take a retirement that is not ours.
            sophia_engine::SurfacePresentationAdmissionState::AwaitingRetirement {
                visual_candidate,
                ..
            } => !escaped_key_names_candidate(key, visual_candidate),
            _ => true,
        }
    }

    /// Escaped Presents the authority has confirmed a map for, which production
    /// may now skip.
    ///
    /// Reported rather than taken: production owns the candidate and may not
    /// have queued it yet, and a request consumed on a queue miss would strand
    /// the client exactly as before.
    ///
    /// Records that can never be acted on are dropped here rather than left to
    /// age out, because eviction is by arrival and a stale record would push out
    /// an actionable one -- silently returning some other window to the slow
    /// path this exists to avoid.
    ///
    /// This does not catch every settled record. A frame ordinarily rejected
    /// while its surface is still pending admission stays eligible by these
    /// tests and remains a permanent queue miss until the surface is cleaned
    /// up. No ownership rule is broken by that, but it is not free either: a
    /// stale record occupies a slot and can evict an actionable one, returning
    /// some other window to the slow path. Closing it needs a positive
    /// terminal-result signal this does not yet have.
    pub(crate) fn skippable_escaped_presents(
        &mut self,
    ) -> Vec<sophia_protocol::DmaBufPresentKey> {
        let ineligible = self
            .escaped_pre_admission
            .iter()
            .filter(|escaped| !self.escaped_present_eligible(escaped.key))
            .map(|escaped| escaped.key)
            .collect::<Vec<_>>();
        self.escaped_pre_admission
            .retain(|escaped| !ineligible.contains(&escaped.key));
        self.escaped_pre_admission
            .iter()
            .filter(|escaped| escaped.ready)
            .map(|escaped| escaped.key)
            .collect()
    }

    /// Forget one escaped Present, once production has actually skipped it or
    /// the surface it belonged to is gone.
    pub(crate) fn consume_escaped_present(&mut self, key: sophia_protocol::DmaBufPresentKey) {
        self.escaped_pre_admission
            .retain(|escaped| escaped.key != key);
    }

    fn forget_escaped_presents(&mut self, surface: SurfaceId) {
        self.escaped_pre_admission
            .retain(|escaped| escaped.key.surface != surface);
    }

    fn observe_pre_admission_groups(
        &mut self,
        batch: &XAuthorityObservedTransactionBatch,
    ) -> Result<bool, &'static str> {
        self.record_escaped_pre_admission(batch);
        let transactions = batch
            .transactions
            .iter()
            .filter(|transaction| self.surface_requires_admission(transaction.surface))
            .cloned()
            .collect::<Vec<_>>();
        let present_submissions = batch
            .present_submissions
            .iter()
            .filter(|submission| self.surface_requires_admission(submission.surface))
            .copied()
            .collect::<Vec<_>>();
        let software_present_submissions = batch
            .software_present_submissions
            .iter()
            .filter(|submission| self.surface_requires_admission(submission.surface))
            .copied()
            .collect::<Vec<_>>();
        let cpu_handles = transactions
            .iter()
            .flat_map(|transaction| transaction.content.variants())
            .filter_map(|variant| match variant.source {
                BufferSource::CpuBuffer { handle } => Some(handle),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let cpu_buffer_updates = batch
            .cpu_buffer_updates
            .iter()
            .filter(|update| cpu_handles.contains(&update.handle()))
            .cloned()
            .collect::<Vec<_>>();
        if transactions.is_empty()
            && present_submissions.is_empty()
            && software_present_submissions.is_empty()
        {
            return Ok(false);
        }
        let group = LiveAdmissionAuthorityGroup {
            transaction: batch.transaction,
            transactions,
            cpu_buffer_updates,
            present_submissions,
            software_present_submissions,
            superseded: false,
        };
        group.validate()?;
        if self.pre_admission_groups.len() >= PRE_ADMISSION_GROUP_CAPACITY {
            return Ok(true);
        }
        self.pre_admission_groups.push_back(group);
        Ok(false)
    }

    fn release_admission_groups(
        &mut self,
        selected_transactions: &BTreeMap<SurfaceId, TransactionId>,
    ) {
        let selected_positions = selected_transactions
            .iter()
            .filter_map(|(surface, transaction)| {
                self.pre_admission_groups
                    .iter()
                    .position(|group| group.transaction == *transaction)
                    .map(|position| (*surface, position))
            })
            .collect::<BTreeMap<_, _>>();
        let mut retained = VecDeque::with_capacity(self.pre_admission_groups.len());
        let mut committed_generations = BTreeMap::<SurfaceId, u64>::new();
        for position in 0..self.pre_admission_groups.len() {
            let mut group = self
                .pre_admission_groups
                .pop_front()
                .expect("pre-admission group position exists");
            let touched = group
                .transactions
                .iter()
                .map(|transaction| transaction.surface)
                .chain(
                    group
                        .present_submissions
                        .iter()
                        .map(|submission| submission.surface),
                )
                .chain(
                    group
                        .software_present_submissions
                        .iter()
                        .map(|submission| submission.surface),
                )
                .collect::<BTreeSet<_>>();
            let selected = !touched.is_empty()
                && touched.iter().all(|surface| {
                    selected_transactions.get(surface) == Some(&group.transaction)
                });
            if selected {
                for surface in &touched {
                    if let Some(layer) = self.layers.get(surface) {
                        group.reproject_surface(*surface, layer.geometry);
                    }
                }
                for transaction in &mut group.transactions {
                    let generation = committed_generations
                        .entry(transaction.surface)
                        .or_insert(0);
                    transaction.previous_committed_generation = *generation;
                    *generation = generation.saturating_add(1);
                }
                self.released_admission_groups.push_back(group);
                continue;
            }
            let covered_by_later_backing = !touched.is_empty()
                && touched.iter().all(|surface| {
                    selected_positions
                        .get(surface)
                        .is_some_and(|selected_position| position < *selected_position)
                        && self
                            .layout_epochs
                            .safe_observation(*surface)
                            .is_some_and(|observation| {
                                observation.evidence
                                    == sophia_engine::SurfaceVisualEvidence::BackingSnapshot
                            })
                });
            if covered_by_later_backing {
                // A stable CPU handle may reach admission as Replace followed
                // by patches. Release the complete prefix so the selected
                // patch never outruns the renderer-owned replacement base.
                for surface in &touched {
                    if let Some(layer) = self.layers.get(surface) {
                        group.reproject_surface(*surface, layer.geometry);
                    }
                }
                for transaction in &mut group.transactions {
                    let generation = committed_generations
                        .entry(transaction.surface)
                        .or_insert(0);
                    transaction.previous_committed_generation = *generation;
                    *generation = generation.saturating_add(1);
                }
                self.released_admission_groups.push_back(group);
                continue;
            }
            let covered_by_later_present = !touched.is_empty()
                && touched.iter().all(|surface| {
                    selected_positions
                        .get(surface)
                        .is_some_and(|selected_position| position < *selected_position)
                        && self
                            .layout_epochs
                            .safe_observation(*surface)
                            .is_some_and(|observation| {
                                observation.evidence
                                    == sophia_engine::SurfaceVisualEvidence::PresentedBuffer
                            })
                });
            if covered_by_later_present {
                if !group.present_submissions.is_empty()
                    || !group.software_present_submissions.is_empty()
                {
                    group.superseded = true;
                    self.released_admission_groups.push_back(group);
                }
                continue;
            }
            retained.push_back(group);
        }
        self.pre_admission_groups = retained;
    }

    fn release_managed_admission_groups(&mut self) {
        let mut retained = VecDeque::with_capacity(self.pre_admission_groups.len());
        let mut committed_generations = self
            .layers
            .iter()
            .map(|(surface, layer)| (*surface, layer.generation))
            .collect::<BTreeMap<_, _>>();
        while let Some(mut group) = self.pre_admission_groups.pop_front() {
            let touched = group
                .transactions
                .iter()
                .map(|transaction| transaction.surface)
                .chain(
                    group
                        .present_submissions
                        .iter()
                        .map(|submission| submission.surface),
                )
                .chain(
                    group
                        .software_present_submissions
                        .iter()
                        .map(|submission| submission.surface),
                )
                .collect::<BTreeSet<_>>();
            if touched.is_empty()
                || touched
                    .iter()
                    .any(|surface| self.surface_requires_admission(*surface))
            {
                retained.push_back(group);
                continue;
            }
            for surface in &touched {
                if let Some(layer) = self.layers.get(surface) {
                    group.reproject_surface(*surface, layer.geometry);
                }
            }
            for transaction in &mut group.transactions {
                let generation = committed_generations
                    .entry(transaction.surface)
                    .or_insert(0);
                transaction.previous_committed_generation = *generation;
                *generation = generation.saturating_add(1);
            }
            self.released_admission_groups.push_back(group);
        }
        self.pre_admission_groups = retained;
    }

    /// Whether no admission group can be quarantined or released while one
    /// owner cycle projects several authority batches.
    ///
    /// `projected_batch` computes its quarantine set from the pre-admission
    /// and released queues and then *drains* the released one. If a group were
    /// released between two projections of the same cycle, the earlier
    /// projection would consume the release and the later batch would no
    /// longer filter its own transactions, committing them twice. With these
    /// three tables empty the release paths are no-ops, so that skew cannot
    /// arise. A pending layout is deliberately not part of the test: requiring
    /// one would disable merging through exactly the resize storms that
    /// produce the bursts worth merging.
    fn authority_merge_quiescent(&self) -> bool {
        self.pre_admission_groups.is_empty()
            && self.released_admission_groups.is_empty()
            && self.unmanaged_surfaces.is_empty()
    }

    fn admission_group_dma_bufs(&self) -> BTreeSet<sophia_protocol::BufferHandle> {
        self.pre_admission_groups
            .iter()
            .chain(self.released_admission_groups.iter())
            .flat_map(LiveAdmissionAuthorityGroup::dma_bufs)
            .collect()
    }

    fn admission_group_fences(&self) -> BTreeSet<sophia_protocol::FenceHandle> {
        self.pre_admission_groups
            .iter()
            .chain(self.released_admission_groups.iter())
            .flat_map(LiveAdmissionAuthorityGroup::fences)
            .collect()
    }

    fn admission_groups_reference_dma_buf(
        &self,
        handle: sophia_protocol::BufferHandle,
    ) -> bool {
        self.pre_admission_groups
            .iter()
            .chain(self.released_admission_groups.iter())
            .any(|group| group.dma_bufs().any(|candidate| candidate == handle))
    }

    fn admission_groups_reference_fence(
        &self,
        handle: sophia_protocol::FenceHandle,
    ) -> bool {
        self.pre_admission_groups
            .iter()
            .chain(self.released_admission_groups.iter())
            .any(|group| group.fences().any(|candidate| candidate == handle))
    }

    fn complete_admission_retirement(
        &mut self,
        visual_candidate: sophia_protocol::SurfaceTransactionKey,
    ) -> bool {
        if !self.admissions.complete_retirement(visual_candidate) {
            return false;
        }
        let surface = visual_candidate.surface;
        self.planning_surfaces.remove(&surface);
        self.unmanaged_surfaces.remove(&surface);
        self.admission_retries.remove(&surface);
        self.manage_settlements.remove(&surface);
        self.layout_epochs
            .set_admission(surface, sophia_engine::SurfaceAdmissionState::Managed);
        // The fallback frame's retirement completes admission, not the
        // standing layout target. Remove its temporary constraint now and
        // queue exactly one ordinary relayout while retaining the pixels.
        self.release_recovery_extent(surface, "admission_present_retired");
        self.release_managed_admission_groups();
        if let Some((expected, wm_transaction)) = self.retirement_focus.remove(&surface)
            && expected == visual_candidate
        {
            self.queue_focus_handoff(wm_transaction, surface);
            crate::session_println!(
                "sophia_live_wm schema=1 status=workspace_focus_restore_queued transaction={} surface={}",
                wm_transaction.raw(),
                surface.index(),
            );
        }
        crate::session_println!(
            "sophia_live_visual_admission schema=1 status=presented transaction={} surface={}",
            visual_candidate.transaction.raw(),
            surface.index(),
        );
        true
    }

    fn observe_terminal_present_feedback(
        &mut self,
        outcome: &sophia_backend_live::LivePresentFeedbackOutcome,
    ) {
        for feedback in &outcome.feedback {
            let sophia_backend_live::LivePresentProtocolFeedback::Complete {
                transaction,
                disposition: sophia_backend_live::LivePresentBufferDisposition::Skipped,
                ..
            } = feedback else {
                continue;
            };
            // Frontend transaction tickets are session-wide. Recover the full
            // bound candidate before changing either admission or resize state.
            let candidate = self.admissions.pending_surfaces().find_map(|surface| {
                match self.admissions.state(surface) {
                    sophia_engine::SurfacePresentationAdmissionState::AwaitingRetirement {
                        visual_candidate, ..
                    } if visual_candidate.transaction == *transaction => Some(visual_candidate),
                    _ => None,
                }
            });
            let Some(candidate) = candidate else { continue; };
            if !self.admissions.reject_retirement(candidate) {
                continue;
            }
            self.awaiting_visual_commits.reject(candidate);
            self.layout_epochs.reject_safe_observation(candidate);
            crate::session_println!(
                "sophia_live_visual_admission schema=1 status=retry_pixels transaction={} surface={}",
                candidate.transaction.raw(),
                candidate.surface.index(),
            );
            // A newer frame may already be quarantined. Give it a layout turn
            // without replaying the skipped buffer or marking either presented.
            self.constraint_relayout_required = true;
        }
    }

    fn remove_admission_groups(&mut self, surface: SurfaceId) {
        self.pre_admission_groups
            .retain(|group| !group.contains_surface(surface));
        self.released_admission_groups
            .retain(|group| !group.contains_surface(surface));
        // Withdrawal clears admission state, so a remap starts inactive again.
        // Carrying an authorization across that would let a stale key skip a
        // frame the new window legitimately presented.
        self.forget_escaped_presents(surface);
    }

    fn observe_presentation_intents(
        &mut self,
        batch: &XAuthorityObservedTransactionBatch,
        new_surfaces: &mut BTreeSet<SurfaceId>,
        withdrawn_surfaces: &mut BTreeSet<SurfaceId>,
    ) {
        for intent in &batch.presentation_intents {
            let facts = sophia_engine::SurfaceLayoutFacts::from(*intent);
            let changed = if self.bypass_policy_admission {
                match intent.kind {
                    sophia_protocol::SurfacePresentationIntentKind::Request => {
                        self.planning_surfaces.get(&intent.surface) != Some(&facts)
                    }
                    sophia_protocol::SurfacePresentationIntentKind::Withdraw => {
                        self.planning_surfaces.contains_key(&intent.surface)
                    }
                }
            } else {
                self.admissions.observe_intent(*intent)
            };
            match intent.kind {
                sophia_protocol::SurfacePresentationIntentKind::Request => {
                    // A re-request is a new question, whatever policy answered
                    // about the previous presentation of this surface.
                    self.manage_settlements.remove(&intent.surface);
                    self.planning_surfaces.insert(intent.surface, facts);
                    self.authority_surface_facts.insert(intent.surface, facts);
                    self.presentation_roles.insert(intent.surface, intent.role);
                    self.layout_epochs
                        .set_declared_constraints(intent.surface, intent.constraints);
                    if !self.bypass_policy_admission {
                        self.unmanaged_surfaces.insert(intent.surface);
                        self.layout_epochs.set_admission(
                            intent.surface,
                            sophia_engine::SurfaceAdmissionState::Unmanaged,
                        );
                    }
                    if changed {
                        new_surfaces.insert(intent.surface);
                    }
                }
                sophia_protocol::SurfacePresentationIntentKind::Withdraw => {
                    withdrawn_surfaces.insert(intent.surface);
                    self.admissions.remove(intent.surface);
                    self.planning_surfaces.remove(&intent.surface);
                    self.authority_surface_facts.remove(&intent.surface);
                    self.unmanaged_surfaces.remove(&intent.surface);
                    self.manage_settlements.remove(&intent.surface);
                    self.remove_admission_groups(intent.surface);
                }
            }
        }
    }

    fn acknowledge_admission_control(
        &mut self,
        transaction: TransactionId,
        surface: SurfaceId,
    ) -> bool {
        if !self.admissions.acknowledge_control(surface, transaction) {
            return false;
        }
        // The authority marks a policy-managed window viewable at the moment it
        // admits it, and that transition travels in no presentation of its own:
        // the engine asked for the map, so there is no client request whose
        // response could carry it. This acknowledgement is the authority's own
        // confirmation that the map happened, and it is already correlated --
        // `acknowledge_control` accepts it only from `ControlPending`, only for
        // the transaction that was issued, and moves the surface on so a repeat
        // is refused. A withdrawn or destroyed surface has no admission state
        // left and is refused for the same reason, so a late acknowledgement
        // cannot revive one.
        //
        // Recording the map here is what makes an admitted window eligible for
        // the scene. Without it the authority reports the window viewable to
        // its X client, which draws, while the session holds it unmapped and
        // composites nothing.
        self.mapped_surfaces.insert(surface);
        // A frame this surface presented before it was mapped is owned by
        // production with no admission claim on it, so nothing here will ever
        // settle it. This acknowledgement is the first point the map is a
        // confirmed fact rather than a request, and therefore the first point
        // the client can be expected to draw again once its buffer comes back.
        for escaped in &mut self.escaped_pre_admission {
            if escaped.key.surface == surface {
                escaped.ready = true;
            }
        }
        true
    }

    fn write_pending_cpu_buffer_handles(&self, handles: &mut Vec<u64>) {
        handles.clear();
        handles.extend(
            self.pre_admission_groups
                .iter()
                .chain(self.released_admission_groups.iter())
                .flat_map(|group| group.transactions.iter())
                .flat_map(|transaction| transaction.content.variants())
                .filter_map(|variant| match variant.source {
                    BufferSource::CpuBuffer { handle } => Some(handle),
                    _ => None,
                }),
        );
        handles.sort_unstable();
        handles.dedup();
    }

    fn knows_surface(&self, surface: SurfaceId) -> bool {
        self.layers.contains_key(&surface)
            || self.planning_surfaces.contains_key(&surface)
            || self.authority_surface_facts.contains_key(&surface)
    }

    fn layout_facts(&self, surface: SurfaceId) -> Option<sophia_engine::SurfaceLayoutFacts> {
        self.planning_surfaces.get(&surface).copied().or_else(|| {
            let layer = self.layers.get(&surface)?;
            Some(sophia_engine::SurfaceLayoutFacts {
                surface,
                role: self
                    .presentation_roles
                    .get(&surface)
                    .copied()
                    .unwrap_or_default(),
                kind: self
                    .surface_kinds
                    .get(&surface)
                    .copied()
                    .unwrap_or(sophia_protocol::LayoutNodeKind::Toplevel),
                placement_preference: self
                    .placement_preferences
                    .get(&surface)
                    .copied()
                    .unwrap_or_default(),
                presentation_owner: self.presentation_owners.get(&surface).copied(),
                stack_rank: self
                    .authority_stack_ranks
                    .get(&surface)
                    .copied()
                    .unwrap_or(layer.stack_rank),
                geometry: layer.geometry,
                constraints: self.layout_epochs.declared_constraints(surface),
                generation: layer.generation,
            })
        }).or_else(|| self.authority_surface_facts.get(&surface).copied())
    }

    fn is_policy_managed(&self, surface: SurfaceId) -> bool {
        !self.is_client_positioned(surface)
    }

    fn present_layout_disposition(
        &self,
        transaction: TransactionId,
        surface: SurfaceId,
        buffer: sophia_protocol::BufferHandle,
    ) -> sophia_backend_live::LiveProductionPresentDisposition {
        let actual = self.dma_buf_sizes.get(&buffer).copied();
        let candidate = sophia_protocol::SurfaceTransactionKey {
            transaction,
            surface,
            target_buffer: BufferSource::DmaBuf {
                handle: buffer.raw(),
            },
        };
        if actual.is_some_and(|size| {
            self.awaiting_visual_commits
                .exact_candidate(candidate, size)
        }) {
            // A launch or resize candidate already selected by the layout
            // epoch must reach exact native retirement before its standing
            // target becomes committed state. The standing target can differ
            // from this temporary recovery extent, so admit only the exact
            // armed identity here; later authority work remains behind the
            // production surface-content fence.
            return sophia_backend_live::LiveProductionPresentDisposition::Immediate;
        }
        let expected = self
            .pending
            .as_ref()
            .and_then(|pending| pending.requested_sizes.get(&surface).copied())
            .or_else(|| self.layout_epochs.pending_target(surface));
        match sophia_engine::classify_surface_visual_extent(
            actual,
            expected,
            self.layout_epochs.recovery_extent(surface),
        ) {
            sophia_engine::SurfaceVisualExtentDisposition::Expected if self.pending.is_some() => {
                sophia_backend_live::LiveProductionPresentDisposition::StageLayout {
                    epoch: self
                        .pending
                        .as_ref()
                        .expect("checked above")
                        .transaction,
                }
            }
            sophia_engine::SurfaceVisualExtentDisposition::Expected
            | sophia_engine::SurfaceVisualExtentDisposition::RetainedRecovery
            | sophia_engine::SurfaceVisualExtentDisposition::Unconstrained
            | sophia_engine::SurfaceVisualExtentDisposition::Mismatch => {
                // A valid Present that does not satisfy the pending resize is
                // still content for the committed X11 window. The renderer
                // clips it without promoting speculative layout geometry.
                sophia_backend_live::LiveProductionPresentDisposition::Immediate
            }
        }
    }
}
