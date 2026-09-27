//! Wire-neutral typed content records: the r5 content vocabulary payloads.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentAdmissionRefused {
    pub reason: u16,
    pub denied_capabilities: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentAllocationRequest {
    pub grant: ContentGrant,
    pub output: ContentOutputId,
    pub allocation_request_id: u64,
    pub operation: u16,
    pub role: u16,
    pub edge: u16,
    pub prior: ContentAllocationId,
    pub parent: ContentAllocationId,
    pub parent_presentation_epoch: u64,
    pub anchor_parent_rect: ContentPixelRect,
    pub desired_width: u32,
    pub desired_height: u32,
    pub margins: ContentMargins,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentAllocationResult {
    pub grant: ContentGrant,
    pub allocation_request_id: u64,
    pub status: u16,
    pub reason: u16,
    pub output: ContentOutputId,
    pub allocation: ContentAllocationId,
    pub parent: ContentAllocationId,
    pub scale_generation: u64,
    pub logical: ContentLogicalRect,
    pub pixel: ContentPixelRect,
    pub scale_numerator: u32,
    pub scale_denominator: u32,
    pub allowed_reservation_extent: u32,
    pub margins: ContentMargins,
    pub acknowledged_anchor: ContentPixelRect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentResourceBegin {
    pub grant: ContentGrant,
    pub resource: ContentResourceId,
    pub width_px: u32,
    pub height_px: u32,
    pub rendered_scale_numerator: u32,
    pub rendered_scale_denominator: u32,
    pub pixel_format: u16,
    pub chunk_count: u32,
    pub total_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentResourceStatus {
    pub grant: ContentGrant,
    pub resource: ContentResourceId,
    pub status: u16,
    pub reason: u16,
    pub next_ordinal: u32,
    pub admitted_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentResourceEnd {
    pub grant: ContentGrant,
    pub resource: ContentResourceId,
    pub total_bytes: u64,
    pub chunk_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentResourceCancel {
    pub grant: ContentGrant,
    pub resource: ContentResourceId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentResourceRetire {
    pub grant: ContentGrant,
    pub resource: ContentResourceId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentResourceReleased {
    pub grant: ContentGrant,
    pub resource: ContentResourceId,
    pub reason: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentCandidateBegin {
    pub grant: ContentGrant,
    pub candidate_generation: u64,
    pub output: ContentOutputId,
    pub facts_generation: u64,
    pub pacing_permit: u64,
    pub interaction_generation: u64,
    pub surface_count: u32,
    pub placement_count: u32,
    pub target_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentCandidateEnd {
    pub grant: ContentGrant,
    pub candidate_generation: u64,
    pub surface_count: u32,
    pub placement_count: u32,
    pub target_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentCandidateOutcome {
    pub grant: ContentGrant,
    pub candidate_generation: u64,
    pub output: ContentOutputId,
    pub kind: u16,
    pub reason: u16,
    pub presentation_epoch: u64,
    pub work_area_generation: u64,
    pub wm_commit_generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentFrameDemand {
    pub grant: ContentGrant,
    pub output: ContentOutputId,
    pub allocation: ContentAllocationId,
    pub demand_id: u64,
    pub reason: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentFramePermit {
    pub grant: ContentGrant,
    pub output: ContentOutputId,
    pub demand_id: u64,
    pub permit_id: u64,
    pub state: u16,
    pub reason: u16,
    pub ttl_ms: u32,
    pub max_candidate_bytes: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentFrameDemandCancel {
    pub grant: ContentGrant,
    pub output: ContentOutputId,
    pub demand_id: u64,
    pub permit_id: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentAction {
    pub grant: ContentGrant,
    pub output: ContentOutputId,
    pub candidate_generation: u64,
    pub presentation_epoch: u64,
    pub interaction_generation: u64,
    pub allocation: ContentAllocationId,
    pub target_id: u64,
    pub target_generation: u64,
    pub action_id: u64,
    pub event_id: u64,
    pub kind: u16,
    pub reason: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentActionAck {
    pub grant: ContentGrant,
    pub output: ContentOutputId,
    pub candidate_generation: u64,
    pub presentation_epoch: u64,
    pub interaction_generation: u64,
    pub allocation: ContentAllocationId,
    pub target_id: u64,
    pub target_generation: u64,
    pub action_id: u64,
    pub event_id: u64,
    pub disposition: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentOutputFactsEntry {
    pub output: ContentOutputId,
    pub local_width: u32,
    pub local_height: u32,
    pub scale_numerator: u32,
    pub scale_denominator: u32,
    pub scale_generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentSurface {
    pub allocation: ContentAllocationId,
    pub scale_generation: u64,
    pub role: u16,
    pub edge: u16,
    pub margins: ContentMargins,
    pub reservation_extent: u32,
    pub parent_surface_index: u16,
    pub anchor_parent_rect: ContentPixelRect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentPlacement {
    pub resource: ContentResourceId,
    pub surface_index: u16,
    pub destination_x_px: i32,
    pub destination_y_px: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentTarget {
    pub surface_index: u16,
    pub action_kind: u16,
    pub target_id: u64,
    pub target_generation: u64,
    pub action_id: u64,
    pub bounds_px: ContentPixelRect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentOutputFacts {
    pub grant: ContentGrant,
    pub facts_generation: u64,
    pub outputs: Vec<ContentOutputFactsEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentResourceChunk {
    pub grant: ContentGrant,
    pub resource: ContentResourceId,
    pub ordinal: u32,
    pub offset: u64,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentCandidateChunk {
    pub grant: ContentGrant,
    pub candidate_generation: u64,
    pub chunk_ordinal: u32,
    pub surfaces: Vec<ContentSurface>,
    pub placements: Vec<ContentPlacement>,
    pub targets: Vec<ContentTarget>,
}

/// A complete content record. The frame codec validates shape; the owning
/// session validates grant, permissions, references and lifecycle state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShellContentRecord {
    AdmissionRefused(ContentAdmissionRefused),
    Limits(ContentLimits),
    OutputFacts(ContentOutputFacts),
    AllocationRequest(ContentAllocationRequest),
    AllocationResult(ContentAllocationResult),
    ResourceBegin(ContentResourceBegin),
    ResourceStatus(ContentResourceStatus),
    ResourceChunk(ContentResourceChunk),
    ResourceEnd(ContentResourceEnd),
    ResourceCancel(ContentResourceCancel),
    ResourceRetire(ContentResourceRetire),
    ResourceReleased(ContentResourceReleased),
    CandidateBegin(ContentCandidateBegin),
    CandidateChunk(ContentCandidateChunk),
    CandidateEnd(ContentCandidateEnd),
    CandidateOutcome(ContentCandidateOutcome),
    FrameDemand(ContentFrameDemand),
    FramePermit(ContentFramePermit),
    FrameDemandCancel(ContentFrameDemandCancel),
    Action(ContentAction),
    ActionAck(ContentActionAck),
}

/// One complete candidate as a whole value: what a content client proposes
/// for one output, with every row. Transports that carry it in parts
/// reassemble it; the candidate owner receives it as Begin, Chunk and End.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentCandidate {
    pub grant: ContentGrant,
    pub candidate_generation: u64,
    pub output: ContentOutputId,
    pub facts_generation: u64,
    pub pacing_permit: u64,
    pub interaction_generation: u64,
    pub surfaces: Vec<ContentSurface>,
    pub placements: Vec<ContentPlacement>,
    pub targets: Vec<ContentTarget>,
}

impl ContentCandidate {
    /// The candidate as the owner's Begin, single Chunk (ordinal 0) and End.
    pub fn parts(&self) -> [ShellContentRecord; 3] {
        let (surfaces, placements, targets) = (
            self.surfaces.len() as u32,
            self.placements.len() as u32,
            self.targets.len() as u32,
        );
        [
            ShellContentRecord::CandidateBegin(ContentCandidateBegin {
                grant: self.grant,
                candidate_generation: self.candidate_generation,
                output: self.output,
                facts_generation: self.facts_generation,
                pacing_permit: self.pacing_permit,
                interaction_generation: self.interaction_generation,
                surface_count: surfaces,
                placement_count: placements,
                target_count: targets,
            }),
            ShellContentRecord::CandidateChunk(ContentCandidateChunk {
                grant: self.grant,
                candidate_generation: self.candidate_generation,
                chunk_ordinal: 0,
                surfaces: self.surfaces.clone(),
                placements: self.placements.clone(),
                targets: self.targets.clone(),
            }),
            ShellContentRecord::CandidateEnd(ContentCandidateEnd {
                grant: self.grant,
                candidate_generation: self.candidate_generation,
                surface_count: surfaces,
                placement_count: placements,
                target_count: targets,
            }),
        ]
    }
}
