use super::*;
use crate::IpcCodecError;

fn require(ok: bool, field: &'static str) -> Result<(), IpcCodecError> {
    if ok {
        Ok(())
    } else {
        Err(IpcCodecError::InvalidRecord(field))
    }
}

fn pair(id: u64, generation: u64, null: bool) -> bool {
    (id > 0 && generation > 0) || (null && id == 0 && generation == 0)
}

pub(super) fn scale(n: u32, d: u32) -> bool {
    if n == 0 || n > 32 || d == 0 || d > 4 {
        return false;
    }
    let (mut a, mut b) = (n, d);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a == 1
}

fn resource(id: ContentResourceId) -> Result<(), IpcCodecError> {
    require(
        pair(id.id, id.generation, false),
        "content resource identity",
    )
}
fn output(id: ContentOutputId) -> Result<(), IpcCodecError> {
    require(pair(id.id, id.generation, false), "content output identity")
}
fn allocation(id: ContentAllocationId, null: bool) -> Result<(), IpcCodecError> {
    require(
        pair(id.id, id.generation, null),
        "content allocation identity",
    )
}
fn margins(m: ContentMargins) -> Result<(), IpcCodecError> {
    require(
        [m.top, m.right, m.bottom, m.left]
            .into_iter()
            .all(|v| (-512..=512).contains(&v)),
        "content margin",
    )
}
fn reason(value: u16) -> Result<(), IpcCodecError> {
    require(value <= 12, "content reason")
}
fn counts(s: u32, p: u32, t: u32) -> Result<(), IpcCodecError> {
    require(s <= 8 && p <= 32 && t <= 64, "content candidate counts")
}
fn pixel_rect(r: ContentPixelRect) -> Result<(), IpcCodecError> {
    require(
        r.width > 0
            && r.height > 0
            && r.x >= 0
            && r.y >= 0
            && i64::from(r.x) + i64::from(r.width) <= i64::from(i32::MAX)
            && i64::from(r.y) + i64::from(r.height) <= i64::from(i32::MAX),
        "content pixel rectangle",
    )
}

pub(crate) fn validate(record: &ShellContentRecord) -> Result<(), IpcCodecError> {
    use ShellContentRecord::*;
    let grant = match record {
        AdmissionRefused(v) => {
            return require(
                (1..=4).contains(&v.reason) && v.denied_capabilities != 0,
                "content refusal",
            );
        }
        Limits(v) => {
            v.validate()?;
            v.grant
        }
        OutputFacts(v) => {
            require(
                v.facts_generation > 0 && v.outputs.len() <= 16,
                "content output facts",
            )?;
            for (index, row) in v.outputs.iter().enumerate() {
                output(row.output)?;
                require(
                    row.local_width > 0
                        && row.local_height > 0
                        && scale(row.scale_numerator, row.scale_denominator)
                        && row.scale_generation > 0
                        && !v.outputs[..index]
                            .iter()
                            .any(|other| other.output.id == row.output.id),
                    "content output facts entry",
                )?;
            }
            v.grant
        }
        AllocationRequest(v) => {
            output(v.output)?;
            allocation(v.prior, v.operation == 1)?;
            allocation(v.parent, v.role == 1 || v.operation == 3)?;
            margins(v.margins)?;
            require(
                v.allocation_request_id > 0
                    && (1..=3).contains(&v.operation)
                    && (1..=2).contains(&v.role)
                    && (1..=4).contains(&v.edge),
                "content allocation request",
            )?;
            if v.operation == 3 {
                require(
                    v.parent == ContentAllocationId::default()
                        && v.parent_presentation_epoch == 0
                        && v.anchor_parent_rect == ContentPixelRect::default()
                        && v.desired_width == 0
                        && v.desired_height == 0
                        && v.margins == ContentMargins::default(),
                    "content release proposal",
                )?;
            } else {
                if v.operation == 1 {
                    require(
                        v.prior == ContentAllocationId::default(),
                        "content acquire prior",
                    )?;
                }
                require(
                    v.desired_width > 0 && v.desired_height > 0,
                    "content desired dimensions",
                )?;
                if v.role == 1 {
                    require(
                        v.parent == ContentAllocationId::default()
                            && v.parent_presentation_epoch == 0
                            && v.anchor_parent_rect == ContentPixelRect::default(),
                        "content panel parent",
                    )?;
                } else {
                    require(
                        v.parent_presentation_epoch > 0,
                        "content parent presentation",
                    )?;
                    pixel_rect(v.anchor_parent_rect)?;
                }
            }
            v.grant
        }
        AllocationResult(v) => {
            output(v.output)?;
            reason(v.reason)?;
            margins(v.margins)?;
            require(
                (1..=4).contains(&v.status) && (v.allocation_request_id == 0) == (v.status == 4),
                "content allocation outcome",
            )?;
            allocation(v.allocation, v.status == 2)?;
            allocation(v.parent, true)?;
            if v.status == 1 {
                require(
                    v.reason == 0
                        && scale(v.scale_numerator, v.scale_denominator)
                        && v.scale_generation > 0
                        && v.pixel.width > 0
                        && v.pixel.height > 0
                        && v.logical.width > 0
                        && v.logical.height > 0
                        && v.allowed_reservation_extent <= 512,
                    "content allocation geometry",
                )?;
            } else if v.status == 2 || v.status == 3 {
                require(
                    v.pixel == ContentPixelRect::default()
                        && v.logical == ContentLogicalRect::default()
                        && v.scale_numerator == 0
                        && v.scale_denominator == 0
                        && v.scale_generation == 0
                        && v.allowed_reservation_extent == 0
                        && v.margins == ContentMargins::default()
                        && v.acknowledged_anchor == ContentPixelRect::default(),
                    "content zero outcome geometry",
                )?;
            }
            v.grant
        }
        ResourceBegin(v) => {
            v.layout(&ContentLimits::prototype(v.grant))?;
            v.grant
        }
        ResourceStatus(v) => {
            resource(v.resource)?;
            reason(v.reason)?;
            require(
                (1..=4).contains(&v.status)
                    && v.admitted_bytes <= 4 * 1024 * 1024
                    && (v.status > 2 || v.reason == 0),
                "content resource status",
            )?;
            v.grant
        }
        ResourceChunk(v) => {
            resource(v.resource)?;
            require(
                !v.bytes.is_empty()
                    && v.bytes.len() <= 65488
                    && v.offset <= 4 * 1024 * 1024
                    && v.offset + v.bytes.len() as u64 <= 4 * 1024 * 1024,
                "content resource chunk",
            )?;
            v.grant
        }
        ResourceEnd(v) => {
            resource(v.resource)?;
            require(
                v.total_bytes > 0
                    && v.total_bytes <= 4 * 1024 * 1024
                    && v.chunk_count > 0
                    && v.chunk_count <= 4096,
                "content resource end",
            )?;
            v.grant
        }
        ResourceCancel(v) => {
            resource(v.resource)?;
            v.grant
        }
        ResourceRetire(v) => {
            resource(v.resource)?;
            v.grant
        }
        ResourceReleased(v) => {
            resource(v.resource)?;
            reason(v.reason)?;
            v.grant
        }
        CandidateBegin(v) => {
            output(v.output)?;
            counts(v.surface_count, v.placement_count, v.target_count)?;
            require(
                v.candidate_generation > 0
                    && v.facts_generation > 0
                    && v.pacing_permit > 0
                    && v.interaction_generation > 0,
                "content candidate begin",
            )?;
            v.grant
        }
        CandidateChunk(v) => {
            validate_candidate_chunk(v, false)?;
            v.grant
        }
        CandidateEnd(v) => {
            require(v.candidate_generation > 0, "content candidate end")?;
            counts(v.surface_count, v.placement_count, v.target_count)?;
            v.grant
        }
        CandidateOutcome(v) => {
            output(v.output)?;
            reason(v.reason)?;
            require(
                v.candidate_generation > 0
                    && (1..=4).contains(&v.kind)
                    && (v.presentation_epoch != 0) == (v.kind == 2)
                    && (v.kind > 2 || v.reason == 0),
                "content candidate outcome",
            )?;
            v.grant
        }
        FrameDemand(v) => {
            output(v.output)?;
            allocation(v.allocation, true)?;
            require(
                v.demand_id > 0 && (1..=3).contains(&v.reason),
                "content frame demand",
            )?;
            v.grant
        }
        FramePermit(v) => {
            output(v.output)?;
            reason(v.reason)?;
            require(
                v.demand_id > 0 && (1..=4).contains(&v.state) && v.max_candidate_bytes <= 8192,
                "content frame permit",
            )?;
            if v.state == 1 {
                require(
                    v.permit_id > 0
                        && v.ttl_ms > 0
                        && v.ttl_ms <= 250
                        && v.max_candidate_bytes > 0
                        && v.reason == 0,
                    "content granted permit",
                )?;
            }
            v.grant
        }
        FrameDemandCancel(v) => {
            output(v.output)?;
            require(v.demand_id > 0, "content cancel demand")?;
            v.grant
        }
        Action(v) => {
            output(v.output)?;
            allocation(v.allocation, false)?;
            reason(v.reason)?;
            require(
                v.candidate_generation > 0
                    && v.presentation_epoch > 0
                    && v.interaction_generation > 0
                    && v.event_id > 0
                    && (1..=3).contains(&v.kind),
                "content action",
            )?;
            if v.kind == 1 {
                require(
                    v.target_id > 0 && v.target_generation > 0 && v.action_id > 0 && v.reason == 0,
                    "content activation",
                )?;
            } else if v.kind == 2 {
                require(
                    v.target_id == 0
                        && v.target_generation == 0
                        && v.action_id == 0
                        && v.reason == 0,
                    "content dismissal",
                )?;
            }
            v.grant
        }
        ActionAck(v) => {
            output(v.output)?;
            allocation(v.allocation, false)?;
            require(
                v.candidate_generation > 0
                    && v.presentation_epoch > 0
                    && v.interaction_generation > 0
                    && v.event_id > 0
                    && (1..=2).contains(&v.disposition),
                "content action acknowledgement",
            )?;
            require(
                (v.target_id > 0 && v.target_generation > 0 && v.action_id > 0)
                    || (v.target_id == 0 && v.target_generation == 0 && v.action_id == 0),
                "content acknowledgement target",
            )?;
            v.grant
        }
    };
    require(
        grant.connection_epoch > 0 && grant.content_grant_epoch > 0,
        "content grant",
    )
}

/// Deterministic row-aligned transfer shape, validated before reserving bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentResourceLayout {
    pub row_bytes: u32,
    pub rows_per_chunk: u32,
    pub chunk_count: u32,
    pub total_bytes: u64,
}

impl ContentResourceBegin {
    pub fn layout(&self, limits: &ContentLimits) -> Result<ContentResourceLayout, IpcCodecError> {
        resource(self.resource)?;
        require(
            self.grant == limits.grant
                && self.width_px > 0
                && self.width_px <= limits.max_width_px
                && self.height_px > 0
                && self.height_px <= limits.max_height_px
                && self.pixel_format == 1
                && self.rendered_scale_numerator <= limits.max_scale_numerator
                && self.rendered_scale_denominator <= limits.max_scale_denominator
                && scale(
                    self.rendered_scale_numerator,
                    self.rendered_scale_denominator,
                ),
            "content resource description",
        )?;
        let row_bytes = self
            .width_px
            .checked_mul(4)
            .ok_or(IpcCodecError::InvalidRecord("content row overflow"))?;
        let total_bytes = u64::from(row_bytes) * u64::from(self.height_px);
        require(
            total_bytes <= limits.max_resource_bytes && self.total_bytes == total_bytes,
            "content resource byte count",
        )?;
        let payload = limits
            .max_frame_payload
            .saturating_sub(48)
            .min(limits.max_chunk_bytes);
        let rows_per_chunk = payload / row_bytes;
        require(rows_per_chunk > 0, "content row does not fit chunk")?;
        let chunk_count = self.height_px.div_ceil(rows_per_chunk);
        require(
            chunk_count == self.chunk_count,
            "content resource chunk count",
        )?;
        Ok(ContentResourceLayout {
            row_bytes,
            rows_per_chunk,
            chunk_count,
            total_bytes,
        })
    }
}

/// The native family opts in explicitly; legacy kind 173 keeps r5 roles/kinds.
pub(crate) fn validate_candidate_chunk(
    v: &ContentCandidateChunk,
    native_launcher: bool,
) -> Result<(), IpcCodecError> {
    validate_candidate_chunk_profile(v, native_launcher, if native_launcher { 2 } else { 1 })
}

pub(crate) fn validate_catalog_candidate_chunk(
    v: &ContentCandidateChunk,
) -> Result<(), IpcCodecError> {
    validate_candidate_chunk_profile(v, false, 3)?;
    require(
        v.surfaces.iter().all(|s| s.role == 1) && v.targets.iter().all(|t| t.action_id <= 4096),
        "persistent catalog rows",
    )
}

fn validate_candidate_chunk_profile(
    v: &ContentCandidateChunk,
    native_launcher: bool,
    action_kind: u16,
) -> Result<(), IpcCodecError> {
    require(v.candidate_generation > 0, "content candidate generation")?;
    require(
        v.surfaces.len() <= 8 && v.placements.len() <= 32 && v.targets.len() <= 64,
        "content chunk counts",
    )?;
    for row in &v.surfaces {
        allocation(row.allocation, false)?;
        margins(row.margins)?;
        require(
            row.scale_generation > 0
                && if native_launcher {
                    row.role == 3
                } else {
                    (1..=2).contains(&row.role)
                }
                && (1..=4).contains(&row.edge)
                && row.reservation_extent <= 512,
            "content surface",
        )?;
        if row.role == 1 || native_launcher {
            require(
                row.parent_surface_index == u16::MAX
                    && row.anchor_parent_rect == ContentPixelRect::default()
                    && (!native_launcher || row.reservation_extent == 0),
                "content panel row",
            )?;
        } else {
            require(
                row.parent_surface_index < 8 && row.reservation_extent == 0,
                "content popout row",
            )?;
            pixel_rect(row.anchor_parent_rect)?;
        }
    }
    for row in &v.placements {
        resource(row.resource)?;
        require(
            row.surface_index < 8 && row.destination_x_px >= 0 && row.destination_y_px >= 0,
            "content placement",
        )?;
    }
    for row in &v.targets {
        require(
            row.surface_index < 8
                && row.action_kind == action_kind
                && row.target_id > 0
                && row.target_generation > 0
                && row.action_id > 0,
            "content target",
        )?;
        pixel_rect(row.bounds_px)?;
    }
    if native_launcher {
        require(
            v.surfaces.len() <= 1 && v.targets.len() <= 32,
            "native launcher chunk counts",
        )?;
        require(
            v.placements.iter().all(|p| p.surface_index == 0)
                && v.targets
                    .iter()
                    .all(|t| t.surface_index == 0 && t.action_id <= 4096),
            "native launcher chunk rows",
        )?;
    }
    require(
        v.grant.connection_epoch > 0 && v.grant.content_grant_epoch > 0,
        "content grant",
    )
}
