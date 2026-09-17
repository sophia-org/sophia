use super::*;
use crate::IpcCodecError;
use crate::ipc::cursor::Cursor;

pub(crate) trait Wire: Sized {
    fn put(&self, bytes: &mut Vec<u8>);
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError>;
}

macro_rules! integer {
    ($ty:ty, $read:ident) => {
        impl Wire for $ty {
            fn put(&self, bytes: &mut Vec<u8>) {
                bytes.extend_from_slice(&self.to_le_bytes());
            }
            fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
                Ok(cursor.$read()? as Self)
            }
        }
    };
}
integer!(u16, u16);
integer!(u32, u32);
integer!(u64, u64);
integer!(i16, u16);
integer!(i32, i32);

/// Reserved fields exist only on the wire, never as mutable record state.
pub(crate) fn reserved<T: Wire + Default + PartialEq>(
    cursor: &mut Cursor<'_>,
) -> Result<(), IpcCodecError> {
    if T::take(cursor)? != T::default() {
        return Err(IpcCodecError::ReservedNonZero(1));
    }
    Ok(())
}

macro_rules! fields {
    ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        impl Wire for $name {
            fn put(&self, bytes: &mut Vec<u8>) {
                $(self.$field.put(bytes);)*
            }
            fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
                Ok(Self { $($field: <$ty>::take(cursor)?),* })
            }
        }
    };
}

fields!(ContentGrant {
    connection_epoch: u64,
    content_grant_epoch: u64,
});

fields!(ContentResourceId {
    id: u64,
    generation: u64,
});

fields!(ContentAllocationId {
    id: u64,
    generation: u64,
});

fields!(ContentOutputId {
    id: u64,
    generation: u64,
});

fields!(ContentLogicalRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
});

fields!(ContentPixelRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
});

fields!(ContentMargins {
    top: i16,
    right: i16,
    bottom: i16,
    left: i16,
});

impl Wire for ContentLimits {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.grant.put(bytes);
        self.limits_generation.put(bytes);
        self.max_resource_bytes.put(bytes);
        self.max_staging_bytes.put(bytes);
        self.max_resident_bytes.put(bytes);
        self.max_retiring_bytes.put(bytes);
        self.max_session_retiring_bytes.put(bytes);
        self.pixel_format_mask.put(bytes);
        self.effect_mask.put(bytes);
        self.max_frame_payload.put(bytes);
        self.max_chunk_bytes.put(bytes);
        self.max_width_px.put(bytes);
        self.max_height_px.put(bytes);
        self.max_live_resources.put(bytes);
        self.max_resource_ids.put(bytes);
        self.max_open_transfers.put(bytes);
        self.max_outputs.put(bytes);
        self.max_allocations_total.put(bytes);
        self.max_allocations_per_output.put(bytes);
        self.max_panels_per_output.put(bytes);
        self.max_popouts_per_output.put(bytes);
        self.max_candidate_surfaces.put(bytes);
        self.max_candidate_placements.put(bytes);
        self.max_candidate_targets.put(bytes);
        self.max_candidate_bytes.put(bytes);
        self.max_pending_allocation_requests.put(bytes);
        self.max_open_candidates_total.put(bytes);
        self.max_open_candidates_per_output.put(bytes);
        self.max_pending_candidates_total.put(bytes);
        self.max_pending_candidates_per_output.put(bytes);
        self.max_pending_actions.put(bytes);
        self.max_frame_demands_per_output.put(bytes);
        self.max_control_records.put(bytes);
        self.reserved_control_queue_bytes.put(bytes);
        self.max_input_queue_bytes.put(bytes);
        self.max_output_queue_bytes.put(bytes);
        self.max_frames_per_service_tick.put(bytes);
        self.max_panel_extent.put(bytes);
        self.max_popout_extent_px.put(bytes);
        self.max_reservation_extent.put(bytes);
        self.max_content_coverage_percent.put(bytes);
        self.max_margin_logical.put(bytes);
        self.max_scale_numerator.put(bytes);
        self.max_scale_denominator.put(bytes);
        self.allocation_timeout_ms.put(bytes);
        self.transfer_timeout_ms.put(bytes);
        self.transfer_idle_timeout_ms.put(bytes);
        self.candidate_timeout_ms.put(bytes);
        self.preparation_timeout_ms.put(bytes);
        self.presentation_timeout_ms.put(bytes);
        self.action_ack_timeout_ms.put(bytes);
        self.permit_timeout_ms.put(bytes);
        self.peer_write_timeout_ms.put(bytes);
        self.max_candidate_rate_millihz.put(bytes);
        0u32.put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let grant = ContentGrant::take(cursor)?;
        let limits_generation = u64::take(cursor)?;
        let max_resource_bytes = u64::take(cursor)?;
        let max_staging_bytes = u64::take(cursor)?;
        let max_resident_bytes = u64::take(cursor)?;
        let max_retiring_bytes = u64::take(cursor)?;
        let max_session_retiring_bytes = u64::take(cursor)?;
        let pixel_format_mask = u64::take(cursor)?;
        let effect_mask = u64::take(cursor)?;
        let max_frame_payload = u32::take(cursor)?;
        let max_chunk_bytes = u32::take(cursor)?;
        let max_width_px = u32::take(cursor)?;
        let max_height_px = u32::take(cursor)?;
        let max_live_resources = u32::take(cursor)?;
        let max_resource_ids = u32::take(cursor)?;
        let max_open_transfers = u32::take(cursor)?;
        let max_outputs = u32::take(cursor)?;
        let max_allocations_total = u32::take(cursor)?;
        let max_allocations_per_output = u32::take(cursor)?;
        let max_panels_per_output = u32::take(cursor)?;
        let max_popouts_per_output = u32::take(cursor)?;
        let max_candidate_surfaces = u32::take(cursor)?;
        let max_candidate_placements = u32::take(cursor)?;
        let max_candidate_targets = u32::take(cursor)?;
        let max_candidate_bytes = u32::take(cursor)?;
        let max_pending_allocation_requests = u32::take(cursor)?;
        let max_open_candidates_total = u32::take(cursor)?;
        let max_open_candidates_per_output = u32::take(cursor)?;
        let max_pending_candidates_total = u32::take(cursor)?;
        let max_pending_candidates_per_output = u32::take(cursor)?;
        let max_pending_actions = u32::take(cursor)?;
        let max_frame_demands_per_output = u32::take(cursor)?;
        let max_control_records = u32::take(cursor)?;
        let reserved_control_queue_bytes = u32::take(cursor)?;
        let max_input_queue_bytes = u32::take(cursor)?;
        let max_output_queue_bytes = u32::take(cursor)?;
        let max_frames_per_service_tick = u32::take(cursor)?;
        let max_panel_extent = u32::take(cursor)?;
        let max_popout_extent_px = u32::take(cursor)?;
        let max_reservation_extent = u32::take(cursor)?;
        let max_content_coverage_percent = u32::take(cursor)?;
        let max_margin_logical = u32::take(cursor)?;
        let max_scale_numerator = u32::take(cursor)?;
        let max_scale_denominator = u32::take(cursor)?;
        let allocation_timeout_ms = u32::take(cursor)?;
        let transfer_timeout_ms = u32::take(cursor)?;
        let transfer_idle_timeout_ms = u32::take(cursor)?;
        let candidate_timeout_ms = u32::take(cursor)?;
        let preparation_timeout_ms = u32::take(cursor)?;
        let presentation_timeout_ms = u32::take(cursor)?;
        let action_ack_timeout_ms = u32::take(cursor)?;
        let permit_timeout_ms = u32::take(cursor)?;
        let peer_write_timeout_ms = u32::take(cursor)?;
        let max_candidate_rate_millihz = u32::take(cursor)?;
        reserved::<u32>(cursor)?;
        Ok(Self {
            grant,
            limits_generation,
            max_resource_bytes,
            max_staging_bytes,
            max_resident_bytes,
            max_retiring_bytes,
            max_session_retiring_bytes,
            pixel_format_mask,
            effect_mask,
            max_frame_payload,
            max_chunk_bytes,
            max_width_px,
            max_height_px,
            max_live_resources,
            max_resource_ids,
            max_open_transfers,
            max_outputs,
            max_allocations_total,
            max_allocations_per_output,
            max_panels_per_output,
            max_popouts_per_output,
            max_candidate_surfaces,
            max_candidate_placements,
            max_candidate_targets,
            max_candidate_bytes,
            max_pending_allocation_requests,
            max_open_candidates_total,
            max_open_candidates_per_output,
            max_pending_candidates_total,
            max_pending_candidates_per_output,
            max_pending_actions,
            max_frame_demands_per_output,
            max_control_records,
            reserved_control_queue_bytes,
            max_input_queue_bytes,
            max_output_queue_bytes,
            max_frames_per_service_tick,
            max_panel_extent,
            max_popout_extent_px,
            max_reservation_extent,
            max_content_coverage_percent,
            max_margin_logical,
            max_scale_numerator,
            max_scale_denominator,
            allocation_timeout_ms,
            transfer_timeout_ms,
            transfer_idle_timeout_ms,
            candidate_timeout_ms,
            preparation_timeout_ms,
            presentation_timeout_ms,
            action_ack_timeout_ms,
            permit_timeout_ms,
            peer_write_timeout_ms,
            max_candidate_rate_millihz,
        })
    }
}

impl Wire for ContentAdmissionRefused {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.reason.put(bytes);
        0u16.put(bytes);
        self.denied_capabilities.put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let reason = u16::take(cursor)?;
        reserved::<u16>(cursor)?;
        let denied_capabilities = u64::take(cursor)?;
        Ok(Self {
            reason,
            denied_capabilities,
        })
    }
}

impl Wire for ContentAllocationRequest {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.grant.put(bytes);
        self.output.put(bytes);
        self.allocation_request_id.put(bytes);
        self.operation.put(bytes);
        self.role.put(bytes);
        self.edge.put(bytes);
        0u16.put(bytes);
        self.prior.put(bytes);
        self.parent.put(bytes);
        self.parent_presentation_epoch.put(bytes);
        self.anchor_parent_rect.put(bytes);
        self.desired_width.put(bytes);
        self.desired_height.put(bytes);
        self.margins.put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let grant = ContentGrant::take(cursor)?;
        let output = ContentOutputId::take(cursor)?;
        let allocation_request_id = u64::take(cursor)?;
        let operation = u16::take(cursor)?;
        let role = u16::take(cursor)?;
        let edge = u16::take(cursor)?;
        reserved::<u16>(cursor)?;
        let prior = ContentAllocationId::take(cursor)?;
        let parent = ContentAllocationId::take(cursor)?;
        let parent_presentation_epoch = u64::take(cursor)?;
        let anchor_parent_rect = ContentPixelRect::take(cursor)?;
        let desired_width = u32::take(cursor)?;
        let desired_height = u32::take(cursor)?;
        let margins = ContentMargins::take(cursor)?;
        Ok(Self {
            grant,
            output,
            allocation_request_id,
            operation,
            role,
            edge,
            prior,
            parent,
            parent_presentation_epoch,
            anchor_parent_rect,
            desired_width,
            desired_height,
            margins,
        })
    }
}

impl Wire for ContentAllocationResult {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.grant.put(bytes);
        self.allocation_request_id.put(bytes);
        self.status.put(bytes);
        self.reason.put(bytes);
        0u32.put(bytes);
        self.output.put(bytes);
        self.allocation.put(bytes);
        self.parent.put(bytes);
        self.scale_generation.put(bytes);
        self.logical.put(bytes);
        self.pixel.put(bytes);
        self.scale_numerator.put(bytes);
        self.scale_denominator.put(bytes);
        self.allowed_reservation_extent.put(bytes);
        self.margins.put(bytes);
        self.acknowledged_anchor.put(bytes);
        0u32.put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let grant = ContentGrant::take(cursor)?;
        let allocation_request_id = u64::take(cursor)?;
        let status = u16::take(cursor)?;
        let reason = u16::take(cursor)?;
        reserved::<u32>(cursor)?;
        let output = ContentOutputId::take(cursor)?;
        let allocation = ContentAllocationId::take(cursor)?;
        let parent = ContentAllocationId::take(cursor)?;
        let scale_generation = u64::take(cursor)?;
        let logical = ContentLogicalRect::take(cursor)?;
        let pixel = ContentPixelRect::take(cursor)?;
        let scale_numerator = u32::take(cursor)?;
        let scale_denominator = u32::take(cursor)?;
        let allowed_reservation_extent = u32::take(cursor)?;
        let margins = ContentMargins::take(cursor)?;
        let acknowledged_anchor = ContentPixelRect::take(cursor)?;
        reserved::<u32>(cursor)?;
        Ok(Self {
            grant,
            allocation_request_id,
            status,
            reason,
            output,
            allocation,
            parent,
            scale_generation,
            logical,
            pixel,
            scale_numerator,
            scale_denominator,
            allowed_reservation_extent,
            margins,
            acknowledged_anchor,
        })
    }
}

impl Wire for ContentResourceBegin {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.grant.put(bytes);
        self.resource.put(bytes);
        self.width_px.put(bytes);
        self.height_px.put(bytes);
        self.rendered_scale_numerator.put(bytes);
        self.rendered_scale_denominator.put(bytes);
        self.pixel_format.put(bytes);
        0u16.put(bytes);
        self.chunk_count.put(bytes);
        self.total_bytes.put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let grant = ContentGrant::take(cursor)?;
        let resource = ContentResourceId::take(cursor)?;
        let width_px = u32::take(cursor)?;
        let height_px = u32::take(cursor)?;
        let rendered_scale_numerator = u32::take(cursor)?;
        let rendered_scale_denominator = u32::take(cursor)?;
        let pixel_format = u16::take(cursor)?;
        reserved::<u16>(cursor)?;
        let chunk_count = u32::take(cursor)?;
        let total_bytes = u64::take(cursor)?;
        Ok(Self {
            grant,
            resource,
            width_px,
            height_px,
            rendered_scale_numerator,
            rendered_scale_denominator,
            pixel_format,
            chunk_count,
            total_bytes,
        })
    }
}

fields!(ContentResourceStatus {
    grant: ContentGrant,
    resource: ContentResourceId,
    status: u16,
    reason: u16,
    next_ordinal: u32,
    admitted_bytes: u64,
});

impl Wire for ContentResourceEnd {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.grant.put(bytes);
        self.resource.put(bytes);
        self.total_bytes.put(bytes);
        self.chunk_count.put(bytes);
        0u32.put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let grant = ContentGrant::take(cursor)?;
        let resource = ContentResourceId::take(cursor)?;
        let total_bytes = u64::take(cursor)?;
        let chunk_count = u32::take(cursor)?;
        reserved::<u32>(cursor)?;
        Ok(Self {
            grant,
            resource,
            total_bytes,
            chunk_count,
        })
    }
}

fields!(ContentResourceCancel {
    grant: ContentGrant,
    resource: ContentResourceId,
});

fields!(ContentResourceRetire {
    grant: ContentGrant,
    resource: ContentResourceId,
});

fields!(ContentResourceReleased {
    grant: ContentGrant,
    resource: ContentResourceId,
    reason: u16,
});

impl Wire for ContentCandidateBegin {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.grant.put(bytes);
        self.candidate_generation.put(bytes);
        self.output.put(bytes);
        self.facts_generation.put(bytes);
        self.pacing_permit.put(bytes);
        self.interaction_generation.put(bytes);
        self.surface_count.put(bytes);
        self.placement_count.put(bytes);
        self.target_count.put(bytes);
        0u32.put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let grant = ContentGrant::take(cursor)?;
        let candidate_generation = u64::take(cursor)?;
        let output = ContentOutputId::take(cursor)?;
        let facts_generation = u64::take(cursor)?;
        let pacing_permit = u64::take(cursor)?;
        let interaction_generation = u64::take(cursor)?;
        let surface_count = u32::take(cursor)?;
        let placement_count = u32::take(cursor)?;
        let target_count = u32::take(cursor)?;
        reserved::<u32>(cursor)?;
        Ok(Self {
            grant,
            candidate_generation,
            output,
            facts_generation,
            pacing_permit,
            interaction_generation,
            surface_count,
            placement_count,
            target_count,
        })
    }
}

impl Wire for ContentCandidateEnd {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.grant.put(bytes);
        self.candidate_generation.put(bytes);
        self.surface_count.put(bytes);
        self.placement_count.put(bytes);
        self.target_count.put(bytes);
        0u32.put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let grant = ContentGrant::take(cursor)?;
        let candidate_generation = u64::take(cursor)?;
        let surface_count = u32::take(cursor)?;
        let placement_count = u32::take(cursor)?;
        let target_count = u32::take(cursor)?;
        reserved::<u32>(cursor)?;
        Ok(Self {
            grant,
            candidate_generation,
            surface_count,
            placement_count,
            target_count,
        })
    }
}

fields!(ContentCandidateOutcome {
    grant: ContentGrant,
    candidate_generation: u64,
    output: ContentOutputId,
    kind: u16,
    reason: u16,
    presentation_epoch: u64,
    work_area_generation: u64,
    wm_commit_generation: u64,
});

fields!(ContentFrameDemand {
    grant: ContentGrant,
    output: ContentOutputId,
    allocation: ContentAllocationId,
    demand_id: u64,
    reason: u16,
});

impl Wire for ContentFramePermit {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.grant.put(bytes);
        self.output.put(bytes);
        self.demand_id.put(bytes);
        self.permit_id.put(bytes);
        self.state.put(bytes);
        self.reason.put(bytes);
        self.ttl_ms.put(bytes);
        self.max_candidate_bytes.put(bytes);
        0u32.put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let grant = ContentGrant::take(cursor)?;
        let output = ContentOutputId::take(cursor)?;
        let demand_id = u64::take(cursor)?;
        let permit_id = u64::take(cursor)?;
        let state = u16::take(cursor)?;
        let reason = u16::take(cursor)?;
        let ttl_ms = u32::take(cursor)?;
        let max_candidate_bytes = u32::take(cursor)?;
        reserved::<u32>(cursor)?;
        Ok(Self {
            grant,
            output,
            demand_id,
            permit_id,
            state,
            reason,
            ttl_ms,
            max_candidate_bytes,
        })
    }
}

fields!(ContentFrameDemandCancel {
    grant: ContentGrant,
    output: ContentOutputId,
    demand_id: u64,
    permit_id: u64,
});

impl Wire for ContentAction {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.grant.put(bytes);
        self.output.put(bytes);
        self.candidate_generation.put(bytes);
        self.presentation_epoch.put(bytes);
        self.interaction_generation.put(bytes);
        self.allocation.put(bytes);
        self.target_id.put(bytes);
        self.target_generation.put(bytes);
        self.action_id.put(bytes);
        self.event_id.put(bytes);
        self.kind.put(bytes);
        self.reason.put(bytes);
        0u32.put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let grant = ContentGrant::take(cursor)?;
        let output = ContentOutputId::take(cursor)?;
        let candidate_generation = u64::take(cursor)?;
        let presentation_epoch = u64::take(cursor)?;
        let interaction_generation = u64::take(cursor)?;
        let allocation = ContentAllocationId::take(cursor)?;
        let target_id = u64::take(cursor)?;
        let target_generation = u64::take(cursor)?;
        let action_id = u64::take(cursor)?;
        let event_id = u64::take(cursor)?;
        let kind = u16::take(cursor)?;
        let reason = u16::take(cursor)?;
        reserved::<u32>(cursor)?;
        Ok(Self {
            grant,
            output,
            candidate_generation,
            presentation_epoch,
            interaction_generation,
            allocation,
            target_id,
            target_generation,
            action_id,
            event_id,
            kind,
            reason,
        })
    }
}

impl Wire for ContentActionAck {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.grant.put(bytes);
        self.output.put(bytes);
        self.candidate_generation.put(bytes);
        self.presentation_epoch.put(bytes);
        self.interaction_generation.put(bytes);
        self.allocation.put(bytes);
        self.target_id.put(bytes);
        self.target_generation.put(bytes);
        self.action_id.put(bytes);
        self.event_id.put(bytes);
        self.disposition.put(bytes);
        0u16.put(bytes);
        0u32.put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let grant = ContentGrant::take(cursor)?;
        let output = ContentOutputId::take(cursor)?;
        let candidate_generation = u64::take(cursor)?;
        let presentation_epoch = u64::take(cursor)?;
        let interaction_generation = u64::take(cursor)?;
        let allocation = ContentAllocationId::take(cursor)?;
        let target_id = u64::take(cursor)?;
        let target_generation = u64::take(cursor)?;
        let action_id = u64::take(cursor)?;
        let event_id = u64::take(cursor)?;
        let disposition = u16::take(cursor)?;
        reserved::<u16>(cursor)?;
        reserved::<u32>(cursor)?;
        Ok(Self {
            grant,
            output,
            candidate_generation,
            presentation_epoch,
            interaction_generation,
            allocation,
            target_id,
            target_generation,
            action_id,
            event_id,
            disposition,
        })
    }
}

fields!(ContentOutputFactsEntry {
    output: ContentOutputId,
    local_width: u32,
    local_height: u32,
    scale_numerator: u32,
    scale_denominator: u32,
    scale_generation: u64,
});

impl Wire for ContentSurface {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.allocation.put(bytes);
        self.scale_generation.put(bytes);
        self.role.put(bytes);
        self.edge.put(bytes);
        self.margins.put(bytes);
        self.reservation_extent.put(bytes);
        self.parent_surface_index.put(bytes);
        0u16.put(bytes);
        self.anchor_parent_rect.put(bytes);
        0u32.put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let allocation = ContentAllocationId::take(cursor)?;
        let scale_generation = u64::take(cursor)?;
        let role = u16::take(cursor)?;
        let edge = u16::take(cursor)?;
        let margins = ContentMargins::take(cursor)?;
        let reservation_extent = u32::take(cursor)?;
        let parent_surface_index = u16::take(cursor)?;
        reserved::<u16>(cursor)?;
        let anchor_parent_rect = ContentPixelRect::take(cursor)?;
        reserved::<u32>(cursor)?;
        Ok(Self {
            allocation,
            scale_generation,
            role,
            edge,
            margins,
            reservation_extent,
            parent_surface_index,
            anchor_parent_rect,
        })
    }
}

impl Wire for ContentPlacement {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.resource.put(bytes);
        self.surface_index.put(bytes);
        0u16.put(bytes);
        self.destination_x_px.put(bytes);
        self.destination_y_px.put(bytes);
        0u32.put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let resource = ContentResourceId::take(cursor)?;
        let surface_index = u16::take(cursor)?;
        reserved::<u16>(cursor)?;
        let destination_x_px = i32::take(cursor)?;
        let destination_y_px = i32::take(cursor)?;
        reserved::<u32>(cursor)?;
        Ok(Self {
            resource,
            surface_index,
            destination_x_px,
            destination_y_px,
        })
    }
}

impl Wire for ContentTarget {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.surface_index.put(bytes);
        self.action_kind.put(bytes);
        self.target_id.put(bytes);
        self.target_generation.put(bytes);
        self.action_id.put(bytes);
        self.bounds_px.put(bytes);
        0u32.put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let surface_index = u16::take(cursor)?;
        let action_kind = u16::take(cursor)?;
        let target_id = u64::take(cursor)?;
        let target_generation = u64::take(cursor)?;
        let action_id = u64::take(cursor)?;
        let bounds_px = ContentPixelRect::take(cursor)?;
        reserved::<u32>(cursor)?;
        Ok(Self {
            surface_index,
            action_kind,
            target_id,
            target_generation,
            action_id,
            bounds_px,
        })
    }
}
