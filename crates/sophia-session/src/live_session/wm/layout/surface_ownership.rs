impl PersistentLiveLayout {
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
}
