//! The authority instance: one seat, one instance, every source in one place.

use crate::capacity::{Capacity, CapacityError};
use crate::grant::{GrantGeneration, GrantId, IssuerHandle, SubmitHandle};
use crate::identity::{DeviceCapability, HoldIncarnation, Input, Origin, SeatBinding, SourceId};
use crate::ledger::{Applied, ReleaseOutcome, SettlementBit};
use sophia_protocol::DeviceId;

/// Why a registration or submission was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationError {
    /// The capability names a different instance or seat than this authority.
    ForeignBinding,
    /// The capability's grant has been revoked or reissued since.
    StaleGeneration,
    /// The request was accepted under an epoch or publication that has moved.
    StaleExecution,
    /// Synthetic routing is unavailable: a transition is between commit and
    /// publication, so there is no current target to route against.
    RoutingUnavailable,
    /// A bound was reached.
    Capacity(CapacityError),
}

/// One registered source, immutable once created.
#[derive(Clone, Copy, Debug)]
struct SourceRecord {
    origin: Origin,
    owner: Option<GrantId>,
    generation: GrantGeneration,
    device: DeviceId,
    incarnation: u64,
}

/// One preallocated hold record.
#[derive(Clone, Copy, Debug, Default)]
struct HoldRecord {
    /// Sources currently contributing. Index into the source table.
    holders: u32,
    held: bool,
    delivered_to: Option<HoldIncarnation>,
    debt: Option<SettlementBit>,
}

/// One seat's input authority inside one instance.
///
/// Construction fixes the binding and the capacity, and verifies the advertised
/// button domain against what it preallocated for. Nothing later can widen it.
pub struct AuthorityInstance {
    binding: SeatBinding,
    capacity: Capacity,
    sources: Vec<SourceRecord>,
    holds: Vec<HoldRecord>,
    grants_in_use: usize,
    next_generation: u64,
    next_hold: u64,
    /// Set while a focus, seat or security transition is between commit and
    /// publication. Execution refuses rather than using the previous snapshot.
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
        let instance = Self {
            binding,
            capacity,
            sources: Vec::with_capacity(capacity.grants * capacity.devices_per_grant),
            holds: vec![HoldRecord::default(); capacity.hold_records()],
            grants_in_use: 0,
            next_generation: 1,
            next_hold: 1,
            routing_unavailable: false,
            publication: 0,
            epoch: 0,
        };
        Ok((instance, IssuerHandle { binding }, SubmitHandle { binding }))
    }

    fn hold_index(&self, input: Input) -> usize {
        match input {
            Input::Key(code) => usize::from(code),
            Input::Button(button) => self.capacity.keys + usize::from(button),
        }
    }
}

/// What one press execution is accepted under.
///
/// Passed whole to [`AuthorityInstance::execute_press`] rather than checked by
/// a separate call, so there is no window between deciding a request is valid
/// and acting on it.
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
    /// Holds this source was the last holder of; each owes a release.
    pub owed_releases: usize,
    /// Holds where a survivor remains; nothing is owed for these.
    pub survivors: usize,
}

impl AuthorityInstance {
    /// Issue a grant. Issuer only.
    pub fn issue_grant(
        &mut self,
        issuer: &IssuerHandle,
    ) -> Result<(GrantId, GrantGeneration), RegistrationError> {
        if issuer.binding != self.binding {
            return Err(RegistrationError::ForeignBinding);
        }
        if self.grants_in_use >= self.capacity.grants {
            return Err(RegistrationError::Capacity(CapacityError::NoGrantSlot));
        }
        let id = GrantId(u32::try_from(self.grants_in_use).unwrap_or(u32::MAX));
        let generation = GrantGeneration(self.next_generation);
        self.next_generation = self.next_generation.saturating_add(1);
        self.grants_in_use += 1;
        Ok((id, generation))
    }

    /// Register a synthetic device for a grant and return its capability.
    ///
    /// Issuer only: an adapter receives capabilities and cannot mint one, so a
    /// forged or cross-instance device is not a check that can be skipped but
    /// a value that cannot be constructed.
    pub fn allocate_device(
        &mut self,
        issuer: &IssuerHandle,
        grant: GrantId,
        generation: GrantGeneration,
        device: DeviceId,
    ) -> Result<DeviceCapability, RegistrationError> {
        if issuer.binding != self.binding {
            return Err(RegistrationError::ForeignBinding);
        }
        let source = SourceId::from_index(u32::try_from(self.sources.len()).unwrap_or(u32::MAX));
        self.sources.push(SourceRecord {
            origin: Origin::Synthetic,
            owner: Some(grant),
            generation,
            device,
            incarnation: 0,
        });
        Ok(DeviceCapability {
            source,
            binding: self.binding,
            grant,
            generation,
            device,
        })
    }

    /// Register a physical source. Issuer only, and never reachable from an
    /// adapter's handle.
    pub fn register_physical(
        &mut self,
        issuer: &IssuerHandle,
        device: DeviceId,
    ) -> Result<SourceId, RegistrationError> {
        if issuer.binding != self.binding {
            return Err(RegistrationError::ForeignBinding);
        }
        let source = SourceId::from_index(u32::try_from(self.sources.len()).unwrap_or(u32::MAX));
        self.sources.push(SourceRecord {
            origin: Origin::Physical,
            owner: None,
            generation: GrantGeneration(0),
            device,
            incarnation: 0,
        });
        Ok(source)
    }

    /// Validate and apply one press in a single step.
    ///
    /// There is deliberately no public `validate`. A caller that could check
    /// first and apply second would hold a decision across a window in which
    /// revocation, an epoch advance or a publication transition could land,
    /// and the whole point of the guard is that those cannot interleave here.
    pub fn execute_press(
        &mut self,
        capability: DeviceCapability,
        input: Input,
        context: ExecutionContext,
        deliver_to: impl FnOnce() -> HoldIncarnation,
    ) -> Result<Applied, RegistrationError> {
        if capability.binding != self.binding {
            return Err(RegistrationError::ForeignBinding);
        }
        if self.routing_unavailable {
            return Err(RegistrationError::RoutingUnavailable);
        }
        if context.epoch != self.epoch || context.publication != self.publication {
            return Err(RegistrationError::StaleExecution);
        }
        let record = self
            .sources
            .get(capability.source.index() as usize)
            .ok_or(RegistrationError::ForeignBinding)?;
        if record.generation != capability.generation || context.generation != capability.generation
        {
            return Err(RegistrationError::StaleGeneration);
        }

        self.apply_press(capability.source, input, deliver_to)
    }

    /// Apply a press from a physical source.
    ///
    /// Issuer only, because a physical source belongs to no grant and so has no
    /// capability to present. Session owns physical ingress and is the only
    /// holder of an issuer, which is what keeps an adapter from claiming to be
    /// a keyboard.
    pub fn execute_physical_press(
        &mut self,
        issuer: &IssuerHandle,
        source: SourceId,
        input: Input,
        deliver_to: impl FnOnce() -> HoldIncarnation,
    ) -> Result<Applied, RegistrationError> {
        if issuer.binding != self.binding {
            return Err(RegistrationError::ForeignBinding);
        }
        if !self.is_physical(source) {
            return Err(RegistrationError::ForeignBinding);
        }
        self.apply_press(source, input, deliver_to)
    }

    fn apply_press(
        &mut self,
        source: SourceId,
        input: Input,
        deliver_to: impl FnOnce() -> HoldIncarnation,
    ) -> Result<Applied, RegistrationError> {
        let index = self.hold_index(input);
        let hold = self
            .holds
            .get_mut(index)
            .ok_or(RegistrationError::Capacity(CapacityError::NoHoldRecord))?;

        let bit = 1u32 << (source.index() % 32);
        if hold.holders & bit != 0 {
            // Duplicate press from a source already holding: not a second hold
            // and not an error. It reports the recipient the hold already has,
            // and never invents one, because a duplicate delivers nothing.
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
        hold.held = true;
        if first {
            let incarnation = deliver_to();
            hold.delivered_to = Some(incarnation);
            hold.debt = Some(SettlementBit::default());
        }
        Ok(Applied {
            source,
            input,
            incarnation: hold.delivered_to.expect("a held record has a recipient"),
        })
    }

    /// Release one source's contribution.
    pub fn release(&mut self, source: SourceId, input: Input) -> ReleaseOutcome {
        let index = self.hold_index(input);
        let Some(hold) = self.holds.get_mut(index) else {
            return ReleaseOutcome::NotHeld;
        };
        let bit = 1u32 << (source.index() % 32);
        if hold.holders & bit == 0 {
            return ReleaseOutcome::NotHeld;
        }
        hold.holders &= !bit;
        if hold.holders != 0 {
            return ReleaseOutcome::SurvivorRemains;
        }
        hold.held = false;
        match hold.delivered_to.take() {
            Some(incarnation) => ReleaseOutcome::DeliverTo(incarnation),
            None => ReleaseOutcome::NotHeld,
        }
    }

    /// Retire every contribution a source holds, without allocating.
    ///
    /// Marks the preallocated records and reports counts. The attempt
    /// scheduler walks the marked population afterwards; nothing here builds a
    /// list, because revocation must not need memory it might not get.
    pub fn retire_source(&mut self, source: SourceId) -> RetiredDebt {
        let bit = 1u32 << (source.index() % 32);
        let mut debt = RetiredDebt::default();
        for hold in &mut self.holds {
            if hold.holders & bit == 0 {
                continue;
            }
            hold.holders &= !bit;
            if hold.holders != 0 {
                debt.survivors += 1;
                continue;
            }
            hold.held = false;
            if hold.delivered_to.is_some() {
                debt.owed_releases += 1;
            }
        }
        debt
    }

    /// Mark synthetic routing unavailable while a transition is in flight.
    pub fn begin_transition(&mut self) {
        self.routing_unavailable = true;
    }

    /// Install the matching publication and re-enable routing.
    pub fn publish(&mut self, publication: u64, epoch: u64) {
        self.publication = publication;
        self.epoch = epoch;
        self.routing_unavailable = false;
    }
}

impl AuthorityInstance {
    /// Whether a source is physical.
    ///
    /// The emergency recognizer consumes physical transitions only, and this is
    /// how it asks. A predicate over the registered origin rather than a filter
    /// applied per event, so an overlapping synthetic hold cannot mask a
    /// physical press by making the aggregate look unchanged.
    pub fn is_physical(&self, source: SourceId) -> bool {
        self.sources
            .get(source.index() as usize)
            .is_some_and(|record| matches!(record.origin, Origin::Physical))
    }

    /// The packet key registered for a source.
    pub fn device_of(&self, source: SourceId) -> Option<DeviceId> {
        self.sources
            .get(source.index() as usize)
            .map(|record| record.device)
    }

    /// Which grant owns a source, if any. Physical sources are owned by none.
    pub fn owner_of(&self, source: SourceId) -> Option<GrantId> {
        self.sources
            .get(source.index() as usize)
            .and_then(|record| record.owner)
    }

    /// Allocate the next hold identity.
    fn next_hold_identity(&mut self) -> u64 {
        let hold = self.next_hold;
        self.next_hold = self.next_hold.saturating_add(1);
        hold
    }

    /// Build the incarnation for a delivery, consuming a hold identity.
    pub fn incarnate(
        &mut self,
        recipient: u64,
        connection_generation: u64,
        input: Input,
    ) -> HoldIncarnation {
        HoldIncarnation {
            recipient,
            connection_generation,
            input,
            hold: self.next_hold_identity(),
        }
    }

    /// Bump a source's incarnation counter when its device is reissued.
    pub fn reincarnate_source(&mut self, source: SourceId) -> Option<u64> {
        let record = self.sources.get_mut(source.index() as usize)?;
        record.incarnation = record.incarnation.saturating_add(1);
        Some(record.incarnation)
    }
}
