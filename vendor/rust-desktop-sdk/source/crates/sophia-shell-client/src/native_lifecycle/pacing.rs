use super::*;

/// One parentless surface. The helper fills allocation, scale, generations,
/// permit and row action bindings. Placements must name Accepted resources.
pub struct NativeScene {
    pub placements: Vec<ContentPlacement>,
    pub targets: Vec<ContentTarget>,
    pub rows: Vec<u16>,
    pub selected: u16,
}
#[derive(Clone, Copy)]
pub(super) struct Demand {
    id: u64,
    output: ContentOutputId,
    at: u64,
    ticket: Option<Ticket>,
    permit: u64,
    expires: u64,
    max_bytes: u32,
    usable: bool,
    cancelled: bool,
}
#[derive(Default)]
pub(super) struct Pacing {
    demand: Option<Demand>,
    pub(super) consumed: Option<Demand>,
    next: u64,
    last_inactive: u64,
}
impl NativeLauncherLifecycle {
    /// Earliest local permit bound, computed from demand enqueue time. This is
    /// advisory: the server's issue-time clock origin is not on the wire.
    pub fn permit_deadline_ms(&self) -> Option<u64> {
        self.pacing.demand.filter(|d| d.usable).map(|d| d.expires)
    }
    pub fn service_at(
        &mut self,
        connection: &ShellConnection,
        now_ms: u64,
    ) -> Result<(), ShellClientError> {
        self.service(connection)?;
        if let Some(d) = &mut self.pacing.demand
            && now_ms >= d.expires
        {
            d.usable = false;
        }
        Ok(())
    }
    pub(super) fn frame_permit(
        &mut self,
        value: &ContentFramePermit,
    ) -> Result<(), ShellClientError> {
        if let Some(d) = &mut self.pacing.demand
            && d.id == value.demand_id
        {
            if d.output != value.output {
                return Err(ShellClientError::Protocol("native permit output"));
            }
            if value.state == 1 {
                d.permit = value.permit_id;
                d.max_bytes = value.max_candidate_bytes;
                d.expires = d.at.saturating_add(u64::from(value.ttl_ms));
                d.usable = !d.cancelled;
            } else {
                self.pacing.last_inactive = d.id;
                self.pacing.demand = None;
            }
            return Ok(());
        }
        if self
            .pacing
            .consumed
            .is_some_and(|d| d.id == value.demand_id)
        {
            // If expiry precedes the candidate's submit refusal, do not later
            // resurrect a wait for the expiry event we have already observed.
            if value.state != 1 {
                self.pacing.last_inactive = value.demand_id;
                self.pacing.consumed = None;
            }
            return Ok(());
        }
        if value.demand_id != 0 && value.demand_id == self.pacing.last_inactive {
            return Ok(());
        }
        Err(ShellClientError::Protocol("native permit correlation"))
    }
    pub(super) fn candidate_refused(&mut self) {
        if let Some(mut demand) = self.pacing.consumed.take() {
            demand.usable = false;
            self.pacing.demand = Some(demand);
        }
    }
    pub(super) fn service_pacing(&mut self, connection: &ShellConnection) {
        if let Some(d) = &mut self.pacing.demand
            && let Some(ticket) = d.ticket
        {
            let outcome = connection.custody(ticket);
            if !matches!(outcome, Some(Custody::Queued | Custody::InFlight)) {
                d.ticket = None;
                if d.permit == 0
                    && matches!(outcome, Some(Custody::Refused(_) | Custody::DroppedUnsent))
                {
                    self.pacing.demand = None;
                }
            }
        }
    }
}

impl ShellConnection {
    pub fn demand_native_frame(
        &mut self,
        lifecycle: &mut NativeLauncherLifecycle,
        transaction: TransactionId,
        reason: u16,
        now_ms: u64,
    ) -> Result<Admission, ShellClientError> {
        lifecycle.service_at(self, now_ms)?;
        let opening = lifecycle
            .opening
            .ok_or(ShellClientError::InvalidArgument("native opening absent"))?;
        let allocation = lifecycle
            .allocation()
            .ok_or(ShellClientError::InvalidArgument(
                "native allocation absent",
            ))?;
        if lifecycle.pacing.demand.is_some() || lifecycle.pending.is_some() {
            return Err(ShellClientError::InvalidArgument(
                "native demand already outstanding",
            ));
        }
        let id = lifecycle.pacing.next.max(1);
        let next = id
            .checked_add(1)
            .ok_or(ShellClientError::InvalidArgument("native demand exhausted"))?;
        let record = ShellContentRecord::FrameDemand(ContentFrameDemand {
            grant: lifecycle.limits.grant,
            output: opening.output,
            allocation: allocation.allocation,
            demand_id: id,
            reason,
        });
        let admission = self.enqueue_content_tracked(transaction, &record)?;
        lifecycle.pacing.next = next;
        lifecycle.pacing.demand = Some(Demand {
            id,
            output: opening.output,
            at: now_ms,
            ticket: Some(admission.first),
            permit: 0,
            expires: 0,
            max_bytes: 0,
            usable: false,
            cancelled: false,
        });
        Ok(admission)
    }
    /// Cancel only a granted permit well inside its advisory bound. Local
    /// expiry never cancels server authority or permits a replacement demand.
    pub fn cancel_native_demand(
        &mut self,
        lifecycle: &mut NativeLauncherLifecycle,
        transaction: TransactionId,
        now_ms: u64,
        margin_ms: u32,
    ) -> Result<Admission, ShellClientError> {
        lifecycle.service_at(self, now_ms)?;
        let d = lifecycle
            .pacing
            .demand
            .filter(|d| d.usable && d.expires.saturating_sub(now_ms) > u64::from(margin_ms))
            .ok_or(ShellClientError::InvalidArgument(
                "native permit unavailable",
            ))?;
        let record = ShellContentRecord::FrameDemandCancel(ContentFrameDemandCancel {
            grant: lifecycle.limits.grant,
            output: d.output,
            demand_id: d.id,
            permit_id: d.permit,
        });
        let admission = self.enqueue_content_tracked(transaction, &record)?;
        lifecycle.pacing.demand.as_mut().unwrap().usable = false;
        lifecycle.pacing.demand.as_mut().unwrap().cancelled = true;
        Ok(admission)
    }

    /// Build and admit one native candidate from tracked allocation, accepted
    /// resources and a usable permit. No identity is borrowed from a later
    /// opening. The margin reduces stale work but cannot prove server validity.
    pub fn present_native_scene(
        &mut self,
        lifecycle: &mut NativeLauncherLifecycle,
        transaction: TransactionId,
        scene: &NativeScene,
        now_ms: u64,
        margin_ms: u32,
    ) -> Result<Admission, ShellClientError> {
        lifecycle.service_at(self, now_ms)?;
        let opening = lifecycle
            .opening
            .ok_or(ShellClientError::InvalidArgument("native opening absent"))?;
        let a = lifecycle
            .allocation()
            .ok_or(ShellClientError::InvalidArgument(
                "native allocation absent",
            ))?;
        let d = lifecycle
            .pacing
            .demand
            .filter(|d| d.usable && d.expires.saturating_sub(now_ms) > u64::from(margin_ms))
            .ok_or(ShellClientError::InvalidArgument(
                "native permit unavailable",
            ))?;
        if lifecycle.allocation.pending.is_some()
            || lifecycle.allocation.opening != opening.opening
            || a.output != opening.output
            || scene.placements.len() > 32
            || scene.rows.len() > 32
            || scene.targets.len() != scene.rows.len()
            || scene.placements.iter().any(|p| {
                lifecycle.resource_state(p.resource) != Some(NativeResourceState::Accepted)
            })
        {
            return Err(ShellClientError::InvalidArgument("native scene resources"));
        }
        let generation = lifecycle.candidate_high_water.checked_add(1).ok_or(
            ShellClientError::InvalidArgument("native candidate exhausted"),
        )?;
        let mut placements = scene.placements.clone();
        for p in &mut placements {
            p.surface_index = 0;
        }
        let mut targets = scene.targets.clone();
        for (target, row) in targets.iter_mut().zip(&scene.rows) {
            target.surface_index = 0;
            target.action_kind = 2;
            target.action_id = u64::from(*row);
        }
        let value = ShellFileNativeCandidate {
            transaction,
            candidate: NativeContentCandidate {
                opening: opening.opening,
                catalog_generation: opening.catalog_generation,
                state_revision: lifecycle.revision,
                selected: scene.selected,
                rows: scene.rows.clone(),
                candidate: ContentCandidate {
                    grant: lifecycle.limits.grant,
                    candidate_generation: generation,
                    output: opening.output,
                    facts_generation: lifecycle.facts,
                    pacing_permit: d.permit,
                    interaction_generation: 1,
                    placements,
                    targets,
                    surfaces: vec![ContentSurface {
                        allocation: a.allocation,
                        scale_generation: a.scale_generation,
                        role: 3,
                        edge: lifecycle.allocation.edge,
                        margins: a.margins,
                        reservation_extent: 0,
                        parent_surface_index: u16::MAX,
                        anchor_parent_rect: ContentPixelRect::default(),
                    }],
                },
            },
        };
        let bytes = encode_shell_file_native_candidate(
            ShellFileHeader {
                kind: ShellFileKind::NativeCandidate,
                connection_epoch: self.connection_epoch(),
                submission_id: 1,
                sequence: 0,
            },
            &value,
        )?
        .len();
        if bytes > d.max_bytes as usize || bytes > lifecycle.limits.max_candidate_bytes as usize {
            return Err(ShellClientError::InvalidArgument(
                "native candidate exceeds permit",
            ));
        }
        let admission = self.enqueue_native_candidate_with_lifecycle(lifecycle, &value)?;
        lifecycle.pacing.last_inactive = d.id;
        lifecycle.pacing.consumed = lifecycle.pacing.demand.take();
        Ok(admission)
    }
}
