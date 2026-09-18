use std::collections::{BTreeMap, VecDeque};

use super::ContentStoreProfile;
use sophia_protocol::*;
mod native;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentAllocationError {
    Stale,
    Budget,
    Malformed,
    Timeout,
    OutputLost,
    AllocationLost,
    ClockRegression,
}

impl ContentAllocationError {
    pub const fn reason(self) -> ContentReason {
        match self {
            Self::Stale => ContentReason::Stale,
            Self::Budget => ContentReason::Budget,
            Self::Malformed | Self::ClockRegression => ContentReason::Malformed,
            Self::Timeout => ContentReason::Timeout,
            Self::OutputLost => ContentReason::OutputLost,
            Self::AllocationLost => ContentReason::AllocationLost,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentAllocationSnapshot {
    /// Exact native opening; absent for legacy panel/popout allocations.
    pub native_opening: Option<u64>,
    pub output: ContentOutputId,
    pub allocation: ContentAllocationId,
    pub scale_generation: u64,
    pub scale_numerator: u32,
    pub scale_denominator: u32,
    pub role: u16,
    pub edge: u16,
    pub margins: ContentMargins,
    pub logical: ContentLogicalRect,
    pub pixel: ContentPixelRect,
    pub parent: ContentAllocationId,
    pub anchor_parent_rect: ContentPixelRect,
    pub allowed_reservation_extent: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentAllocationEvent {
    pub transaction: TransactionId,
    pub record: ShellContentRecord,
}

#[derive(Clone)]
struct PendingAllocation {
    native_opening: Option<u64>,
    transaction: TransactionId,
    request: ContentAllocationRequest,
    deadline: u64,
}

/// Bounded allocation proposals for one content grant.
///
/// Engine remains the placement authority. This reducer owns request identity,
/// freshness, budgets and terminal results; a grant accepts only an exact
/// Engine-resolved snapshot against the currently published output facts.
pub struct ContentAllocationStore {
    profile: ContentStoreProfile,
    limits: ContentLimits,
    facts_generation: u64,
    outputs: Vec<ContentOutputFactsEntry>,
    active: BTreeMap<ContentAllocationId, ContentAllocationSnapshot>,
    pending: BTreeMap<u64, PendingAllocation>,
    events: VecDeque<ContentAllocationEvent>,
    response_credits: usize,
    last_request_id: u64,
    last_allocation_id: u64,
    last_now: u64,
    revoked: bool,
}

impl ContentAllocationStore {
    pub(crate) fn add_accounting(&self, value: &mut super::ContentEpochAccounting) {
        value.allocations += self.active.len() + self.pending.len();
        value.response_records += self.control_occupancy();
    }

    pub(crate) fn queued_bulk_occupancy(&self) -> (usize, usize) {
        self.events
            .iter()
            .filter_map(|event| match &event.record {
                ShellContentRecord::OutputFacts(facts) => {
                    Some(sophia_protocol::SOPHIA_IPC_HEADER_LEN + 32 + 40 * facts.outputs.len())
                }
                _ => None,
            })
            .fold((0, 0), |(records, bytes), frame| {
                (records + 1, bytes + frame)
            })
    }

    pub(crate) fn control_occupancy(&self) -> usize {
        self.events.len() + self.response_credits
    }

    pub fn new(limits: ContentLimits) -> Result<Self, ContentAllocationError> {
        Self::with_profile(limits, ContentStoreProfile::Legacy)
    }

    pub fn with_profile(
        limits: ContentLimits,
        profile: ContentStoreProfile,
    ) -> Result<Self, ContentAllocationError> {
        limits
            .validate()
            .map_err(|_| ContentAllocationError::Malformed)?;
        Ok(Self {
            profile,
            limits,
            facts_generation: 0,
            outputs: Vec::new(),
            active: BTreeMap::new(),
            pending: BTreeMap::new(),
            events: VecDeque::new(),
            response_credits: 0,
            last_request_id: 0,
            last_allocation_id: 0,
            last_now: 0,
            revoked: false,
        })
    }

    pub fn facts_generation(&self) -> u64 {
        self.facts_generation
    }

    pub fn outputs(&self) -> &[ContentOutputFactsEntry] {
        &self.outputs
    }

    pub fn snapshots(&self) -> Vec<ContentAllocationSnapshot> {
        self.active.values().cloned().collect()
    }

    pub fn pending_request(&self) -> Option<(TransactionId, ContentAllocationRequest)> {
        self.pending
            .values()
            .next()
            .map(|pending| (pending.transaction, pending.request.clone()))
    }

    pub fn take_event(&mut self) -> Option<ContentAllocationEvent> {
        self.events.pop_front()
    }

    pub fn pending_event(&self) -> Option<&ContentAllocationEvent> {
        self.events.front()
    }

    pub fn quiescent(&self) -> bool {
        self.pending.is_empty() && self.active.is_empty()
    }

    pub fn publish_outputs(
        &mut self,
        transaction: TransactionId,
        facts_generation: u64,
        outputs: Vec<ContentOutputFactsEntry>,
    ) -> Result<(), ContentAllocationError> {
        self.check_grant(self.limits.grant)?;
        if !transaction.is_valid() || facts_generation <= self.facts_generation {
            return Err(ContentAllocationError::Stale);
        }
        if !valid_outputs(&outputs, &self.limits) {
            return Err(ContentAllocationError::Malformed);
        }
        if outputs.len() > self.limits.max_outputs as usize
            || self.events.len() + self.response_credits + 1
                > self.limits.max_control_records as usize
        {
            return Err(ContentAllocationError::Budget);
        }
        let changed = |output: ContentOutputId| {
            self.outputs.iter().find(|row| row.output == output)
                != outputs.iter().find(|row| row.output == output)
        };
        if self.active.values().any(|entry| changed(entry.output))
            || self
                .pending
                .values()
                .any(|entry| changed(entry.request.output))
        {
            return Err(ContentAllocationError::AllocationLost);
        }
        self.facts_generation = facts_generation;
        self.outputs = outputs.clone();
        self.push(
            transaction,
            ShellContentRecord::OutputFacts(ContentOutputFacts {
                grant: self.limits.grant,
                facts_generation,
                outputs,
            }),
        );
        Ok(())
    }

    pub fn request(
        &mut self,
        transaction: TransactionId,
        request: ContentAllocationRequest,
        presented_parents: &[(ContentAllocationId, u64)],
        now: u64,
    ) -> Result<(), ContentAllocationError> {
        // Persistent catalog components reuse edge allocations, but cannot
        // inherit a bar popout or a transient launcher's opening-bound surface.
        let allowed = match self.profile {
            ContentStoreProfile::Legacy => true,
            ContentStoreProfile::PersistentCatalog => request.role == 1,
            ContentStoreProfile::NativeLauncher => false,
        };
        if !allowed {
            return Err(ContentAllocationError::Malformed);
        }
        self.request_inner(transaction, request, presented_parents, None, now)
    }

    fn request_inner(
        &mut self,
        transaction: TransactionId,
        request: ContentAllocationRequest,
        presented_parents: &[(ContentAllocationId, u64)],
        native_opening: Option<u64>,
        now: u64,
    ) -> Result<(), ContentAllocationError> {
        self.check_grant(request.grant)?;
        self.time(now)?;
        self.expire(now)?;
        if !transaction.is_valid() || request.allocation_request_id <= self.last_request_id {
            return Err(ContentAllocationError::Stale);
        }
        self.last_request_id = request.allocation_request_id;
        if !self.outputs.iter().any(|row| row.output == request.output) {
            return Err(ContentAllocationError::OutputLost);
        }
        if !valid_request(&request, &self.limits, native_opening.is_some()) {
            return Err(ContentAllocationError::Malformed);
        }
        if self.pending.len() >= self.limits.max_pending_allocation_requests as usize
            || self.events.len() + self.response_credits + 1
                > self.limits.max_control_records as usize
        {
            return Err(ContentAllocationError::Budget);
        }
        self.validate_operation(&request, presented_parents)?;
        let deadline = now
            .checked_add(u64::from(self.limits.allocation_timeout_ms))
            .ok_or(ContentAllocationError::Malformed)?;
        self.response_credits += 1;
        self.pending.insert(
            request.allocation_request_id,
            PendingAllocation {
                native_opening,
                transaction,
                request,
                deadline,
            },
        );
        Ok(())
    }

    pub fn grant(
        &mut self,
        request_id: u64,
        snapshot: ContentAllocationSnapshot,
        presented_parents: &[(ContentAllocationId, u64)],
    ) -> Result<(), ContentAllocationError> {
        let pending = self
            .pending
            .get(&request_id)
            .ok_or(ContentAllocationError::Stale)?;
        if pending.request.operation == 3 {
            return Err(ContentAllocationError::Malformed);
        }
        if pending.native_opening != snapshot.native_opening {
            return Err(ContentAllocationError::Stale);
        }
        self.validate_live_references(&pending.request, presented_parents)?;
        self.validate_snapshot(&pending.request, &snapshot)?;
        let pending = self.pending.remove(&request_id).expect("checked request");
        if pending.request.operation == 2 {
            self.active.remove(&pending.request.prior);
        }
        self.last_allocation_id = self.last_allocation_id.max(snapshot.allocation.id);
        self.active.insert(snapshot.allocation, snapshot.clone());
        self.response_credits -= 1;
        self.push(
            pending.transaction,
            ShellContentRecord::AllocationResult(result(
                self.limits.grant,
                request_id,
                1,
                ContentReason::None,
                &snapshot,
            )),
        );
        Ok(())
    }

    pub fn reject(
        &mut self,
        request_id: u64,
        error: ContentAllocationError,
    ) -> Result<(), ContentAllocationError> {
        let pending = self
            .pending
            .remove(&request_id)
            .ok_or(ContentAllocationError::Stale)?;
        self.response_credits -= 1;
        self.push(
            pending.transaction,
            ShellContentRecord::AllocationResult(zero_result(
                self.limits.grant,
                request_id,
                2,
                error.reason(),
                pending.request.output,
                ContentAllocationId::default(),
            )),
        );
        Ok(())
    }

    pub fn release(&mut self, request_id: u64) -> Result<(), ContentAllocationError> {
        let pending = self
            .pending
            .get(&request_id)
            .ok_or(ContentAllocationError::Stale)?;
        if pending.request.operation != 3
            || self
                .active
                .values()
                .any(|entry| entry.parent == pending.request.prior)
            || self.pending.values().any(|entry| {
                entry.request.allocation_request_id != request_id
                    && entry.request.parent == pending.request.prior
            })
        {
            return Err(ContentAllocationError::Stale);
        }
        let pending = self.pending.remove(&request_id).expect("checked request");
        let removed = self
            .active
            .remove(&pending.request.prior)
            .ok_or(ContentAllocationError::AllocationLost)?;
        self.response_credits -= 1;
        self.push(
            pending.transaction,
            ShellContentRecord::AllocationResult(zero_result(
                self.limits.grant,
                request_id,
                3,
                ContentReason::None,
                removed.output,
                removed.allocation,
            )),
        );
        Ok(())
    }

    pub fn invalidate(
        &mut self,
        transaction: TransactionId,
        allocation: ContentAllocationId,
        reason: ContentReason,
    ) -> Result<(), ContentAllocationError> {
        if !transaction.is_valid()
            || !matches!(
                reason,
                ContentReason::OutputLost | ContentReason::AllocationLost | ContentReason::Revoked
            )
            || self.events.len() + self.response_credits + 1
                > self.limits.max_control_records as usize
            || self.active.values().any(|entry| entry.parent == allocation)
            || self.pending.values().any(|entry| {
                entry.request.prior == allocation || entry.request.parent == allocation
            })
        {
            return Err(ContentAllocationError::Stale);
        }
        let snapshot = self
            .active
            .remove(&allocation)
            .ok_or(ContentAllocationError::AllocationLost)?;
        self.push(
            transaction,
            ShellContentRecord::AllocationResult(result(
                self.limits.grant,
                0,
                4,
                reason,
                &snapshot,
            )),
        );
        Ok(())
    }

    pub fn expire(&mut self, now: u64) -> Result<(), ContentAllocationError> {
        self.time(now)?;
        let expired: Vec<_> = self
            .pending
            .iter()
            .filter_map(|(id, pending)| (now >= pending.deadline).then_some(*id))
            .collect();
        for request_id in expired {
            self.reject(request_id, ContentAllocationError::Timeout)?;
        }
        Ok(())
    }

    pub fn revoke(&mut self) {
        self.revoked = true;
        self.outputs.clear();
        self.active.clear();
        self.pending.clear();
        self.events.clear();
        self.response_credits = 0;
    }

    fn validate_operation(
        &self,
        request: &ContentAllocationRequest,
        presented_parents: &[(ContentAllocationId, u64)],
    ) -> Result<(), ContentAllocationError> {
        if request.operation != 1
            && self
                .pending
                .values()
                .any(|pending| pending.request.prior == request.prior)
        {
            return Err(ContentAllocationError::Stale);
        }
        match request.operation {
            1 => {
                self.reserve_slot(request, None)?;
            }
            2 => {
                let prior = self
                    .active
                    .get(&request.prior)
                    .ok_or(ContentAllocationError::AllocationLost)?;
                if prior.output != request.output || prior.role != request.role {
                    return Err(ContentAllocationError::Stale);
                }
                if self
                    .active
                    .values()
                    .any(|entry| entry.parent == request.prior)
                    || self
                        .pending
                        .values()
                        .any(|entry| entry.request.parent == request.prior)
                {
                    return Err(ContentAllocationError::Stale);
                }
                self.reserve_slot(request, Some(request.prior))?;
            }
            3 => {
                let prior = self
                    .active
                    .get(&request.prior)
                    .ok_or(ContentAllocationError::AllocationLost)?;
                if prior.output != request.output || prior.role != request.role {
                    return Err(ContentAllocationError::Stale);
                }
                if self
                    .active
                    .values()
                    .any(|entry| entry.parent == request.prior)
                    || self
                        .pending
                        .values()
                        .any(|entry| entry.request.parent == request.prior)
                {
                    return Err(ContentAllocationError::Stale);
                }
            }
            _ => return Err(ContentAllocationError::Malformed),
        }
        self.validate_live_references(request, presented_parents)?;
        Ok(())
    }

    fn validate_live_references(
        &self,
        request: &ContentAllocationRequest,
        presented_parents: &[(ContentAllocationId, u64)],
    ) -> Result<(), ContentAllocationError> {
        if request.operation == 2 {
            self.active
                .get(&request.prior)
                .filter(|prior| prior.output == request.output && prior.role == request.role)
                .ok_or(ContentAllocationError::AllocationLost)?;
        }
        if request.operation != 3 && request.role == 2 {
            let parent = self
                .active
                .get(&request.parent)
                .filter(|parent| parent.output == request.output && parent.role == 1)
                .ok_or(ContentAllocationError::AllocationLost)?;
            if !presented_parents.contains(&(parent.allocation, request.parent_presentation_epoch))
                || !inside(
                    request.anchor_parent_rect,
                    parent.pixel.width,
                    parent.pixel.height,
                )
                || self
                    .pending
                    .values()
                    .any(|pending| pending.request.prior == parent.allocation)
            {
                return Err(ContentAllocationError::Stale);
            }
        }
        Ok(())
    }

    fn reserve_slot(
        &self,
        request: &ContentAllocationRequest,
        replacing: Option<ContentAllocationId>,
    ) -> Result<(), ContentAllocationError> {
        let active = self
            .active
            .values()
            .filter(|entry| Some(entry.allocation) != replacing);
        let total = active.clone().count()
            + self
                .pending
                .values()
                .filter(|entry| entry.request.operation != 3)
                .count();
        let per_output = active
            .clone()
            .filter(|entry| entry.output == request.output)
            .count()
            + self
                .pending
                .values()
                .filter(|entry| {
                    entry.request.operation != 3 && entry.request.output == request.output
                })
                .count();
        let per_role = active
            .filter(|entry| entry.output == request.output && entry.role == request.role)
            .count()
            + self
                .pending
                .values()
                .filter(|entry| {
                    entry.request.operation != 3
                        && entry.request.output == request.output
                        && entry.request.role == request.role
                })
                .count();
        let role_limit = if request.role == 3 {
            1
        } else if request.role == 1 {
            self.limits.max_panels_per_output
        } else {
            self.limits.max_popouts_per_output
        } as usize;
        if (request.role == 3 && total >= 1)
            || total >= self.limits.max_allocations_total as usize
            || per_output >= self.limits.max_allocations_per_output as usize
            || per_role >= role_limit
        {
            return Err(ContentAllocationError::Budget);
        }
        Ok(())
    }

    fn validate_snapshot(
        &self,
        request: &ContentAllocationRequest,
        snapshot: &ContentAllocationSnapshot,
    ) -> Result<(), ContentAllocationError> {
        let output = self
            .outputs
            .iter()
            .find(|row| row.output == request.output)
            .ok_or(ContentAllocationError::OutputLost)?;
        let identity_valid = if request.operation == 1 {
            snapshot.allocation.id > self.last_allocation_id && snapshot.allocation.generation == 1
        } else {
            snapshot.allocation.id == request.prior.id
                && snapshot.allocation.generation == request.prior.generation.saturating_add(1)
        };
        if !identity_valid
            || snapshot.output != request.output
            || snapshot.role != request.role
            || snapshot.edge != request.edge
            || snapshot.margins != request.margins
            || snapshot.parent != request.parent
            || snapshot.anchor_parent_rect != request.anchor_parent_rect
            || snapshot.logical.width != request.desired_width
            || snapshot.logical.height != request.desired_height
            || snapshot.scale_generation != output.scale_generation
            || snapshot.scale_numerator != output.scale_numerator
            || snapshot.scale_denominator != output.scale_denominator
            || !resolved_pixel_geometry_is_valid(snapshot)
            || output_pixel_extent(output)
                .is_none_or(|(width, height)| !inside(snapshot.pixel, width, height))
            || snapshot.allowed_reservation_extent > self.limits.max_reservation_extent
            || (snapshot.role != 1 && snapshot.allowed_reservation_extent != 0)
        {
            return Err(ContentAllocationError::Malformed);
        }
        let logical_thickness = if matches!(snapshot.edge, 1 | 3) {
            snapshot.logical.height
        } else {
            snapshot.logical.width
        };
        let pixel_thickness = if matches!(snapshot.edge, 1 | 3) {
            snapshot.pixel.height
        } else {
            snapshot.pixel.width
        };
        if (snapshot.role == 1
            && (logical_thickness > self.limits.max_panel_extent
                || snapshot.allowed_reservation_extent > pixel_thickness))
            || (snapshot.role != 1
                && (snapshot.pixel.width > self.limits.max_popout_extent_px
                    || snapshot.pixel.height > self.limits.max_popout_extent_px))
            || !self.coverage_allows(snapshot, output)
        {
            return Err(ContentAllocationError::Budget);
        }
        Ok(())
    }

    fn coverage_allows(
        &self,
        candidate: &ContentAllocationSnapshot,
        output: &ContentOutputFactsEntry,
    ) -> bool {
        let retained: u64 = self
            .active
            .values()
            .filter(|entry| {
                entry.output == candidate.output && entry.allocation.id != candidate.allocation.id
            })
            .map(|entry| u64::from(entry.pixel.width) * u64::from(entry.pixel.height))
            .sum();
        let area = retained
            .saturating_add(u64::from(candidate.pixel.width) * u64::from(candidate.pixel.height));
        let Some((output_width, output_height)) = output_pixel_extent(output) else {
            return false;
        };
        let output_area = u64::from(output_width) * u64::from(output_height);
        area.saturating_mul(100)
            <= output_area.saturating_mul(u64::from(self.limits.max_content_coverage_percent))
    }

    fn check_grant(&self, grant: ContentGrant) -> Result<(), ContentAllocationError> {
        if self.revoked || grant != self.limits.grant {
            Err(ContentAllocationError::Stale)
        } else {
            Ok(())
        }
    }

    fn time(&mut self, now: u64) -> Result<(), ContentAllocationError> {
        if now < self.last_now {
            return Err(ContentAllocationError::ClockRegression);
        }
        self.last_now = now;
        Ok(())
    }

    fn push(&mut self, transaction: TransactionId, record: ShellContentRecord) {
        self.events.push_back(ContentAllocationEvent {
            transaction,
            record,
        });
    }
}

fn resolved_pixel_geometry_is_valid(snapshot: &ContentAllocationSnapshot) -> bool {
    let Some(quantized) = quantize(
        snapshot.logical,
        snapshot.scale_numerator,
        snapshot.scale_denominator,
    ) else {
        return false;
    };
    if snapshot.role != 2 {
        quantized == snapshot.pixel
    } else {
        // A popout origin is selected from its physical parent anchor. At a
        // fractional scale it may have no integer logical representation; the
        // exact acknowledged physical origin is authoritative while the
        // desired logical extent still determines its physical size.
        quantized.width == snapshot.pixel.width && quantized.height == snapshot.pixel.height
    }
}

fn output_pixel_extent(output: &ContentOutputFactsEntry) -> Option<(u32, u32)> {
    let extent = |logical: u32| {
        let scaled = u128::from(logical).checked_mul(u128::from(output.scale_numerator))?;
        let denominator = u128::from(output.scale_denominator);
        u32::try_from(scaled.div_ceil(denominator)).ok()
    };
    Some((extent(output.local_width)?, extent(output.local_height)?))
}

fn valid_outputs(outputs: &[ContentOutputFactsEntry], limits: &ContentLimits) -> bool {
    outputs.iter().enumerate().all(|(index, output)| {
        output.output.id > 0
            && output.output.generation > 0
            && output.local_width > 0
            && output.local_height > 0
            && output.scale_numerator > 0
            && output.scale_numerator <= limits.max_scale_numerator
            && output.scale_denominator > 0
            && output.scale_denominator <= limits.max_scale_denominator
            && gcd(output.scale_numerator, output.scale_denominator) == 1
            && output.scale_generation > 0
            && !outputs[..index]
                .iter()
                .any(|prior| prior.output.id == output.output.id)
    })
}

fn valid_request(request: &ContentAllocationRequest, limits: &ContentLimits, native: bool) -> bool {
    let margins = [
        request.margins.top,
        request.margins.right,
        request.margins.bottom,
        request.margins.left,
    ];
    if !(1..=3).contains(&request.operation)
        || !(if native {
            request.role == 3
        } else {
            (1..=2).contains(&request.role)
        })
        || !(1..=4).contains(&request.edge)
        || margins
            .into_iter()
            .any(|value| i32::from(value).unsigned_abs() > limits.max_margin_logical)
    {
        return false;
    }
    if request.operation == 3 {
        return request.prior != ContentAllocationId::default()
            && request.parent == ContentAllocationId::default()
            && request.parent_presentation_epoch == 0
            && request.anchor_parent_rect == ContentPixelRect::default()
            && request.desired_width == 0
            && request.desired_height == 0
            && request.margins == ContentMargins::default();
    }
    request.desired_width > 0
        && request.desired_height > 0
        && (request.operation != 1 || request.prior == ContentAllocationId::default())
        && if request.role == 1 || native {
            request.parent == ContentAllocationId::default()
                && request.parent_presentation_epoch == 0
                && request.anchor_parent_rect == ContentPixelRect::default()
        } else {
            request.parent != ContentAllocationId::default()
                && request.parent_presentation_epoch > 0
                && request.anchor_parent_rect.width > 0
                && request.anchor_parent_rect.height > 0
        }
}

fn result(
    grant: ContentGrant,
    request_id: u64,
    status: u16,
    reason: ContentReason,
    snapshot: &ContentAllocationSnapshot,
) -> ContentAllocationResult {
    ContentAllocationResult {
        grant,
        allocation_request_id: request_id,
        status,
        reason: reason as u16,
        output: snapshot.output,
        allocation: snapshot.allocation,
        parent: snapshot.parent,
        scale_generation: snapshot.scale_generation,
        logical: snapshot.logical,
        pixel: snapshot.pixel,
        scale_numerator: snapshot.scale_numerator,
        scale_denominator: snapshot.scale_denominator,
        allowed_reservation_extent: snapshot.allowed_reservation_extent,
        margins: snapshot.margins,
        acknowledged_anchor: snapshot.anchor_parent_rect,
    }
}

fn zero_result(
    grant: ContentGrant,
    request_id: u64,
    status: u16,
    reason: ContentReason,
    output: ContentOutputId,
    allocation: ContentAllocationId,
) -> ContentAllocationResult {
    ContentAllocationResult {
        grant,
        allocation_request_id: request_id,
        status,
        reason: reason as u16,
        output,
        allocation,
        parent: ContentAllocationId::default(),
        scale_generation: 0,
        logical: ContentLogicalRect::default(),
        pixel: ContentPixelRect::default(),
        scale_numerator: 0,
        scale_denominator: 0,
        allowed_reservation_extent: 0,
        margins: ContentMargins::default(),
        acknowledged_anchor: ContentPixelRect::default(),
    }
}

fn inside(rect: ContentPixelRect, width: u32, height: u32) -> bool {
    rect.width > 0
        && rect.height > 0
        && rect.x >= 0
        && rect.y >= 0
        && u64::try_from(rect.x).unwrap_or(u64::MAX) + u64::from(rect.width) <= u64::from(width)
        && u64::try_from(rect.y).unwrap_or(u64::MAX) + u64::from(rect.height) <= u64::from(height)
}

fn quantize(
    logical: ContentLogicalRect,
    numerator: u32,
    denominator: u32,
) -> Option<ContentPixelRect> {
    let numerator = i128::from(numerator);
    let denominator = i128::from(denominator);
    let x0 = i128::from(logical.x).checked_mul(numerator)?;
    let y0 = i128::from(logical.y).checked_mul(numerator)?;
    let x1 = (i128::from(logical.x) + i128::from(logical.width)).checked_mul(numerator)?;
    let y1 = (i128::from(logical.y) + i128::from(logical.height)).checked_mul(numerator)?;
    let x = x0.div_euclid(denominator);
    let y = y0.div_euclid(denominator);
    let right = div_ceil(x1, denominator);
    let bottom = div_ceil(y1, denominator);
    Some(ContentPixelRect {
        x: i32::try_from(x).ok()?,
        y: i32::try_from(y).ok()?,
        width: u32::try_from(right.checked_sub(x)?).ok()?,
        height: u32::try_from(bottom.checked_sub(y)?).ok()?,
    })
}

fn div_ceil(value: i128, divisor: i128) -> i128 {
    let floor = value.div_euclid(divisor);
    floor + i128::from(value.rem_euclid(divisor) != 0)
}

fn gcd(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}
