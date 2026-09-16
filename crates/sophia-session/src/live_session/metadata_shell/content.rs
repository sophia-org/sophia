use std::collections::BTreeMap;
use std::collections::BTreeSet;

use sophia_backend_live::LiveShellContentFrame;
use sophia_engine::{CompositorContentImage, CompositorNodeId, HeadlessOutput};
use sophia_protocol::{ContentOutputId, OutputId, Rect, Size};
use sophia_runtime::{ContentAllocationSnapshot, ContentRenderBundle};
use sophia_runtime::{ShellSessionTransport, ShellTransportError};

#[path = "content/actions.rs"]
pub(in crate::live_session) mod actions;
use actions::ContentActionLedger;

const DRM_FORMAT_ARGB8888: u32 = u32::from_le_bytes(*b"AR24");

#[derive(Clone, Copy)]
enum ContentServiceStage {
    Idle,
    Resources,
    Outputs,
    Allocations,
    Demands,
    Candidates,
    Submission,
    Projection,
    Runtime,
    Prepared,
}

impl ContentServiceStage {
    const fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Resources => "resources",
            Self::Outputs => "outputs",
            Self::Allocations => "allocations",
            Self::Demands => "demands",
            Self::Candidates => "candidates",
            Self::Submission => "submission",
            Self::Projection => "projection",
            Self::Runtime => "runtime",
            Self::Prepared => "prepared",
        }
    }
}

#[derive(Clone)]
struct PendingPresentation {
    grant: sophia_protocol::ContentGrant,
    output: ContentOutputId,
    candidate_generation: u64,
    bands: Vec<sophia_protocol::OutputReservation>,
    allocations: Vec<sophia_protocol::ContentAllocationId>,
}

#[derive(Clone, Debug, Default)]
struct PresentedOutputContent {
    grant: sophia_protocol::ContentGrant,
    candidate_generation: u64,
    presentation_epoch: u64,
    bands: Vec<sophia_protocol::OutputReservation>,
    allocations: Vec<(sophia_protocol::ContentAllocationId, u64)>,
}

pub(super) struct LiveContentSession {
    requested: bool,
    input_requested: bool,
    panel_limit: Option<u16>,
    facts_generation: u64,
    published_facts: Vec<sophia_protocol::ContentOutputFactsEntry>,
    next_allocation_id: u64,
    next_permit_id: u64,
    pending: Vec<PendingPresentation>,
    presented: BTreeMap<ContentOutputId, PresentedOutputContent>,
    actions: ContentActionLedger,
    started: std::time::Instant,
    stage: ContentServiceStage,
}

impl LiveContentSession {
    pub(super) fn new(requested: bool, input_requested: bool, panel_limit: Option<u16>) -> Self {
        Self {
            requested,
            input_requested,
            panel_limit,
            facts_generation: 0,
            published_facts: Vec::new(),
            next_allocation_id: 1,
            next_permit_id: 1,
            pending: Vec::new(),
            presented: BTreeMap::new(),
            actions: ContentActionLedger::default(),
            started: std::time::Instant::now(),
            stage: ContentServiceStage::Idle,
        }
    }

    pub(super) fn admission_policy(&self) -> sophia_runtime::ShellContentAdmissionPolicy {
        match self.requested {
            false => sophia_runtime::ShellContentAdmissionPolicy::Denied,
            true => sophia_runtime::ShellContentAdmissionPolicy::Granted {
                discrete_input: self.input_requested,
            },
        }
    }

    pub(super) fn reset_connection(&mut self) {
        self.facts_generation = 0;
        self.published_facts.clear();
        self.pending.clear();
        self.actions.reset();
    }

    fn now_msec(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    pub(super) fn work_area_bands(&self) -> Vec<sophia_protocol::OutputReservation> {
        self.presented
            .values()
            .flat_map(|presented| presented.bands.iter().cloned())
            .collect()
    }

    pub(super) const fn owns_work_area(&self) -> bool {
        self.requested
    }

    pub(super) const fn service_stage(&self) -> &'static str {
        self.stage.name()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn service(
        &mut self,
        transport: &mut ShellSessionTransport,
        runtime: &mut sophia_backend_live::LiveProductionVisualRuntime,
        scene: &sophia_backend_live::LiveProductionCpuScene,
        native_scanout: Option<&mut sophia_backend_live::LiveProductionNativeScanout>,
        outputs: &[HeadlessOutput],
        output_bounds: &[(OutputId, Rect)],
        root: Rect,
        transaction: &mut dyn FnMut() -> Result<
            sophia_protocol::TransactionId,
            Box<dyn std::error::Error>,
        >,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if transport.content_grant().is_none() {
            return Ok(());
        }
        let now = self.now_msec();
        let presented_allocations = self
            .presented
            .values()
            .flat_map(|presented| presented.allocations.iter().copied())
            .collect::<Vec<_>>();
        self.stage = ContentServiceStage::Resources;
        transport.service_content_resources(now)?;
        self.stage = ContentServiceStage::Outputs;
        self.publish_outputs(transport, outputs, transaction)?;
        self.stage = ContentServiceStage::Allocations;
        transport.service_content_allocation_requests(&presented_allocations, now)?;
        while let Some((_, request)) = transport.next_content_allocation_request() {
            if request.operation == 3 {
                transport.release_content_allocation(request.allocation_request_id)?;
                continue;
            }
            let active_allocations = transport.content_allocation_snapshots();
            match self.resolve_allocation(&request, outputs, &active_allocations) {
                Ok(snapshot) => transport.grant_content_allocation(
                    request.allocation_request_id,
                    snapshot,
                    &presented_allocations,
                )?,
                Err(error) => {
                    transport.reject_content_allocation(request.allocation_request_id, error)?
                }
            }
        }
        let allocations = transport.content_allocation_snapshots();
        let content_outputs = self
            .published_facts
            .iter()
            .map(|facts| facts.output)
            .collect::<Vec<_>>();
        self.stage = ContentServiceStage::Demands;
        transport.service_content_demands(&content_outputs, &allocations)?;
        while let Some((_, demand)) = transport.next_content_demand() {
            let permit = self.next_permit_id;
            self.next_permit_id = permit
                .checked_add(1)
                .ok_or("shell content permit identity exhausted")?;
            transport.grant_content_demand(transaction()?, demand.output, permit, now)?;
        }
        let contexts = content_outputs
            .iter()
            .map(|output| sophia_runtime::ContentCandidateContext {
                output: *output,
                facts_generation: self.facts_generation,
                interaction_generation: 1,
                allocations: &allocations,
            })
            .collect::<Vec<_>>();
        self.stage = ContentServiceStage::Candidates;
        transport.service_content_candidates(&contexts, now)?;
        let mut native_scanout = native_scanout;
        while let Some((output, generation)) = transport.next_content_submission_for(|output| {
            !self.pending.iter().any(|pending| pending.output == output)
        }) {
            self.stage = ContentServiceStage::Submission;
            let bundle = transport.begin_content_submission(output, generation, now)?;
            let descriptor = outputs
                .iter()
                .find(|descriptor| descriptor.id.raw() == output.id)
                .copied()
                .ok_or("content candidate targets a removed output")?;
            self.stage = ContentServiceStage::Projection;
            let frame = project_render_bundle(&bundle, descriptor, output, &allocations)?;
            let bands = candidate_bands(&bundle, &allocations, output_bounds, root)?;
            self.stage = ContentServiceStage::Runtime;
            runtime.set_shell_content(frame, scene, native_scanout.as_deref_mut())?;
            let grant = transport.content_grant().ok_or("content grant vanished")?;
            self.stage = ContentServiceStage::Prepared;
            transport.content_prepared(grant, output, generation, 1, 1, now)?;
            let usage = transport.content_usage().unwrap_or_default();
            crate::session_println!(
                "sophia_live_shell_content schema=1 status=prepared output={} candidate_generation={} staging_bytes={} resident_bytes={} retiring_bytes={} backing_bytes={}",
                output.id,
                generation,
                usage.staging,
                usage.resident,
                usage.retiring,
                usage.backing,
            );
            let candidate_allocations = bundle
                .surfaces
                .iter()
                .map(|surface| surface.allocation)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            self.pending.push(PendingPresentation {
                grant,
                output,
                candidate_generation: generation,
                bands,
                allocations: candidate_allocations,
            });
        }
        self.stage = ContentServiceStage::Idle;
        Ok(())
    }

    pub(super) fn observe_presentation(
        &mut self,
        transport: &mut ShellSessionTransport,
        runtime: &sophia_backend_live::LiveProductionVisualRuntime,
    ) -> Result<bool, ShellTransportError> {
        let Some((index, epoch)) = self
            .pending
            .iter()
            .enumerate()
            .find_map(|(index, pending)| {
                runtime
                    .shell_content_presentation_epoch(
                        OutputId::from_raw(pending.output.id),
                        pending.candidate_generation,
                    )
                    .map(|epoch| (index, epoch))
            })
        else {
            return Ok(false);
        };
        let pending = &self.pending[index];
        let publication = transport.content_presented(
            pending.grant,
            pending.output,
            pending.candidate_generation,
            epoch,
            1,
            1,
        );
        if publication == Err(ShellTransportError::ContentQueueSaturated) {
            return Ok(false);
        }
        publication?;
        let pending = self.pending.remove(index);
        let usage = transport.content_usage().unwrap_or_default();
        crate::session_println!(
            "sophia_live_shell_content schema=1 status=presented output={} candidate_generation={} presentation_epoch={} staging_bytes={} resident_bytes={} retiring_bytes={} backing_bytes={}",
            pending.output.id,
            pending.candidate_generation,
            epoch,
            usage.staging,
            usage.resident,
            usage.retiring,
            usage.backing,
        );
        self.presented.insert(
            pending.output,
            PresentedOutputContent {
                grant: pending.grant,
                candidate_generation: pending.candidate_generation,
                presentation_epoch: epoch,
                bands: pending.bands,
                allocations: pending
                    .allocations
                    .into_iter()
                    .map(|allocation| (allocation, epoch))
                    .collect(),
            },
        );
        Ok(true)
    }

    fn publish_outputs(
        &mut self,
        transport: &mut ShellSessionTransport,
        outputs: &[HeadlessOutput],
        transaction: &mut dyn FnMut() -> Result<
            sophia_protocol::TransactionId,
            Box<dyn std::error::Error>,
        >,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let facts = outputs
            .iter()
            .copied()
            .map(output_facts_entry)
            .collect::<Result<Vec<_>, _>>()?;
        if facts == self.published_facts {
            return Ok(());
        }
        self.facts_generation = self
            .facts_generation
            .checked_add(1)
            .ok_or("shell content facts generation exhausted")?;
        transport.publish_content_output_facts(
            transaction()?,
            self.facts_generation,
            facts.clone(),
        )?;
        crate::session_println!(
            "sophia_live_shell_content schema=1 status=outputs facts_generation={} outputs={}",
            self.facts_generation,
            facts.len(),
        );
        self.published_facts = facts;
        Ok(())
    }

    fn resolve_allocation(
        &mut self,
        request: &sophia_protocol::ContentAllocationRequest,
        outputs: &[HeadlessOutput],
        active: &[ContentAllocationSnapshot],
    ) -> Result<ContentAllocationSnapshot, sophia_runtime::ContentAllocationError> {
        let output = outputs
            .iter()
            .find(|output| output.id.raw() == request.output.id)
            .copied()
            .ok_or(sophia_runtime::ContentAllocationError::OutputLost)?;
        if !matches!(request.role, 1 | 2) {
            return Err(sophia_runtime::ContentAllocationError::Malformed);
        }
        let logical_output = Size {
            width: output.size.width / i32::try_from(output.scale.max(1)).unwrap_or(i32::MAX),
            height: output.size.height / i32::try_from(output.scale.max(1)).unwrap_or(i32::MAX),
        };
        let width = i32::try_from(request.desired_width)
            .map_err(|_| sophia_runtime::ContentAllocationError::Malformed)?;
        let height = i32::try_from(request.desired_height)
            .map_err(|_| sophia_runtime::ContentAllocationError::Malformed)?;
        let (logical, pixel) = if request.role == 1 {
            let logical = panel_rect(request, logical_output, width, height)?;
            let pixel = quantize(logical, output.scale)
                .ok_or(sophia_runtime::ContentAllocationError::Malformed)?;
            (logical, pixel)
        } else {
            let parent = active
                .iter()
                .find(|allocation| allocation.allocation == request.parent)
                .filter(|allocation| allocation.output == request.output && allocation.role == 1)
                .ok_or(sophia_runtime::ContentAllocationError::AllocationLost)?;
            popout_rect(request, output, parent)?
        };
        let allowed_reservation_extent = if request.role == 1 {
            let thickness = panel_pixel_thickness(pixel, request.edge);
            if self
                .panel_limit
                .is_none_or(|limit| thickness > u32::from(limit))
            {
                return Err(sophia_runtime::ContentAllocationError::Budget);
            }
            thickness.min(u32::from(self.panel_limit.unwrap_or(0)))
        } else {
            0
        };
        let allocation = if request.operation == 1 {
            let id = self.next_allocation_id;
            self.next_allocation_id = id
                .checked_add(1)
                .ok_or(sophia_runtime::ContentAllocationError::Budget)?;
            sophia_protocol::ContentAllocationId { id, generation: 1 }
        } else {
            sophia_protocol::ContentAllocationId {
                id: request.prior.id,
                generation: request.prior.generation.saturating_add(1),
            }
        };
        Ok(ContentAllocationSnapshot {
            output: request.output,
            allocation,
            scale_generation: 1,
            scale_numerator: output.scale,
            scale_denominator: 1,
            role: request.role,
            edge: request.edge,
            margins: request.margins,
            logical,
            pixel,
            parent: request.parent,
            anchor_parent_rect: request.anchor_parent_rect,
            allowed_reservation_extent,
        })
    }
}

fn output_facts_entry(
    output: HeadlessOutput,
) -> Result<sophia_protocol::ContentOutputFactsEntry, &'static str> {
    let scale = i32::try_from(output.scale).map_err(|_| "content output scale is too large")?;
    if scale == 0
        || output.size.width <= 0
        || output.size.height <= 0
        || output.size.width % scale != 0
        || output.size.height % scale != 0
    {
        return Err("content output has no exact integer logical extent");
    }
    Ok(sophia_protocol::ContentOutputFactsEntry {
        output: ContentOutputId {
            id: output.id.raw(),
            generation: 1,
        },
        local_width: u32::try_from(output.size.width / scale)
            .map_err(|_| "content output width is not representable")?,
        local_height: u32::try_from(output.size.height / scale)
            .map_err(|_| "content output height is not representable")?,
        scale_numerator: output.scale,
        scale_denominator: 1,
        scale_generation: 1,
    })
}

fn panel_rect(
    request: &sophia_protocol::ContentAllocationRequest,
    logical_output: Size,
    width: i32,
    height: i32,
) -> Result<sophia_protocol::ContentLogicalRect, sophia_runtime::ContentAllocationError> {
    let x = match request.edge {
        2 => logical_output
            .width
            .checked_sub(width)
            .and_then(|value| value.checked_sub(i32::from(request.margins.right))),
        _ => Some(i32::from(request.margins.left)),
    }
    .ok_or(sophia_runtime::ContentAllocationError::Malformed)?;
    let y = match request.edge {
        3 => logical_output
            .height
            .checked_sub(height)
            .and_then(|value| value.checked_sub(i32::from(request.margins.bottom))),
        _ => Some(i32::from(request.margins.top)),
    }
    .ok_or(sophia_runtime::ContentAllocationError::Malformed)?;
    Ok(sophia_protocol::ContentLogicalRect {
        x,
        y,
        width: request.desired_width,
        height: request.desired_height,
    })
}

/// Places a popout wholly inside its parent output. The request edge names the
/// panel edge, so it supplies the first tie-break (away from the panel); the
/// actual side is carried by the resolved rectangle and cannot be changed by
/// the client after acknowledgement.
fn popout_rect(
    request: &sophia_protocol::ContentAllocationRequest,
    output: HeadlessOutput,
    parent: &ContentAllocationSnapshot,
) -> Result<
    (
        sophia_protocol::ContentLogicalRect,
        sophia_protocol::ContentPixelRect,
    ),
    sophia_runtime::ContentAllocationError,
> {
    let scale = i32::try_from(output.scale.max(1))
        .map_err(|_| sophia_runtime::ContentAllocationError::Malformed)?;
    let width_px = i32::try_from(request.desired_width)
        .ok()
        .and_then(|value| value.checked_mul(scale))
        .ok_or(sophia_runtime::ContentAllocationError::Malformed)?;
    let height_px = i32::try_from(request.desired_height)
        .ok()
        .and_then(|value| value.checked_mul(scale))
        .ok_or(sophia_runtime::ContentAllocationError::Malformed)?;
    let margin = |value: i16| i32::from(value).checked_mul(scale);
    let [top, right, bottom, left] = [
        margin(request.margins.top),
        margin(request.margins.right),
        margin(request.margins.bottom),
        margin(request.margins.left),
    ]
    .map(|value| value.ok_or(sophia_runtime::ContentAllocationError::Malformed));
    let (top, right, bottom, left) = (top?, right?, bottom?, left?);
    let anchor = request.anchor_parent_rect;
    if anchor.width == 0
        || anchor.height == 0
        || anchor.x < 0
        || anchor.y < 0
        || anchor
            .x
            .saturating_add(i32::try_from(anchor.width).unwrap_or(i32::MAX))
            > i32::try_from(parent.pixel.width).unwrap_or(i32::MAX)
        || anchor
            .y
            .saturating_add(i32::try_from(anchor.height).unwrap_or(i32::MAX))
            > i32::try_from(parent.pixel.height).unwrap_or(i32::MAX)
    {
        return Err(sophia_runtime::ContentAllocationError::Malformed);
    }
    let anchor = Rect {
        x: parent.pixel.x.saturating_add(anchor.x),
        y: parent.pixel.y.saturating_add(anchor.y),
        width: i32::try_from(anchor.width)
            .map_err(|_| sophia_runtime::ContentAllocationError::Malformed)?,
        height: i32::try_from(anchor.height)
            .map_err(|_| sophia_runtime::ContentAllocationError::Malformed)?,
    };
    let output_size = output.size;
    let side_room = [
        (1_u16, anchor.y.saturating_sub(bottom)),
        (
            2,
            output_size
                .width
                .saturating_sub(anchor.x.saturating_add(anchor.width))
                .saturating_sub(left),
        ),
        (
            3,
            output_size
                .height
                .saturating_sub(anchor.y.saturating_add(anchor.height))
                .saturating_sub(top),
        ),
        (4, anchor.x.saturating_sub(right)),
    ];
    let away = match request.edge {
        1 => 3,
        2 => 4,
        3 => 1,
        4 => 2,
        _ => return Err(sophia_runtime::ContentAllocationError::Malformed),
    };
    let rank = |side| {
        if side == away {
            0
        } else {
            match side {
                1 => 1,
                4 => 2,
                3 => 3,
                2 => 4,
                _ => 5,
            }
        }
    };
    let mut sides = side_room;
    sides.sort_by_key(|(side, room)| (std::cmp::Reverse(*room), rank(*side)));
    for (side, room) in sides {
        let required = if matches!(side, 1 | 3) {
            height_px
        } else {
            width_px
        };
        if room < required {
            continue;
        }
        let x = match side {
            2 => anchor.x.saturating_add(anchor.width).saturating_add(left),
            4 => anchor.x.saturating_sub(right).saturating_sub(width_px),
            _ => anchor.x.saturating_add(left),
        };
        let y = match side {
            1 => anchor.y.saturating_sub(bottom).saturating_sub(height_px),
            3 => anchor.y.saturating_add(anchor.height).saturating_add(top),
            _ => anchor.y.saturating_add(top),
        };
        let rect = Rect {
            x,
            y,
            width: width_px,
            height: height_px,
        };
        if rect.x >= 0
            && rect.y >= 0
            && rect.x.saturating_add(rect.width) <= output_size.width
            && rect.y.saturating_add(rect.height) <= output_size.height
        {
            return Ok((
                sophia_protocol::ContentLogicalRect {
                    x: rect.x.div_euclid(scale),
                    y: rect.y.div_euclid(scale),
                    width: request.desired_width,
                    height: request.desired_height,
                },
                sophia_protocol::ContentPixelRect {
                    x: rect.x,
                    y: rect.y,
                    width: u32::try_from(rect.width)
                        .map_err(|_| sophia_runtime::ContentAllocationError::Malformed)?,
                    height: u32::try_from(rect.height)
                        .map_err(|_| sophia_runtime::ContentAllocationError::Malformed)?,
                },
            ));
        }
    }
    Err(sophia_runtime::ContentAllocationError::Budget)
}

fn panel_pixel_thickness(pixel: sophia_protocol::ContentPixelRect, edge: u16) -> u32 {
    if matches!(edge, 1 | 3) {
        pixel.height
    } else {
        pixel.width
    }
}

fn quantize(
    logical: sophia_protocol::ContentLogicalRect,
    scale: u32,
) -> Option<sophia_protocol::ContentPixelRect> {
    Some(sophia_protocol::ContentPixelRect {
        x: logical.x.checked_mul(i32::try_from(scale).ok()?)?,
        y: logical.y.checked_mul(i32::try_from(scale).ok()?)?,
        width: logical.width.checked_mul(scale)?,
        height: logical.height.checked_mul(scale)?,
    })
}

fn candidate_bands(
    bundle: &ContentRenderBundle,
    allocations: &[ContentAllocationSnapshot],
    output_bounds: &[(OutputId, Rect)],
    root: Rect,
) -> Result<Vec<sophia_protocol::OutputReservation>, &'static str> {
    let mut bands = Vec::new();
    for surface in &bundle.surfaces {
        if surface.reservation_extent == 0 {
            continue;
        }
        let allocation = allocations
            .iter()
            .find(|allocation| allocation.allocation == surface.allocation)
            .ok_or("content reservation lost its allocation")?;
        let bounds = output_bounds
            .iter()
            .find_map(|(output, bounds)| {
                (*output == OutputId::from_raw(allocation.output.id)).then_some(*bounds)
            })
            .ok_or("content reservation lost its output")?;
        let edge = match surface.edge {
            1 => sophia_protocol::ShellV1ReservationEdge::Top,
            2 => sophia_protocol::ShellV1ReservationEdge::Right,
            3 => sophia_protocol::ShellV1ReservationEdge::Bottom,
            4 => sophia_protocol::ShellV1ReservationEdge::Left,
            _ => return Err("content reservation has an invalid edge"),
        };
        let thickness_px = u16::try_from(surface.reservation_extent)
            .map_err(|_| "content reservation exceeds protocol range")?;
        let band = sophia_engine::shell_reservation_band(
            sophia_protocol::ShellV1WorkAreaReservation { edge, thickness_px },
            root,
            bounds,
        )
        .ok_or("content reservation cannot fit its output")?;
        bands.push(band);
    }
    Ok(bands)
}

/// Converts one validated content bundle into the immutable renderer record.
/// Candidate validation owns references and table shape; this seam owns the
/// exact allocation-local physical placement and retains each resource lease.
pub(super) fn project_render_bundle(
    bundle: &ContentRenderBundle,
    output: HeadlessOutput,
    output_identity: ContentOutputId,
    allocations: &[ContentAllocationSnapshot],
) -> Result<LiveShellContentFrame, &'static str> {
    if bundle.output != output_identity
        || bundle.candidate_generation == 0
        || output.id.raw() != output_identity.id
        || output.size.width <= 0
        || output.size.height <= 0
    {
        return Err("content bundle targets stale output facts");
    }
    let mut nodes = BTreeSet::new();
    let mut images = Vec::with_capacity(bundle.placements.len());
    for (placement_index, placement) in bundle.placements.iter().enumerate() {
        let surface_index = usize::from(placement.surface_index);
        let surface = bundle
            .surfaces
            .get(surface_index)
            .ok_or("content placement names an absent surface")?;
        let allocation = allocations
            .iter()
            .find(|allocation| allocation.allocation == surface.allocation)
            .ok_or("content placement names a lost allocation")?;
        if allocation.output != output_identity {
            return Err("content placement crosses outputs");
        }
        let resource = bundle
            .resource(placement.resource)
            .ok_or("content placement names an absent resource")?
            .clone();
        let description = resource.description();
        let width = i32::try_from(description.width_px)
            .map_err(|_| "content width exceeds renderer geometry")?;
        let height = i32::try_from(description.height_px)
            .map_err(|_| "content height exceeds renderer geometry")?;
        let geometry_px = Rect {
            x: allocation
                .pixel
                .x
                .checked_add(placement.destination_x_px)
                .ok_or("content placement x overflow")?,
            y: allocation
                .pixel
                .y
                .checked_add(placement.destination_y_px)
                .ok_or("content placement y overflow")?,
            width,
            height,
        };
        let allocation_width = i32::try_from(allocation.pixel.width)
            .map_err(|_| "content allocation width exceeds renderer geometry")?;
        let allocation_height = i32::try_from(allocation.pixel.height)
            .map_err(|_| "content allocation height exceeds renderer geometry")?;
        if geometry_px.x < allocation.pixel.x
            || geometry_px.y < allocation.pixel.y
            || geometry_px.x.saturating_add(width)
                > allocation.pixel.x.saturating_add(allocation_width)
            || geometry_px.y.saturating_add(height)
                > allocation.pixel.y.saturating_add(allocation_height)
        {
            return Err("content placement escapes its allocation");
        }
        let node = CompositorNodeId::ShellContent {
            output: output.id,
            candidate: bundle.candidate_generation,
            surface: placement.surface_index,
            placement: u16::try_from(placement_index)
                .map_err(|_| "content placement identity exceeds renderer bound")?,
        };
        if !nodes.insert(node) {
            return Err("content placement repeats a renderer node");
        }
        images.push(CompositorContentImage {
            node,
            generation: placement.resource.generation,
            output_size_px: output.size,
            geometry_px,
            size_px: Size { width, height },
            stride: description
                .width_px
                .checked_mul(4)
                .ok_or("content stride overflow")?,
            format: DRM_FORMAT_ARGB8888,
            resource,
        });
    }
    if images.is_empty() {
        return Err("content candidate has no visible placements");
    }
    let mut allocation_rows = Vec::new();
    let mut targets = Vec::with_capacity(bundle.targets.len());
    for target in &bundle.targets {
        let surface = bundle
            .surfaces
            .get(usize::from(target.surface_index))
            .ok_or("content target names an absent surface")?;
        let allocation = allocations
            .iter()
            .find(|candidate| candidate.allocation == surface.allocation)
            .ok_or("content target names a lost allocation")?;
        let row = (allocation.allocation, allocation.logical, allocation.pixel);
        if !allocation_rows.contains(&row) {
            allocation_rows.push(row);
        }
        targets.push(sophia_engine::PresentedContentTarget {
            continuity: None,
            scale_generation: allocation.scale_generation,
            grant: bundle.grant,
            output: bundle.output,
            candidate_generation: bundle.candidate_generation,
            presentation_epoch: 0,
            interaction_generation: bundle.interaction_generation,
            allocation: allocation.allocation,
            allocation_logical: allocation.logical,
            allocation_pixel: allocation.pixel,
            target_id: target.target_id,
            target_generation: target.target_generation,
            action_id: target.action_id,
            bounds_px: target.bounds_px,
        });
    }
    Ok(LiveShellContentFrame {
        output: OutputId::from_raw(output_identity.id),
        content_output: output_identity,
        grant: bundle.grant,
        candidate_generation: bundle.candidate_generation,
        interaction_generation: bundle.interaction_generation,
        images,
        targets,
        allocations: allocation_rows,
    })
}

#[path = "content/tests.rs"]
mod tests;
