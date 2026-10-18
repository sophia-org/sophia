use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeAllocationParams {
    pub edge: u16,
    pub width: u32,
    pub height: u32,
    pub margins: ContentMargins,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeResourceState {
    Accepted,
    Retiring,
}

#[derive(Default)]
pub(super) struct Allocation {
    pub(super) granted: Option<ContentAllocationResult>,
    pub(super) opening: u64,
    pub(super) orphaned: bool,
    pub(super) pending: Option<(u64, Ticket, u64, u16, u64)>, // request, ticket, opening, edge, time
    pub(super) edge: u16,
    next_request: u64,
}
#[derive(Clone, Copy)]
pub(super) struct Resource {
    pub(super) id: ContentResourceId,
    pub(super) state: NativeResourceState,
    pub(super) ticket: Option<Ticket>,
    pub(super) image_offer: u64,
}
impl NativeLauncherLifecycle {
    /// A current grant disappears from this view at close, before its eventual
    /// invalidation. Closing is not proof that old pixels or resources retired.
    pub fn allocation(&self) -> Option<&ContentAllocationResult> {
        if self.allocation.orphaned {
            None
        } else {
            self.allocation.granted.as_ref()
        }
    }
    pub fn awaiting_invalidation(&self) -> bool {
        self.allocation.orphaned
    }
    /// Informational server timeout; local expiry does not settle the request.
    pub fn allocation_deadline_ms(&self) -> Option<u64> {
        self.allocation
            .pending
            .map(|(_, _, _, _, at)| at.saturating_add(u64::from(self.limits.allocation_timeout_ms)))
    }
    pub fn resource_state(&self, id: ContentResourceId) -> Option<NativeResourceState> {
        self.resources
            .iter()
            .flatten()
            .find(|r| r.id == id)
            .map(|r| r.state)
    }
    pub(super) fn resource_index(&self, id: ContentResourceId) -> Option<usize> {
        self.resources
            .iter()
            .position(|r| r.is_some_and(|r| r.id == id))
    }
    pub(super) fn free_resource(&mut self, id: ContentResourceId) {
        if let Some(index) = self.resource_index(id) {
            self.resources[index] = None;
        }
    }
    pub(super) fn allocation_result(
        &mut self,
        value: &ContentAllocationResult,
    ) -> Result<(), ShellClientError> {
        if value.status == 4 {
            if self
                .allocation
                .granted
                .as_ref()
                .is_some_and(|v| v.allocation == value.allocation)
            {
                self.allocation.granted = None;
                self.allocation.orphaned = false;
            }
            return Ok(());
        }
        let (request, _, opening, edge, _) = self.allocation.pending.ok_or(
            ShellClientError::Protocol("unsolicited native allocation result"),
        )?;
        if request != value.allocation_request_id {
            return Err(ShellClientError::Protocol("native allocation correlation"));
        }
        self.allocation.pending = None;
        match value.status {
            1 => {
                self.allocation.granted = Some(value.clone());
                self.allocation.opening = opening;
                self.allocation.edge = edge;
                self.allocation.orphaned = self.opening.is_none_or(|v| v.opening != opening);
            }
            2 => {} // Rejected resize keeps the previous grant.
            3 => {
                self.allocation.granted = None;
                self.allocation.orphaned = false;
            }
            _ => return Err(ShellClientError::Protocol("native allocation status")),
        }
        Ok(())
    }
    pub(super) fn resource_status(
        &mut self,
        transaction: TransactionId,
        value: &ContentResourceStatus,
    ) -> Result<(), ShellClientError> {
        let Some(index) = self.resource_index(value.resource) else {
            return Ok(());
        };
        let offer = self.resources[index].unwrap().image_offer;
        // Adoption already verified Accepted. Late delivery cannot resurrect
        // a Retiring resource; every contradictory status fails closed.
        if transaction == TransactionId::from_raw(offer) && value.status == 2 && value.reason == 0 {
            Ok(())
        } else {
            Err(ShellClientError::Protocol("adopted image status"))
        }
    }
    pub(super) fn service_resources(&mut self, connection: &ShellConnection) {
        if let Some((_, ticket, _, _, _)) = self.allocation.pending
            && matches!(
                connection.custody(ticket),
                Some(Custody::Refused(_) | Custody::DroppedUnsent)
            )
        {
            self.allocation.pending = None;
        }
        for index in 0..self.resources.len() {
            let Some(resource) = self.resources[index] else {
                continue;
            };
            let Some(ticket) = resource.ticket else {
                continue;
            };
            let outcome = connection.custody(ticket);
            if matches!(outcome, Some(Custody::Queued | Custody::InFlight)) {
                continue;
            }
            self.resources[index].as_mut().unwrap().ticket = None;
            if matches!(outcome, Some(Custody::Refused(_) | Custody::DroppedUnsent))
                && resource.state == NativeResourceState::Retiring
            {
                self.resources[index].as_mut().unwrap().state = NativeResourceState::Accepted;
            }
        }
    }
}

impl ShellConnection {
    /// Acquire or resize the current opening's parentless allocation. One
    /// request at a time; an orphaned old grant blocks reopening until invalidated.
    pub fn allocate_native(
        &mut self,
        lifecycle: &mut NativeLauncherLifecycle,
        transaction: TransactionId,
        params: NativeAllocationParams,
        now_ms: u64,
    ) -> Result<Admission, ShellClientError> {
        self.native_allocation_request(lifecycle, transaction, Some(params), now_ms)
    }
    pub fn release_native_allocation(
        &mut self,
        lifecycle: &mut NativeLauncherLifecycle,
        transaction: TransactionId,
        now_ms: u64,
    ) -> Result<Admission, ShellClientError> {
        self.native_allocation_request(lifecycle, transaction, None, now_ms)
    }
    fn native_allocation_request(
        &mut self,
        lifecycle: &mut NativeLauncherLifecycle,
        transaction: TransactionId,
        params: Option<NativeAllocationParams>,
        now_ms: u64,
    ) -> Result<Admission, ShellClientError> {
        lifecycle.service(self)?;
        let opening = lifecycle
            .opening
            .ok_or(ShellClientError::InvalidArgument("native opening absent"))?;
        let a = &lifecycle.allocation;
        if a.pending.is_some() || a.orphaned || (params.is_none() && a.granted.is_none()) {
            return Err(ShellClientError::InvalidArgument(
                "native allocation busy or absent",
            ));
        }
        let request_id = a.next_request.max(1);
        let next = request_id
            .checked_add(1)
            .ok_or(ShellClientError::InvalidArgument(
                "native request exhausted",
            ))?;
        let (edge, width, height, margins) = params
            .map_or((a.edge, 0, 0, ContentMargins::default()), |p| {
                (p.edge, p.width, p.height, p.margins)
            });
        let operation = if params.is_none() {
            3
        } else if a.granted.is_some() {
            2
        } else {
            1
        };
        let request = NativeLauncherAllocationRequest {
            grant: lifecycle.limits.grant,
            opening: opening.opening,
            output: opening.output,
            request_id,
            prior: a
                .granted
                .as_ref()
                .map_or(ContentAllocationId::default(), |v| v.allocation),
            operation,
            edge,
            desired_width: width,
            desired_height: height,
            margins,
        };
        let admission = self.enqueue_native_launcher_tracked(&ShellFileNativeLauncherRecord {
            transaction,
            record: ShellNativeLauncherRecord::AllocationRequest(request),
        })?;
        lifecycle.allocation.next_request = next;
        lifecycle.allocation.pending =
            Some((request_id, admission.first, opening.opening, edge, now_ms));
        Ok(admission)
    }

    /// Retire only Accepted storage. Close does not retire it; ResourceReleased
    /// is the release barrier. A submit refusal restores Accepted on service.
    pub fn retire_native_resource(
        &mut self,
        lifecycle: &mut NativeLauncherLifecycle,
        transaction: TransactionId,
        resource: ContentResourceId,
    ) -> Result<Admission, ShellClientError> {
        lifecycle.service(self)?;
        let index = lifecycle
            .resource_index(resource)
            .ok_or(ShellClientError::InvalidArgument("native resource absent"))?;
        if lifecycle.resources[index].unwrap().state != NativeResourceState::Accepted {
            return Err(ShellClientError::InvalidArgument(
                "native resource not accepted",
            ));
        }
        let admission = self.enqueue_content_tracked(
            transaction,
            &ShellContentRecord::ResourceRetire(ContentResourceRetire {
                grant: lifecycle.limits.grant,
                resource,
            }),
        )?;
        let tracked = lifecycle.resources[index].as_mut().unwrap();
        tracked.state = NativeResourceState::Retiring;
        tracked.ticket = Some(admission.first);
        Ok(admission)
    }
}
