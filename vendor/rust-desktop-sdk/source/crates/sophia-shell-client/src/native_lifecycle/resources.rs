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
    Begun,
    Admitted,
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
    id: ContentResourceId,
    state: NativeResourceState,
    ticket: Option<Ticket>,
}
#[derive(Clone, Copy)]
pub(super) struct Upload {
    id: ContentResourceId,
    total: u64,
    chunks: u32,
    offset: u64,
    ordinal: u32,
    pending: Option<(Ticket, usize)>,
    failed: bool,
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
    fn resource_index(&self, id: ContentResourceId) -> Option<usize> {
        self.resources
            .iter()
            .position(|r| r.is_some_and(|r| r.id == id))
    }
    pub(super) fn free_resource(&mut self, id: ContentResourceId) {
        if let Some(index) = self.resource_index(id) {
            self.resources[index] = None;
        }
        if self.upload.is_some_and(|u| u.id == id) {
            self.upload = None;
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
    pub(super) fn resource_status(&mut self, value: &ContentResourceStatus) {
        let Some(index) = self.resource_index(value.resource) else {
            return;
        };
        match value.status {
            1 => self.resources[index].as_mut().unwrap().state = NativeResourceState::Admitted,
            2 => self.resources[index].as_mut().unwrap().state = NativeResourceState::Accepted,
            _ => self.free_resource(value.resource),
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
            if matches!(outcome, Some(Custody::Refused(_) | Custody::DroppedUnsent)) {
                match resource.state {
                    NativeResourceState::Begun => self.free_resource(resource.id),
                    NativeResourceState::Retiring => {
                        self.resources[index].as_mut().unwrap().state =
                            NativeResourceState::Accepted
                    }
                    _ => {}
                }
            }
        }
        if let Some(upload) = &mut self.upload
            && let Some((ticket, bytes)) = upload.pending
        {
            match connection.custody(ticket) {
                Some(Custody::Queued | Custody::InFlight) => {}
                Some(Custody::Stored) => {
                    upload.offset += bytes as u64;
                    upload.ordinal += 1;
                    upload.pending = None;
                }
                _ => {
                    upload.failed = true;
                    upload.pending = None;
                } // Never replay uncertain bytes.
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

    /// Start one bounded upload. The helper fills the grant and retains at most
    /// 16 resource identities. No resource is usable until Accepted arrives.
    pub fn begin_native_resource(
        &mut self,
        lifecycle: &mut NativeLauncherLifecycle,
        transaction: TransactionId,
        mut begin: ContentResourceBegin,
    ) -> Result<Admission, ShellClientError> {
        lifecycle.service(self)?;
        if lifecycle.upload.is_some() || lifecycle.resource_index(begin.resource).is_some() {
            return Err(ShellClientError::InvalidArgument(
                "native upload already active",
            ));
        }
        let index = lifecycle
            .resources
            .iter()
            .position(Option::is_none)
            .ok_or(ShellClientError::QueueSaturated)?;
        begin.grant = lifecycle.limits.grant;
        begin
            .layout(&lifecycle.limits)
            .map_err(|_| ShellClientError::InvalidArgument("native resource layout"))?;
        let upload = Upload {
            id: begin.resource,
            total: begin.total_bytes,
            chunks: begin.chunk_count,
            offset: 0,
            ordinal: 0,
            pending: None,
            failed: false,
        };
        let admission =
            self.enqueue_content_tracked(transaction, &ShellContentRecord::ResourceBegin(begin))?;
        lifecycle.resources[index] = Some(Resource {
            id: upload.id,
            state: NativeResourceState::Begun,
            ticket: Some(admission.first),
        });
        lifecycle.upload = Some(upload);
        Ok(admission)
    }

    /// Admit one chunk, then wait for Stored before the next chunk or End.
    /// No uncertain or refused payload is replayed. Cancel a failed upload.
    pub fn upload_native_chunk(
        &mut self,
        lifecycle: &mut NativeLauncherLifecycle,
        bytes: &[u8],
    ) -> Result<Admission, ShellClientError> {
        lifecycle.service(self)?;
        let upload = lifecycle
            .upload
            .ok_or(ShellClientError::InvalidArgument("no native upload"))?;
        if upload.pending.is_some() {
            return Err(ShellClientError::QueueSaturated);
        }
        if upload.failed
            || lifecycle.resource_state(upload.id) != Some(NativeResourceState::Admitted)
            || bytes.is_empty()
            || bytes.len() as u64 > upload.total - upload.offset
        {
            return Err(ShellClientError::InvalidArgument("native upload chunk"));
        }
        let record = ShellContentRecord::ResourceChunk(ContentResourceChunk {
            grant: lifecycle.limits.grant,
            resource: upload.id,
            ordinal: upload.ordinal,
            offset: upload.offset,
            bytes: bytes.to_vec(),
        });
        // A slot write has no transaction record; its ticket still tracks Stored.
        let admission = self.enqueue_content_tracked(TransactionId::INVALID, &record)?;
        lifecycle.upload.as_mut().unwrap().pending = Some((admission.first, bytes.len()));
        Ok(admission)
    }

    pub fn end_native_resource(
        &mut self,
        lifecycle: &mut NativeLauncherLifecycle,
        transaction: TransactionId,
    ) -> Result<Admission, ShellClientError> {
        lifecycle.service(self)?;
        let upload = lifecycle
            .upload
            .ok_or(ShellClientError::InvalidArgument("no native upload"))?;
        if upload.pending.is_some() {
            return Err(ShellClientError::QueueSaturated);
        }
        if upload.failed
            || upload.offset != upload.total
            || lifecycle.resource_state(upload.id) != Some(NativeResourceState::Admitted)
        {
            return Err(ShellClientError::InvalidArgument(
                "native upload incomplete",
            ));
        }
        let admission = self.enqueue_content_tracked(
            transaction,
            &ShellContentRecord::ResourceEnd(ContentResourceEnd {
                grant: lifecycle.limits.grant,
                resource: upload.id,
                total_bytes: upload.total,
                chunk_count: upload.chunks,
            }),
        )?;
        lifecycle.upload = None;
        Ok(admission)
    }

    /// Cancel a begun/admitted resource, including after a refused End. The
    /// resource remains tracked until its terminal status or release arrives.
    pub fn cancel_native_resource(
        &mut self,
        lifecycle: &mut NativeLauncherLifecycle,
        transaction: TransactionId,
        resource: ContentResourceId,
    ) -> Result<Admission, ShellClientError> {
        lifecycle.service(self)?;
        if !matches!(
            lifecycle.resource_state(resource),
            Some(NativeResourceState::Begun | NativeResourceState::Admitted)
        ) {
            return Err(ShellClientError::InvalidArgument(
                "native resource not staging",
            ));
        }
        let admission = self.enqueue_content_tracked(
            transaction,
            &ShellContentRecord::ResourceCancel(ContentResourceCancel {
                grant: lifecycle.limits.grant,
                resource,
            }),
        )?;
        if lifecycle.upload.is_some_and(|u| u.id == resource) {
            lifecycle.upload = None;
        }
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
        lifecycle.resources[index] = Some(Resource {
            id: resource,
            state: NativeResourceState::Retiring,
            ticket: Some(admission.first),
        });
        Ok(admission)
    }
}
