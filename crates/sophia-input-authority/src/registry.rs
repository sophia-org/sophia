//! The authority instance: one seat, one instance, every source in one place.

use crate::capacity::{Capacity, CapacityError};
use crate::grant::{GrantGeneration, GrantId, IssuerHandle, SubmitHandle};
use crate::identity::{
    AuthorityUid, DeviceCapability, HoldIncarnation, Input, Origin, SeatBinding, SourceId,
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
    /// An earlier incarnation whose release has not settled. A new hold on this
    /// input is refused until it clears, so a stale release cannot arrive after
    /// a newer press and clear it.
    awaiting_settlement: Option<HoldIncarnation>,
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
    grants: Vec<GrantGeneration>,
    next_generation: u64,
    next_hold: u64,
    routing_unavailable: bool,
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
        let uid = AuthorityUid::allocate();
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
            routing_unavailable: false,
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
        if self.grants.len() >= self.capacity.grants {
            return Err(RegistrationError::Capacity(CapacityError::NoGrantSlot));
        }
        let id = GrantId(u32::try_from(self.grants.len()).unwrap_or(u32::MAX));
        let generation = GrantGeneration(self.next_generation);
        self.next_generation = self.next_generation.saturating_add(1);
        self.grants.push(generation);
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
        let doomed: Vec<SourceId> = self
            .synthetic
            .iter()
            .enumerate()
            .filter(|(_, record)| record.owner == Some(grant) && record.live)
            .map(|(index, _)| SourceId {
                synthetic: true,
                index: u16::try_from(index).unwrap_or(u16::MAX),
            })
            .collect();
        for source in doomed {
            let retired = self.retire_source_inner(source);
            debt.owed_releases += retired.owed_releases;
            debt.survivors += retired.survivors;
        }
        if let Some(slot) = self.grants.get_mut(grant.0 as usize) {
            // A new generation for this slot, so nothing minted under the old
            // one validates even if it is presented a moment later.
            self.next_generation = self.next_generation.saturating_add(1);
            *slot = GrantGeneration(self.next_generation);
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
        let owned = self
            .synthetic
            .iter()
            .filter(|record| record.owner == Some(grant) && record.live)
            .count();
        if owned >= self.capacity.devices_per_grant {
            return Err(RegistrationError::Capacity(CapacityError::NoDeviceSlot));
        }
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
        Ok(DeviceCapability {
            source: SourceId {
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
            synthetic: false,
            index,
        })
    }

    pub fn execute_physical_press(
        &mut self,
        issuer: &IssuerHandle,
        source: SourceId,
        input: Input,
        deliver_to: impl FnOnce() -> HoldIncarnation,
    ) -> Result<Applied, RegistrationError> {
        self.check_issuer(issuer)?;
        if source.synthetic {
            return Err(RegistrationError::ForeignAuthority);
        }
        self.apply_press(source, input, deliver_to)
    }

    pub fn release_physical(
        &mut self,
        issuer: &IssuerHandle,
        source: SourceId,
        input: Input,
    ) -> Result<ReleaseOutcome, RegistrationError> {
        self.check_issuer(issuer)?;
        if source.synthetic {
            return Err(RegistrationError::ForeignAuthority);
        }
        Ok(self.release_inner(source, input))
    }

    pub fn begin_transition(&mut self, issuer: &IssuerHandle) -> Result<(), RegistrationError> {
        self.check_issuer(issuer)?;
        self.routing_unavailable = true;
        Ok(())
    }

    pub fn publish(
        &mut self,
        issuer: &IssuerHandle,
        publication: u64,
        epoch: u64,
    ) -> Result<(), RegistrationError> {
        self.check_issuer(issuer)?;
        self.publication = publication;
        self.epoch = epoch;
        self.routing_unavailable = false;
        Ok(())
    }

    // ---- submission: capability validated ----------------------------------

    /// Validate and apply one synthetic press in a single step.
    pub fn execute_press(
        &mut self,
        submit: &SubmitHandle,
        capability: DeviceCapability,
        input: Input,
        context: ExecutionContext,
        deliver_to: impl FnOnce() -> HoldIncarnation,
    ) -> Result<Applied, RegistrationError> {
        self.check_submit(submit)?;
        if self.routing_unavailable {
            return Err(RegistrationError::RoutingUnavailable);
        }
        if context.epoch != self.epoch || context.publication != self.publication {
            return Err(RegistrationError::StaleExecution);
        }
        let source = self.resolve(capability)?;
        if context.generation != capability.generation {
            return Err(RegistrationError::StaleGeneration);
        }
        self.apply_press(source, input, deliver_to)
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
        deliver_to: impl FnOnce() -> HoldIncarnation,
    ) -> Result<Applied, RegistrationError> {
        let width = self.capacity.synthetic_sources();
        let bit_index = source.holder_bit(width);
        let slot = input.slot();
        let hold = self
            .holds
            .get_mut(slot)
            .ok_or(RegistrationError::Capacity(CapacityError::NoHoldRecord))?;
        if hold.awaiting_settlement.is_some() {
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
        hold.holders |= bit;
        if first {
            hold.delivered_to = Some(deliver_to());
        }
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
        match owed {
            Some(incarnation) => {
                // The barrier is raised above; the debt must be recorded too.
                // A barrier without a debt record can never be cleared, which
                // blocks the input forever rather than protecting it.
                if let Some(cell) = self.debts.get_mut(slot) {
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
        let mut debt = RetiredDebt::default();
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
                if let Some(cell) = self.debts.get_mut(slot) {
                    *cell = Some(DebtRecord {
                        incarnation,
                        settlement: SettlementBit::default(),
                    });
                }
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

    /// Record that a debt's obligations were met, clearing the barrier when
    /// both are.
    pub fn settle(
        &mut self,
        issuer: &IssuerHandle,
        input: Input,
        incarnation: HoldIncarnation,
        bit: SettlementBit,
    ) -> Result<bool, RegistrationError> {
        self.check_issuer(issuer)?;
        let slot = input.slot();
        let Some(Some(record)) = self.debts.get_mut(slot) else {
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
            self.debts[slot] = None;
            if let Some(hold) = self.holds.get_mut(slot) {
                hold.awaiting_settlement = None;
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

    pub fn incarnate(
        &mut self,
        recipient: u64,
        connection_generation: u64,
        input: Input,
    ) -> HoldIncarnation {
        let hold = self.next_hold;
        self.next_hold = self.next_hold.saturating_add(1);
        HoldIncarnation {
            recipient,
            connection_generation,
            input,
            hold,
        }
    }
}
