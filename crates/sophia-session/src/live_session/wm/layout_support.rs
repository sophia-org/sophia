impl PersistentLiveLayout {
    /// Whether a policy-managed surface belongs in the scene.
    ///
    /// Mapping is the authority's fact; policy's projection is a cached view of
    /// it that is not refreshed in the step carrying an unmap. Asking policy
    /// alone therefore keeps a torn-down window compositing and taking pointer
    /// hits until policy catches up, and keeps the popups it owns on screen
    /// with it. Requiring the authority's mapping first means an unmapped
    /// surface leaves the scene on the cycle that unmaps it.
    ///
    /// This is the same requirement the client-positioned path has always
    /// placed on a popup's own mapping, applied to the managed surface too.
    fn managed_scene_visible<E>(
        &self,
        surface: SurfaceId,
        projected: impl FnOnce(SurfaceId) -> Result<bool, E>,
    ) -> Result<bool, E> {
        if !self.mapped_surfaces.contains(&surface) {
            return Ok(false);
        }
        projected(surface)
    }

    /// Whether a surface can still answer input.
    ///
    /// Mapping alone is not the question for a client-positioned surface. A
    /// popup stays mapped while the window that owns it is hidden, and it is
    /// not eligible then -- the owner chain decides, which is what
    /// `client_positioned_visible` already walks. Policy is not consulted:
    /// eligibility here is the authority's mapping, so a projection that has
    /// not caught up cannot keep a hidden surface answering.
    fn input_eligible(&self, surface: SurfaceId) -> bool {
        if self.is_client_positioned(surface) {
            self.client_positioned_visible::<()>(surface, |_| Ok(true))
                .unwrap_or(false)
        } else {
            self.mapped_surfaces.contains(&surface)
        }
    }

    /// Drops the claims the layout still holds on a surface that can no
    /// longer answer input.
    ///
    /// Clearing `focus_to_apply` and `retirement_focus` is not enough on its
    /// own. A staged proposal keeps its own `pending.focus`, and committing it
    /// puts that surface straight back into `retirement_focus` or queues a
    /// focus handoff for it, so the hidden window would take the keyboard again
    /// one commit later. The pending layout also stops positioning and sizing
    /// it, which is what destroy already does; the difference is that nothing
    /// here touches content, admission or the settlement identity, so the
    /// surface keeps everything it needs to come back.
    fn retire_hidden_input_claims(&mut self, surface: SurfaceId) {
        if self
            .focus_to_apply
            .is_some_and(|(_, pending)| pending == surface)
        {
            self.focus_to_apply = None;
        }
        self.retirement_focus.remove(&surface);
        if let Some(pending) = self.pending.as_mut() {
            if pending.focus == Some(surface) {
                pending.focus = None;
            }
            pending.layers.retain(|layer| layer.surface != surface);
            pending.requested_sizes.remove(&surface);
        }
    }

    /// Whether this batch can end any surface input eligibility.
    ///
    /// Most batches are repaints, and sweeping eligibility for one would put a
    /// per-frame allocation on the steady-state path for nothing. Only three
    /// things can take eligibility away: a surface being removed, a surface
    /// reporting itself unmapped, and a change of owner or role -- reparenting
    /// a mapped popup under a hidden window hides it without touching its own
    /// mapped bit. A surface the layout has not described yet counts as a role
    /// change, which is a lifecycle event and rare.
    fn batch_can_end_input_eligibility(
        &self,
        batch: &sophia_x_authority::XAuthorityObservedTransactionBatch,
    ) -> bool {
        !batch.removed_surfaces.is_empty()
            || batch.surface_presentations.iter().any(|presentation| {
                !presentation.mapped
                    || self.presentation_owners.get(&presentation.surface).copied()
                        != presentation.owner
                    || self.presentation_roles.get(&presentation.surface).copied()
                        != Some(presentation.role)
            })
    }

    /// Every surface that can currently answer input.
    ///
    /// Enumerated over the surfaces the authority has described rather than
    /// over retained layers: layers come from surface transactions, so a
    /// surface can be mapped and holding focus before one exists, and sweeping
    /// layers alone would never see it. Roles are purged on removal, so a
    /// destroyed surface drops out here without special handling.
    fn input_eligible_surfaces(&self) -> BTreeSet<SurfaceId> {
        self.presentation_roles
            .keys()
            .copied()
            .filter(|surface| self.input_eligible(*surface))
            .collect()
    }

    /// Surfaces that could answer input before and cannot now.
    ///
    /// Taken as a difference rather than from a mapped bit, because the event
    /// that ends a popup's eligibility is often not its own: hiding the window
    /// that owns it leaves the popup mapped and ineligible, and a surface whose
    /// own bit never changed would otherwise keep its focus, its pressed keys
    /// and its route lease.
    fn newly_ineligible_surfaces(&self, eligible_before: &BTreeSet<SurfaceId>) -> Vec<SurfaceId> {
        eligible_before
            .iter()
            .copied()
            .filter(|surface| !self.input_eligible(*surface))
            .collect()
    }

    fn client_positioned_visible<E>(
        &self,
        surface: SurfaceId,
        managed_visible: impl FnOnce(SurfaceId) -> Result<bool, E>,
    ) -> Result<bool, E> {
        let mut current = surface;
        // Panels and nested popups bypass WM placement. Follow their mapped
        // ownership chain before consulting policy for a managed ancestor.
        // A cycle or a stale owner cannot establish visible ancestry.
        for _ in 0..=self.presentation_owners.len() {
            // Mapping descriptions establish the authority lifetime before a
            // client supplies pixels. Layout/pixel caches cannot be the
            // prerequisite for grabbing a newly mapped popup.
            if !self.presentation_roles.contains_key(&current) {
                return Ok(false);
            }
            if !self.is_client_positioned(current) {
                return self.managed_scene_visible(current, managed_visible);
            }
            if !self.client_positioned_mapped(current) {
                return Ok(false);
            }
            match self.presentation_owner(current) {
                Some(owner) => current = owner,
                None => return Ok(true),
            }
        }
        Ok(false)
    }
}

fn reconcile_live_layout_progress(
    layout: &mut PersistentLiveLayout,
    update_slot_available: bool,
) -> LiveLayoutProgress {
    if !layout.pending_is_ready() {
        return LiveLayoutProgress::Blocked;
    }
    if !update_slot_available {
        return LiveLayoutProgress::DeferredReady;
    }
    LiveLayoutProgress::Committed(
        layout
            .resolve_pending()
            .expect("ready pending layout resolves when its output slot is available"),
    )
}

fn wm_update_coordinator_batch(
    transaction: TransactionId,
) -> XAuthorityObservedTransactionBatch {
    XAuthorityObservedTransactionBatch {
        client: None,
        admission: None,
        surface_routes: Vec::new(),
        transaction,
        transactions: Vec::new(),
        surface_presentations: Vec::new(),
        presentation_intents: Vec::new(),
        removed_surfaces: Vec::new(),
        surface_output_reservations: Vec::new(),
        cpu_buffer_updates: Vec::new(),
        raster_responses: Vec::new(),
        dma_buf_registrations: Vec::new(),
        fence_registrations: Vec::new(),
        present_submissions: Vec::new(),
        software_present_submissions: Vec::new(),
        released_dma_bufs: Vec::new(),
        released_fences: Vec::new(),
        protocol_errors: Vec::new(),
        expected_protocol_errors: Vec::new(),
        metadata: Vec::new(),
        selection_owner_change: false,
        selection_conversion: false,
    }
}

fn center_geometry_without_scaling(mut geometry: Rect, output: Size) -> Rect {
    geometry.x = output.width.saturating_sub(geometry.width).max(0) / 2;
    geometry.y = output.height.saturating_sub(geometry.height).max(0) / 2;
    geometry
}

fn successful_primary_exit_ends_session(input_proof_requested: bool) -> bool {
    !input_proof_requested
}

/// Whether a terminal client's non-zero exit is this session's failure.
///
/// A CLIENT THAT DIES WHILE THE SESSION IS SHUTTING IT DOWN IS NOT A FAULT.
/// Quiescence closes the X server the clients are connected to, so a terminal
/// loses its connection and exits non-zero as a direct consequence -- xterm
/// reports `fatal IO error 11` and exits 84. Reported as a client fatal, that
/// turned every deliberate shutdown into `session_failure failure_code=
/// unclassified`, which is how the QEMU session gate stayed red from
/// 2026-08-28: the session failed on the consequence of its own tick limit,
/// with `cleanup_errors=0` on the very next line.
///
/// While the session is live it is still a fault, and that is the whole of the
/// difference. The exit is recorded either way; what this decides is whether
/// it ends the session as a failure.
fn terminal_exit_is_session_failure(
    status_success: bool,
    normal_session: bool,
    quiescing: bool,
) -> bool {
    !status_success && !normal_session && !quiescing
}

fn global_runtime_deadline_ends_session(input_proof_requested: bool) -> bool {
    !input_proof_requested
}
