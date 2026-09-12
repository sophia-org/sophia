//! The authority instance: one seat, one instance, every source in one place.

use crate::capacity::{Capacity, CapacityError};
use crate::grant::{GrantGeneration, GrantId, IssuerHandle, SubmitHandle};
use crate::identity::{
    AuthorityUid, DeviceCapability, HoldIncarnation, Input, Origin, Recipient, SeatBinding,
    SourceId,
};
use crate::ledger::{Applied, ReleaseOutcome, SettlementBit};
use sophia_protocol::DeviceId;

/// Why a registration or submission was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationError {
    /// The handle or capability belongs to a different authority. Checked
    /// against an identity the caller cannot construct, not against the public
    /// seat binding, which anyone can rebuild.
    ForeignAuthority,
    /// The grant has been revoked or reissued since this capability was minted.
    StaleGeneration,
    /// Accepted under an epoch or publication that has since moved.
    StaleExecution,
    /// A transition has committed and not yet published, so there is no current
    /// target to route against.
    RoutingUnavailable,
    /// This input is still owed a release to an earlier recipient. A new hold
    /// waits rather than racing the old one's settlement.
    ReleaseBarrier,
    /// A bound was reached.
    Capacity(CapacityError),
}

/// One grant slot. Reusable only once nothing it owned is still owed.
#[derive(Clone, Copy, Debug)]
struct GrantSlot {
    generation: GrantGeneration,
    live: bool,
    /// Debts outstanding from sources this grant owned. The slot cannot be
    /// reissued while this is nonzero, or a new client would inherit an
    /// identity whose releases are still in flight.
    outstanding: usize,
}

#[derive(Clone, Copy, Debug)]
struct SourceRecord {
    origin: Origin,
    owner: Option<GrantId>,
    generation: GrantGeneration,
    device: DeviceId,
    live: bool,
}

/// One input's aggregate hold.
#[derive(Clone, Copy, Debug, Default)]
struct HoldRecord {
    holders: u64,
    delivered_to: Option<HoldIncarnation>,
    /// An earlier incarnation whose release has not settled.
    ///
    /// This blocks only a synthetic press that would deliver to the SAME
    /// recipient and connection. A different recipient is not implicated by
    /// this debt, and a physical source is never blocked by it: the operator's
    /// keyboard must not stop because an injector owes a release. Correctness
    /// for those cases rests on incarnation matching at settlement, which
    /// discards a stale completion rather than letting it clear a newer hold.
    awaiting_settlement: Option<HoldIncarnation>,
}

/// Scratch for one retirement's owed releases.
///
/// Fixed size and stack-resident: a retirement can owe at most one release per
/// input, and it must not need an allocator to say so.
struct FixedOwed {
    entries: [(usize, HoldIncarnation); Self::CAPACITY],
    len: usize,
}

impl FixedOwed {
    /// One per key plus one per button in the widest domain.
    const CAPACITY: usize = 256 + 256;
}

impl Default for FixedOwed {
    fn default() -> Self {
        Self {
            entries: [(
                0,
                HoldIncarnation {
                    recipient: 0,
                    connection_generation: 0,
                    input: Input::PLACEHOLDER,
                    hold: 0,
                },
            ); Self::CAPACITY],
            len: 0,
        }
    }
}

impl FixedOwed {
    fn push(&mut self, slot: usize, incarnation: HoldIncarnation) {
        if self.len < Self::CAPACITY {
            self.entries[self.len] = (slot, incarnation);
            self.len += 1;
        }
    }
}

/// One retained debt: a release owed to a recipient that no longer holds.
#[derive(Clone, Copy, Debug)]
struct DebtRecord {
    incarnation: HoldIncarnation,
    settlement: SettlementBit,
}

/// One seat's input authority inside one instance.
pub struct AuthorityInstance {
    uid: AuthorityUid,
    binding: SeatBinding,
    capacity: Capacity,
    synthetic: Vec<SourceRecord>,
    physical: Vec<SourceRecord>,
    holds: Vec<HoldRecord>,
    debts: Vec<Option<DebtRecord>>,
    grants: Vec<GrantSlot>,
    next_generation: u64,
    next_hold: u64,
    /// The revision a transition is moving to, set while routing is
    /// unavailable. Only a publish naming this revision re-enables routing, so
    /// a stale publication cannot reopen the window it was meant to close.
    pending_revision: Option<(u64, u64)>,
    publication: u64,
    epoch: u64,
}

impl AuthorityInstance {
    /// Build one authority for one seat, verifying the button domain.
    pub fn new(
        binding: SeatBinding,
        capacity: Capacity,
        advertised_buttons: u16,
    ) -> Result<(Self, IssuerHandle, SubmitHandle), CapacityError> {
        capacity.verify_button_domain(advertised_buttons)?;
        capacity.verify_holder_width()?;
        let uid = AuthorityUid::allocate().ok_or(CapacityError::IdentityExhausted)?;
        let instance = Self {
            uid,
            binding,
            capacity,
            synthetic: Vec::with_capacity(capacity.synthetic_sources()),
            physical: Vec::with_capacity(capacity.physical_sources),
            holds: vec![HoldRecord::default(); capacity.input_slots()],
            debts: vec![None; capacity.debt_records()],
            grants: Vec::with_capacity(capacity.grants),
            next_generation: 1,
            next_hold: 1,
            pending_revision: None,
            publication: 0,
            epoch: 0,
        };
        Ok((
            instance,
            IssuerHandle::new(uid, binding),
            SubmitHandle::new(uid, binding),
        ))
    }

    fn check_issuer(&self, issuer: &IssuerHandle) -> Result<(), RegistrationError> {
        if issuer.authority() == self.uid {
            Ok(())
        } else {
            Err(RegistrationError::ForeignAuthority)
        }
    }

    /// A submit handle proves the caller is this authority's adapter. Required
    /// alongside the capability, so a capability that leaked on its own is not
    /// enough to move the seat.
    fn check_submit(&self, submit: &SubmitHandle) -> Result<(), RegistrationError> {
        if submit.authority() == self.uid {
            Ok(())
        } else {
            Err(RegistrationError::ForeignAuthority)
        }
    }

    /// Where a debt for this owner and input lives.
    ///
    /// Computed rather than searched, so recording one allocates nothing and
    /// cannot fail for want of a free cell. Each grant owns a block, and
    /// physical sources own the block past the last grant.
    fn debt_slot(&self, owner: Option<GrantId>, input: Input) -> Option<usize> {
        let block = match owner {
            Some(grant) => usize::try_from(grant.0).ok()?,
            None => self.capacity.grants,
        };
        self.debt_slot_for_block(block, input.slot())
    }

    fn debt_slot_for_block(&self, block: usize, input_slot: usize) -> Option<usize> {
        block
            .checked_mul(self.capacity.input_slots())?
            .checked_add(input_slot)
    }

    /// A physical source must belong to this authority, be physical, and be
    /// registered and live. An index alone proves none of those, and trusting
    /// one from elsewhere would let a foreign id claim physical origin, which
    /// is what the emergency recognizer reads.
    fn check_physical_source(&self, source: SourceId) -> Result<(), RegistrationError> {
        if source.authority != self.uid || source.synthetic {
            return Err(RegistrationError::ForeignAuthority);
        }
        match self.record(source) {
            Some(record) if record.live && matches!(record.origin, Origin::Physical) => Ok(()),
            _ => Err(RegistrationError::ForeignAuthority),
        }
    }

    fn record(&self, source: SourceId) -> Option<&SourceRecord> {
        let table = if source.synthetic {
            &self.synthetic
        } else {
            &self.physical
        };
        table.get(usize::from(source.index))
    }

    /// Resolve a capability to a live source, or say why not.
    fn resolve(&self, capability: DeviceCapability) -> Result<SourceId, RegistrationError> {
        if capability.authority != self.uid {
            return Err(RegistrationError::ForeignAuthority);
        }
        let record = self
            .record(capability.source)
            .ok_or(RegistrationError::ForeignAuthority)?;
        if !record.live || record.generation != capability.generation {
            return Err(RegistrationError::StaleGeneration);
        }
        // The device's own record is not enough: the grant that owns it must
        // still be live at this generation, so revoking a grant invalidates
        // every capability under it without having to find them.
        let slot = self
            .grants
            .get(capability.grant.0 as usize)
            .ok_or(RegistrationError::StaleGeneration)?;
        if !slot.live || slot.generation != capability.generation {
            return Err(RegistrationError::StaleGeneration);
        }
        Ok(capability.source)
    }
}

/// What one execution is accepted under.
#[derive(Clone, Copy, Debug)]
pub struct ExecutionContext {
    pub generation: GrantGeneration,
    pub epoch: u64,
    pub publication: u64,
    pub request: u64,
}

/// How many records a retirement marked, without allocating to say so.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RetiredDebt {
    pub owed_releases: usize,
    pub survivors: usize,
}

impl AuthorityInstance {
    // ---- privileged: issuer only -------------------------------------------

    pub fn issue_grant(
        &mut self,
        issuer: &IssuerHandle,
    ) -> Result<(GrantId, GrantGeneration), RegistrationError> {
        self.check_issuer(issuer)?;
        let generation = GrantGeneration(self.next_generation);
        // Exhaustion stops the authority rather than wrapping. A reused
        // generation would let a capability minted long ago validate against a
        // grant issued to someone else.
        self.next_generation =
            self.next_generation
                .checked_add(1)
                .ok_or(RegistrationError::Capacity(
                    CapacityError::IdentityExhausted,
                ))?;

        // Reclaim a dead slot whose debts have all settled. Without this a
        // private instance is permanently exhausted by sixteen short-lived
        // clients that never held anything.
        if let Some(index) = self
            .grants
            .iter()
            .position(|slot| !slot.live && slot.outstanding == 0)
        {
            self.grants[index] = GrantSlot {
                generation,
                live: true,
                outstanding: 0,
            };
            self.release_devices_of(GrantId(index as u32));
            return Ok((GrantId(index as u32), generation));
        }
        if self.grants.len() >= self.capacity.grants {
            return Err(RegistrationError::Capacity(CapacityError::NoGrantSlot));
        }
        let id = GrantId(u32::try_from(self.grants.len()).unwrap_or(u32::MAX));
        self.grants.push(GrantSlot {
            generation,
            live: true,
            outstanding: 0,
        });
        Ok((id, generation))
    }

    /// Revoke a grant: its devices stop being live and its capabilities stop
    /// validating, immediately and without needing to find them.
    pub fn revoke_grant(
        &mut self,
        issuer: &IssuerHandle,
        grant: GrantId,
    ) -> Result<RetiredDebt, RegistrationError> {
        self.check_issuer(issuer)?;
        let mut debt = RetiredDebt::default();
        // Walk the fixed table by index. Collecting the doomed sources first
        // would allocate at exactly the moment allocation must not be needed:
        // revocation that fails for want of memory leaves holds nobody
        // releases.
        for index in 0..self.synthetic.len() {
            let owned = self.synthetic[index].owner == Some(grant) && self.synthetic[index].live;
            if !owned {
                continue;
            }
            let source = SourceId {
                authority: self.uid,
                synthetic: true,
                index: u16::try_from(index).unwrap_or(u16::MAX),
            };
            let retired = self.retire_source_inner(source);
            debt.owed_releases += retired.owed_releases;
            debt.survivors += retired.survivors;
        }
        if let Some(slot) = self.grants.get_mut(grant.0 as usize) {
            // Dead, and not reusable until its debts settle. The slot keeps its
            // generation so a late completion can still be matched to it.
            slot.live = false;
            slot.outstanding = slot.outstanding.saturating_add(debt.owed_releases);
        }
        Ok(debt)
    }

    pub fn allocate_device(
        &mut self,
        issuer: &IssuerHandle,
        grant: GrantId,
        generation: GrantGeneration,
        device: DeviceId,
    ) -> Result<DeviceCapability, RegistrationError> {
        self.check_issuer(issuer)?;
        // The grant must exist, be live, and be the generation the caller
        // names. Without this a revoked grant is resurrected simply by asking
        // for another device under its old identity.
        let slot = self
            .grants
            .get(grant.0 as usize)
            .ok_or(RegistrationError::StaleGeneration)?;
        if !slot.live || slot.generation != generation {
            return Err(RegistrationError::StaleGeneration);
        }
        let owned = self
            .synthetic
            .iter()
            .filter(|record| record.owner == Some(grant) && record.live)
            .count();
        if owned >= self.capacity.devices_per_grant {
            return Err(RegistrationError::Capacity(CapacityError::NoDeviceSlot));
        }
        // Reuse a dead record before growing. Without this the device table
        // fills after thirty-two short-lived clients even though none of them
        // still exists.
        let reusable = self.synthetic.iter().position(|record| !record.live);
        let index = match reusable {
            Some(index) => {
                self.synthetic[index] = SourceRecord {
                    origin: Origin::Synthetic,
                    owner: Some(grant),
                    generation,
                    device,
                    live: true,
                };
                u16::try_from(index).unwrap_or(u16::MAX)
            }
            None => {
                if self.synthetic.len() >= self.capacity.synthetic_sources() {
                    return Err(RegistrationError::Capacity(CapacityError::NoDeviceSlot));
                }
                let index = u16::try_from(self.synthetic.len()).unwrap_or(u16::MAX);
                self.synthetic.push(SourceRecord {
                    origin: Origin::Synthetic,
                    owner: Some(grant),
                    generation,
                    device,
                    live: true,
                });
                index
            }
        };
        Ok(DeviceCapability {
            source: SourceId {
                authority: self.uid,
                synthetic: true,
                index,
            },
            binding: self.binding,
            authority: self.uid,
            grant,
            generation,
            device,
        })
    }

    pub fn register_physical(
        &mut self,
        issuer: &IssuerHandle,
        device: DeviceId,
    ) -> Result<SourceId, RegistrationError> {
        self.check_issuer(issuer)?;
        if self.physical.len() >= self.capacity.physical_sources {
            return Err(RegistrationError::Capacity(CapacityError::NoPhysicalSlot));
        }
        let index = u16::try_from(self.physical.len()).unwrap_or(u16::MAX);
        self.physical.push(SourceRecord {
            origin: Origin::Physical,
            owner: None,
            generation: GrantGeneration(0),
            device,
            live: true,
        });
        Ok(SourceId {
            authority: self.uid,
            synthetic: false,
            index,
        })
    }

    pub fn execute_physical_press(
        &mut self,
        issuer: &IssuerHandle,
        source: SourceId,
        input: Input,
        to: Recipient,
    ) -> Result<Applied, RegistrationError> {
        self.check_issuer(issuer)?;
        self.check_physical_source(source)?;
        self.apply_press(source, input, to)
    }

    pub fn release_physical(
        &mut self,
        issuer: &IssuerHandle,
        source: SourceId,
        input: Input,
    ) -> Result<ReleaseOutcome, RegistrationError> {
        self.check_issuer(issuer)?;
        self.check_physical_source(source)?;
        Ok(self.release_inner(source, input))
    }

    pub fn begin_transition(
        &mut self,
        issuer: &IssuerHandle,
        publication: u64,
        epoch: u64,
    ) -> Result<(), RegistrationError> {
        self.check_issuer(issuer)?;
        self.pending_revision = Some((publication, epoch));
        Ok(())
    }

    pub fn publish(
        &mut self,
        issuer: &IssuerHandle,
        publication: u64,
        epoch: u64,
    ) -> Result<(), RegistrationError> {
        self.check_issuer(issuer)?;
        match self.pending_revision {
            // Only the revision the transition announced reopens routing. A
            // stale or unexpected publication would otherwise close a window
            // that is still open, and execution would resume against state the
            // transition had already moved past.
            Some((expected_publication, expected_epoch))
                if expected_publication == publication && expected_epoch == epoch =>
            {
                self.publication = publication;
                self.epoch = epoch;
                self.pending_revision = None;
                Ok(())
            }
            Some(_) => Err(RegistrationError::StaleExecution),
            None => Err(RegistrationError::StaleExecution),
        }
    }

    // ---- submission: capability validated ----------------------------------

    /// Validate and apply one synthetic press in a single step.
    pub fn execute_press(
        &mut self,
        submit: &SubmitHandle,
        capability: DeviceCapability,
        input: Input,
        context: ExecutionContext,
        to: Recipient,
    ) -> Result<Applied, RegistrationError> {
        self.check_submit(submit)?;
        if self.pending_revision.is_some() {
            return Err(RegistrationError::RoutingUnavailable);
        }
        if context.epoch != self.epoch || context.publication != self.publication {
            return Err(RegistrationError::StaleExecution);
        }
        let source = self.resolve(capability)?;
        if context.generation != capability.generation {
            return Err(RegistrationError::StaleGeneration);
        }
        self.apply_press(source, input, to)
    }

    /// Release a synthetic contribution. Capability validated, so a raw source
    /// from elsewhere cannot reach a hold here.
    pub fn release(
        &mut self,
        submit: &SubmitHandle,
        capability: DeviceCapability,
        input: Input,
    ) -> Result<ReleaseOutcome, RegistrationError> {
        self.check_submit(submit)?;
        let source = self.resolve(capability)?;
        Ok(self.release_inner(source, input))
    }

    // ---- shared internals --------------------------------------------------

    fn apply_press(
        &mut self,
        source: SourceId,
        input: Input,
        to: Recipient,
    ) -> Result<Applied, RegistrationError> {
        let width = self.capacity.synthetic_sources();
        let bit_index = source.holder_bit(width);
        let slot = input.slot();
        let hold = self
            .holds
            .get_mut(slot)
            .ok_or(RegistrationError::Capacity(CapacityError::NoHoldRecord))?;
        // The barrier holds only against a synthetic press that would reach the
        // SAME recipient and connection. A different recipient is not
        // implicated by this debt, and a physical source is never blocked: the
        // operator's keyboard must not stop because an injector owes a release.
        // Correctness for those cases rests on incarnation matching, which
        // discards a stale completion rather than letting it clear a new hold.
        if source.synthetic
            && let Some(blocked) = hold.awaiting_settlement
            && to.recipient == blocked.recipient
            && to.connection_generation == blocked.connection_generation
        {
            return Err(RegistrationError::ReleaseBarrier);
        }
        let bit = 1u64 << bit_index;
        if hold.holders & bit != 0 {
            let incarnation = hold
                .delivered_to
                .ok_or(RegistrationError::Capacity(CapacityError::NoHoldRecord))?;
            return Ok(Applied {
                source,
                input,
                incarnation,
            });
        }
        let first = hold.holders == 0;
        if first {
            // Minted here, never accepted from a caller: a fresh identity per
            // hold is what makes a stale completion fail to match.
            let hold_id = self
                .next_hold
                .checked_add(1)
                .ok_or(RegistrationError::Capacity(
                    CapacityError::IdentityExhausted,
                ))?;
            let incarnation = HoldIncarnation {
                recipient: to.recipient,
                connection_generation: to.connection_generation,
                input,
                hold: self.next_hold,
            };
            self.next_hold = hold_id;
            let hold = self
                .holds
                .get_mut(slot)
                .ok_or(RegistrationError::Capacity(CapacityError::NoHoldRecord))?;
            hold.holders |= bit;
            hold.delivered_to = Some(incarnation);
            return Ok(Applied {
                source,
                input,
                incarnation,
            });
        }
        hold.holders |= bit;
        Ok(Applied {
            source,
            input,
            incarnation: hold.delivered_to.expect("a held record has a recipient"),
        })
    }

    fn release_inner(&mut self, source: SourceId, input: Input) -> ReleaseOutcome {
        let width = self.capacity.synthetic_sources();
        let bit_index = source.holder_bit(width);
        let slot = input.slot();
        let Some(hold) = self.holds.get_mut(slot) else {
            return ReleaseOutcome::NotHeld;
        };
        let bit = 1u64 << bit_index;
        if hold.holders & bit == 0 {
            return ReleaseOutcome::NotHeld;
        }
        hold.holders &= !bit;
        if hold.holders != 0 {
            return ReleaseOutcome::SurvivorRemains;
        }
        let owed = hold.delivered_to.take();
        if let Some(incarnation) = owed {
            hold.awaiting_settlement = Some(incarnation);
        }
        let owner = self.owner_of(source);
        match owed {
            Some(incarnation) => {
                // The barrier is raised above; the debt must be recorded too.
                // A barrier without a debt record can never be cleared, which
                // blocks the input forever rather than protecting it. It goes
                // in the owner's own block, so one grant's unsettled release
                // cannot displace another's on the same input.
                if let Some(index) = self.debt_slot(owner, input)
                    && let Some(cell) = self.debts.get_mut(index)
                {
                    *cell = Some(DebtRecord {
                        incarnation,
                        settlement: SettlementBit::default(),
                    });
                }
                ReleaseOutcome::DeliverTo(incarnation)
            }
            None => ReleaseOutcome::NotHeld,
        }
    }

    fn retire_source_inner(&mut self, source: SourceId) -> RetiredDebt {
        let width = self.capacity.synthetic_sources();
        let bit = 1u64 << source.holder_bit(width);
        let owner = self.owner_of(source);
        let inputs = self.capacity.input_slots();
        let block = match owner {
            Some(grant) => usize::try_from(grant.0).unwrap_or(self.capacity.grants),
            None => self.capacity.grants,
        };
        let mut debt = RetiredDebt::default();
        // Two passes over fixed tables, no intermediate collection: the first
        // clears the holds and the second records their debts at computed
        // slots. Nothing here can allocate, which is the point.
        let mut owed = FixedOwed::default();
        for (slot, hold) in self.holds.iter_mut().enumerate() {
            if hold.holders & bit == 0 {
                continue;
            }
            hold.holders &= !bit;
            if hold.holders != 0 {
                debt.survivors += 1;
                continue;
            }
            if let Some(incarnation) = hold.delivered_to.take() {
                hold.awaiting_settlement = Some(incarnation);
                debt.owed_releases += 1;
                owed.push(slot, incarnation);
            }
        }
        let _ = (block, inputs);
        for index in 0..owed.len {
            let (slot, incarnation) = owed.entries[index];
            let Some(cell_index) = slot
                .checked_add(0)
                .and_then(|_| self.debt_slot_for_block(block, slot))
            else {
                continue;
            };
            if let Some(cell) = self.debts.get_mut(cell_index) {
                *cell = Some(DebtRecord {
                    incarnation,
                    settlement: SettlementBit::default(),
                });
            }
        }
        let table = if source.synthetic {
            &mut self.synthetic
        } else {
            &mut self.physical
        };
        if let Some(record) = table.get_mut(usize::from(source.index)) {
            // Stops every capability naming this source from validating again,
            // which is what keeps a retired injector from pressing once more.
            record.live = false;
        }
        debt
    }

    /// Frees the device records a reclaimed grant slot owned.
    fn release_devices_of(&mut self, grant: GrantId) {
        for record in &mut self.synthetic {
            if record.owner == Some(grant) {
                record.owner = None;
                record.live = false;
            }
        }
    }

    /// Record that a debt's obligations were met, clearing the barrier when
    /// both are.
    pub fn settle(
        &mut self,
        issuer: &IssuerHandle,
        owner: Option<GrantId>,
        input: Input,
        incarnation: HoldIncarnation,
        bit: SettlementBit,
    ) -> Result<bool, RegistrationError> {
        self.check_issuer(issuer)?;
        // Addressed by owner as well as input: two grants can owe a release on
        // the same input at once, and settling one must not clear the other.
        let Some(index) = self.debt_slot(owner, input) else {
            return Ok(false);
        };
        let slot = input.slot();
        let Some(Some(record)) = self.debts.get_mut(index) else {
            return Ok(false);
        };
        if record.incarnation != incarnation {
            // A late completion for an older hold. Discarded here rather than
            // allowed to clear a newer one.
            return Ok(false);
        }
        record.settlement.native_reconciled |= bit.native_reconciled;
        record.settlement.recipient_settled |= bit.recipient_settled;
        if record.settlement.is_settled() {
            self.debts[index] = None;
            if let Some(hold) = self.holds.get_mut(slot)
                && hold.awaiting_settlement == Some(incarnation)
            {
                hold.awaiting_settlement = None;
            }
            // The grant's slot becomes reusable once nothing it owned is owed.
            if let Some(grant) = owner
                && let Some(gslot) = self.grants.get_mut(grant.0 as usize)
            {
                gslot.outstanding = gslot.outstanding.saturating_sub(1);
            }
            return Ok(true);
        }
        Ok(false)
    }

    pub fn is_physical(&self, source: SourceId) -> bool {
        self.record(source)
            .is_some_and(|record| matches!(record.origin, Origin::Physical))
    }

    pub fn device_of(&self, source: SourceId) -> Option<DeviceId> {
        self.record(source).map(|record| record.device)
    }

    pub fn owner_of(&self, source: SourceId) -> Option<GrantId> {
        self.record(source).and_then(|record| record.owner)
    }
}
