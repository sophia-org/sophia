use super::*;

impl ContentAllocationStore {
    /// Session supplies the currently authorized opening, never a value copied
    /// from this request. The opening stays attached to the actual pending and
    /// granted allocation; old openings cannot resize/release its replacement.
    pub fn request_native_launcher(
        &mut self,
        transaction: TransactionId,
        request: NativeLauncherAllocationRequest,
        current: NativeLauncherOpening,
        now: u64,
    ) -> Result<(), ContentAllocationError> {
        if self.profile != ContentStoreProfile::NativeLauncher
            || current.grant != self.limits.grant
            || current.opening == 0
            || current.catalog_generation == 0
            || current.state_revision != 1
            || request.grant != current.grant
            || request.output != current.output
            || request.opening != current.opening
        {
            return Err(ContentAllocationError::Stale);
        }
        // One opening per role/grant, including requests still awaiting Engine.
        if self
            .active
            .values()
            .any(|v| v.native_opening != Some(current.opening))
            || self
                .pending
                .values()
                .any(|v| v.native_opening != Some(current.opening))
        {
            return Err(ContentAllocationError::Stale);
        }
        let normalized = ContentAllocationRequest {
            grant: request.grant,
            output: request.output,
            allocation_request_id: request.request_id,
            operation: request.operation,
            role: 3,
            edge: request.edge,
            prior: request.prior,
            parent: ContentAllocationId::default(),
            parent_presentation_epoch: 0,
            anchor_parent_rect: ContentPixelRect::default(),
            desired_width: request.desired_width,
            desired_height: request.desired_height,
            margins: request.margins,
        };
        self.request_inner(transaction, normalized, &[], Some(current.opening), now)
    }

    /// Read the exact owner of the pending proposal while Engine decides its
    /// geometry. None denotes legacy provenance, not a guessed current opening.
    pub fn pending_native_opening(&self, request_id: u64) -> Option<u64> {
        self.pending.get(&request_id).and_then(|v| v.native_opening)
    }
}

impl ContentAllocationStore {
    /// Closing rejects proposals, but does not dispose live allocations before
    /// Session has removed their pixels. Each reserved reply remains charged.
    pub(crate) fn close_native_proposals(
        &mut self,
        opening: NativeLauncherOpening,
    ) -> Result<(), ContentAllocationError> {
        if self.profile != ContentStoreProfile::NativeLauncher
            || opening.grant != self.limits.grant
            || opening.opening == 0
            || self
                .pending
                .values()
                .any(|v| v.native_opening != Some(opening.opening))
        {
            return Err(ContentAllocationError::Stale);
        }
        while let Some((&id, _)) = self.pending.first_key_value() {
            self.reject(id, ContentAllocationError::Stale)?;
        }
        Ok(())
    }
}

impl ContentAllocationStore {
    pub(crate) fn reject_closed_native_request(
        &mut self,
        transaction: TransactionId,
        request: NativeLauncherAllocationRequest,
        opening: NativeLauncherOpening,
    ) -> Result<(), ContentAllocationError> {
        if self.profile != ContentStoreProfile::NativeLauncher
            || !transaction.is_valid()
            || request.grant != self.limits.grant
            || request.opening != opening.opening
            || request.output != opening.output
            || request.request_id == 0
        {
            return Err(ContentAllocationError::Stale);
        }
        if request.request_id <= self.last_request_id {
            return Ok(());
        }
        if self.events.len() + self.response_credits >= self.limits.max_control_records as usize {
            return Err(ContentAllocationError::Budget);
        }
        self.last_request_id = request.request_id;
        self.push(
            transaction,
            ShellContentRecord::AllocationResult(zero_result(
                self.limits.grant,
                request.request_id,
                2,
                ContentReason::Stale,
                request.output,
                ContentAllocationId::default(),
            )),
        );
        Ok(())
    }
}
