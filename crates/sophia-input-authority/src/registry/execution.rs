//! Pool records survive release, revocation, focus movement and slot reuse.
use super::*;

impl AuthorityInstance {
    pub fn execute_press(
        &mut self,
        submit: &SubmitHandle,
        capability: DeviceCapability,
        input: Input,
        context: ExecutionContext,
        to: Recipient,
    ) -> Result<Applied, RegistrationError> {
        let source = self.validate_execution(submit, capability, context)?;
        self.apply_press(source, input, to)
    }

    /// Ordinary releases are executions too. Cleanup uses issuer-only
    /// revocation/retirement instead and therefore outlives a revoked context.
    pub fn release(
        &mut self,
        submit: &SubmitHandle,
        capability: DeviceCapability,
        input: Input,
        context: ExecutionContext,
    ) -> Result<ReleaseOutcome, RegistrationError> {
        let source = self.validate_execution(submit, capability, context)?;
        self.check_input(input)?;
        Ok(self.release_inner(source, input))
    }

    /// This is an issuer path for modelled physical ingress, not an adapter
    /// capability. Physical emergency recognition must run before aggregate
    /// delivery; a recipient barrier must not swallow that recognition.
    pub fn execute_physical_press(
        &mut self,
        issuer: &IssuerHandle,
        source: SourceId,
        input: Input,
        to: Recipient,
    ) -> Result<Applied, RegistrationError> {
        self.check_issuer(issuer)?;
        self.check_physical(source)?;
        self.apply_press(source, input, to)
    }

    pub fn release_physical(
        &mut self,
        issuer: &IssuerHandle,
        source: SourceId,
        input: Input,
    ) -> Result<ReleaseOutcome, RegistrationError> {
        self.check_issuer(issuer)?;
        self.check_physical(source)?;
        self.check_input(input)?;
        Ok(self.release_inner(source, input))
    }

    pub(super) fn check_input(&self, input: Input) -> Result<(), RegistrationError> {
        if input.slot() < self.capacity.input_slots() {
            Ok(())
        } else {
            Err(RegistrationError::Capacity(CapacityError::NoHoldRecord))
        }
    }

    fn add_participant(&mut self, index: usize, source: SourceId) {
        let bit = 1u64 << source.holder_bit(self.capacity.synthetic_sources());
        let record = self.records[index].as_mut().expect("reserved record");
        record.holders |= bit;
        if record.participants & bit != 0 {
            return;
        }
        record.participants |= bit;
        let source_record = self.record_mut(source);
        source_record.references += 1;
        if let Some(grant) = source_record.owner {
            self.grants[grant.slot].references += 1;
        }
    }

    pub(super) fn apply_press(
        &mut self,
        source: SourceId,
        input: Input,
        to: Recipient,
    ) -> Result<Applied, RegistrationError> {
        self.check_input(input)?;
        if let Some(index) = self.active[input.slot()] {
            let incarnation = self.records[index].expect("active record").incarnation;
            self.add_participant(index, source);
            // Joining or repeating a held source changes no aggregate press.
            return Ok(Applied {
                source,
                input,
                incarnation,
                first_press: false,
            });
        }
        // Check EVERY retained incarnation, not the latest recipient or owner.
        // Native debt is seat state; transport debt concerns its exact recipient.
        // Physical delivery needs the same barrier or a delayed old wire release
        // could clear the operator's new press. Recognition is a separate path.
        if self.records.iter().flatten().any(|record| {
            record.incarnation.input == input
                && record.holders == 0
                && (!record.settlement.native_reconciled
                    || ((!record.settlement.recipient_settled || record.attempt.is_some())
                        && record.incarnation.recipient == to.recipient
                        && record.incarnation.connection_generation == to.connection_generation))
        }) {
            return Err(RegistrationError::ReleaseBarrier);
        }
        let synthetic_end = self.capacity.debt_records();
        let range = if source.synthetic {
            0..synthetic_end
        } else {
            synthetic_end..self.records.len()
        };
        let index = range
            .into_iter()
            .find(|&i| self.records[i].is_none())
            .ok_or(RegistrationError::Capacity(CapacityError::NoHoldRecord))?;
        let incarnation = HoldIncarnation {
            authority: self.uid,
            recipient: to.recipient,
            connection_generation: to.connection_generation,
            input,
            hold: self.identity()?,
        };
        self.records[index] = Some(HoldRecord {
            incarnation,
            holders: 0,
            participants: 0,
            settlement: SettlementBit::default(),
            attempt: None,
        });
        self.add_participant(index, source);
        self.active[input.slot()] = Some(index);
        Ok(Applied {
            source,
            input,
            incarnation,
            first_press: true,
        })
    }

    pub(super) fn release_inner(&mut self, source: SourceId, input: Input) -> ReleaseOutcome {
        let Some(index) = self.active[input.slot()] else {
            return ReleaseOutcome::NotHeld;
        };
        let bit = 1u64 << source.holder_bit(self.capacity.synthetic_sources());
        let record = self.records[index].as_mut().expect("active record");
        if record.holders & bit == 0 {
            return ReleaseOutcome::NotHeld;
        }
        record.holders &= !bit;
        if record.holders != 0 {
            return ReleaseOutcome::SurvivorRemains;
        }
        self.active[input.slot()] = None;
        // Both obligation bits were reserved at the original press. Nothing is
        // allocated, copied into a reusable cell, or dropped during retirement.
        ReleaseOutcome::DeliverTo(record.incarnation)
    }

    pub(super) fn retire_source(&mut self, source: SourceId) -> RetiredDebt {
        self.record_mut(source).live = false;
        let mut result = RetiredDebt::default();
        for input_slot in 0..self.active.len() {
            let Some(index) = self.active[input_slot] else {
                continue;
            };
            let input = self.records[index]
                .expect("active record")
                .incarnation
                .input;
            match self.release_inner(source, input) {
                ReleaseOutcome::DeliverTo(_) => result.owed_releases += 1,
                ReleaseOutcome::SurvivorRemains => result.survivors += 1,
                ReleaseOutcome::NotHeld => (),
            }
        }
        result
    }

    /// Fixed-storage sweep for the scheduler. The cursor persists across calls;
    /// the attempt scheduler must reserve its own slot before starting delivery.
    /// This method reports debt, it does not itself claim an attempt or settle it.
    pub fn next_debt(&self, cursor: &mut usize) -> Option<(HoldIncarnation, SettlementBit)> {
        let count = self.records.len();
        for _ in 0..count {
            let index = *cursor % count;
            *cursor = (index + 1) % count;
            if let Some(record) = self.records[index]
                && record.holders == 0
            {
                return Some((record.incarnation, record.settlement));
            }
        }
        None
    }

    /// Accepts only a retained incarnation and an owning participant. The issuer
    /// is responsible for mapping actual server/transport receipts to these bits;
    /// a failed route, timeout or missing surface is not a settlement receipt.
    pub fn settle(
        &mut self,
        issuer: &IssuerHandle,
        owner: Option<GrantId>,
        input: Input,
        incarnation: HoldIncarnation,
        bit: SettlementBit,
    ) -> Result<bool, RegistrationError> {
        self.check_issuer(issuer)?;
        if incarnation.authority != self.uid || incarnation.input != input {
            return Ok(false);
        }
        let Some(index) = self
            .records
            .iter()
            .position(|r| r.is_some_and(|r| r.incarnation == incarnation && r.holders == 0))
        else {
            return Ok(false);
        };
        let participants = self.records[index].expect("retained").participants;
        let authorized_owner = (0..u64::BITS as usize).any(|i| {
            participants & (1u64 << i) != 0
                && self
                    .record(self.source_at_bit(i))
                    .expect("retained source")
                    .owner
                    == owner
        });
        if !authorized_owner {
            return Ok(false);
        }
        let record = self.records[index].as_mut().expect("retained");
        record.settlement.native_reconciled |= bit.native_reconciled;
        record.settlement.recipient_settled |= bit.recipient_settled;
        if !record.settlement.is_settled() || record.attempt.is_some() {
            return Ok(false);
        }
        self.free_record(index);
        Ok(true)
    }

    pub(super) fn free_record(&mut self, index: usize) {
        let record = self.records[index].take().expect("retained record");
        assert_eq!(record.holders, 0);
        assert!(record.settlement.is_settled() && record.attempt.is_none());
        let participants = record.participants;
        for i in 0..u64::BITS as usize {
            if participants & (1u64 << i) == 0 {
                continue;
            }
            let source = self.source_at_bit(i);
            let record = self.record_mut(source);
            record.references = record
                .references
                .checked_sub(1)
                .expect("one retained source reference");
            if let Some(grant) = record.owner {
                let slot = &mut self.grants[grant.slot];
                assert_eq!(slot.id, grant, "referenced grant cannot be recycled");
                slot.references = slot
                    .references
                    .checked_sub(1)
                    .expect("one retained grant reference");
            }
        }
    }
}
