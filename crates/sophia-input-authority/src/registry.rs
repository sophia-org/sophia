//! One seat's authority. All mutation requires exclusive access supplied by the
//! integration guard; this crate never acquires an adapter or runtime lock.

mod attempts;
use attempts::AttemptRecord;
pub use attempts::{AttemptClaim, AttemptToken};
mod execution;
mod requests;
use requests::RequestCell;
pub use requests::{ExecutionPermit, RequestCompletion, RequestToken};

use crate::capacity::{Capacity, CapacityError};
use crate::grant::{GrantGeneration, GrantId, IssuerHandle, SubmitHandle};
use crate::identity::{
    AuthorityUid, ConnectionIdentity, DeviceCapability, HoldIncarnation, Input, Origin, Recipient,
    SeatBinding, SourceId,
};
use crate::ledger::{Applied, ReleaseOutcome, SettlementBit};
use sophia_protocol::DeviceId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationError {
    ForeignAuthority,
    StaleGeneration,
    StaleExecution,
    RoutingUnavailable,
    ReleaseBarrier,
    StaleRequest,
    RequestConsumed,
    WrongConnection,
    Capacity(CapacityError),
}

/// References count source participation in retained incarnations, not just
/// releases created by revoke. A grant joining another source's hold retains
/// its slot until that incarnation settles, even if the other source survives.
#[derive(Clone, Copy, Debug)]
struct GrantSlot {
    id: GrantId,
    control_epoch: u64,
    connection: ConnectionIdentity,
    live: bool,
    references: usize,
    request: Option<RequestCell>,
}

#[derive(Clone, Copy, Debug)]
struct SourceRecord {
    id: SourceId,
    owner: Option<GrantId>,
    device: DeviceId,
    live: bool,
    references: usize,
}

/// Allocated from a fixed pool BEFORE the first press applies. The same record
/// remains after the final release; retirement never needs a new allocation.
/// Participants retain source identities after they stop holding, so recycling
/// a source index cannot change either provenance or the grant's reference count.
#[derive(Clone, Copy, Debug)]
struct HoldRecord {
    incarnation: HoldIncarnation,
    holders: u64,
    participants: u64,
    settlement: SettlementBit,
    attempt: Option<AttemptToken>,
}

pub struct AuthorityInstance {
    uid: AuthorityUid,
    binding: SeatBinding,
    capacity: Capacity,
    synthetic: Vec<SourceRecord>,
    physical: Vec<SourceRecord>,
    active: Vec<Option<usize>>,
    records: Vec<Option<HoldRecord>>,
    attempts: Vec<Option<AttemptRecord>>,
    grants: Vec<GrantSlot>,
    next_identity: u64,
    pending_revision: Option<(u64, u64)>,
    publication: u64,
    epoch: u64,
}

/// The generation and publication captured by a queued request. The consumer
/// must supply that original context, never restamp it when it becomes runnable.
#[derive(Clone, Copy, Debug)]
pub struct ExecutionContext {
    pub generation: GrantGeneration,
    pub connection: ConnectionIdentity,
    pub epoch: u64,
    pub publication: u64,
    pub request: u64,
}

/// The committed revision read while the caller holds the common guard.
/// This is a construction/observation value, not a capability or a promise
/// that the revision will remain current after that guard is released.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublishedRevision {
    pub control_epoch: u64,
    pub publication: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RetiredDebt {
    pub owed_releases: usize,
    pub survivors: usize,
}

impl AuthorityInstance {
    /// Derive a coordinator's initial state from its authority. Never expose
    /// the previous publication as usable while a transition is in progress.
    pub fn published_revision(
        &self,
        issuer: &IssuerHandle,
    ) -> Result<PublishedRevision, RegistrationError> {
        self.check_issuer(issuer)?;
        if self.pending_revision.is_some() {
            return Err(RegistrationError::RoutingUnavailable);
        }
        Ok(PublishedRevision {
            control_epoch: self.epoch,
            publication: self.publication,
        })
    }

    pub fn new(
        binding: SeatBinding,
        capacity: Capacity,
        advertised_buttons: u16,
    ) -> Result<(Self, IssuerHandle, SubmitHandle), CapacityError> {
        capacity.verify(advertised_buttons)?;
        let uid = AuthorityUid::allocate().ok_or(CapacityError::IdentityExhausted)?;
        let instance = Self {
            uid,
            binding,
            capacity,
            synthetic: Vec::with_capacity(capacity.synthetic_sources()),
            physical: Vec::with_capacity(capacity.physical_sources),
            active: vec![None; capacity.input_slots()],
            records: vec![
                None;
                capacity.debt_records()
                    + capacity.physical_sources * capacity.input_slots()
            ],
            attempts: vec![None; capacity.attempts],
            grants: Vec::with_capacity(capacity.grants),
            next_identity: 1,
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

    fn identity(&mut self) -> Result<u64, RegistrationError> {
        let result = self.next_identity;
        self.next_identity = result.checked_add(1).ok_or(RegistrationError::Capacity(
            CapacityError::IdentityExhausted,
        ))?;
        Ok(result)
    }

    fn check_issuer(&self, issuer: &IssuerHandle) -> Result<(), RegistrationError> {
        if issuer.authority() == self.uid {
            Ok(())
        } else {
            Err(RegistrationError::ForeignAuthority)
        }
    }

    fn check_submit(&self, submit: &SubmitHandle) -> Result<(), RegistrationError> {
        if submit.authority() == self.uid {
            Ok(())
        } else {
            Err(RegistrationError::ForeignAuthority)
        }
    }

    fn grant(&self, id: GrantId) -> Result<&GrantSlot, RegistrationError> {
        if id.authority != self.uid {
            return Err(RegistrationError::ForeignAuthority);
        }
        self.grants
            .get(id.slot)
            .filter(|slot| slot.id == id)
            .ok_or(RegistrationError::StaleGeneration)
    }

    fn record(&self, source: SourceId) -> Option<&SourceRecord> {
        if source.authority != self.uid {
            return None;
        }
        let table = if source.synthetic {
            &self.synthetic
        } else {
            &self.physical
        };
        table
            .get(usize::from(source.index))
            .filter(|record| record.id == source)
    }

    fn record_mut(&mut self, source: SourceId) -> &mut SourceRecord {
        // Internal callers have already validated a source or retain its exact
        // identity through a pool reference. Never exposed as a raw-index API.
        let table = if source.synthetic {
            &mut self.synthetic
        } else {
            &mut self.physical
        };
        let record = &mut table[usize::from(source.index)];
        assert_eq!(record.id, source);
        record
    }

    fn source_at_bit(&self, bit: usize) -> SourceId {
        if bit < self.capacity.synthetic_sources() {
            self.synthetic[bit].id
        } else {
            self.physical[bit - self.capacity.synthetic_sources()].id
        }
    }

    fn resolve(&self, cap: DeviceCapability) -> Result<SourceId, RegistrationError> {
        if cap.authority != self.uid || cap.binding != self.binding {
            return Err(RegistrationError::ForeignAuthority);
        }
        let grant = self.grant(cap.grant)?;
        if grant.connection != cap.connection {
            return Err(RegistrationError::WrongConnection);
        }
        if !grant.live || cap.generation != grant.id.generation || grant.control_epoch != self.epoch
        {
            return Err(RegistrationError::StaleGeneration);
        }
        let record = self
            .record(cap.source)
            .ok_or(RegistrationError::StaleGeneration)?;
        if !record.live || record.owner != Some(cap.grant) || record.device != cap.device {
            return Err(RegistrationError::StaleGeneration);
        }
        Ok(record.id)
    }

    fn validate_execution(
        &self,
        submit: &SubmitHandle,
        cap: DeviceCapability,
        context: ExecutionContext,
    ) -> Result<SourceId, RegistrationError> {
        self.check_submit(submit)?;
        if self.pending_revision.is_some() {
            return Err(RegistrationError::RoutingUnavailable);
        }
        if context.epoch != self.epoch || context.publication != self.publication {
            return Err(RegistrationError::StaleExecution);
        }
        if context.connection != cap.connection {
            return Err(RegistrationError::WrongConnection);
        }
        if context.generation != cap.generation {
            return Err(RegistrationError::StaleGeneration);
        }
        self.resolve(cap)
    }

    fn check_physical(&self, source: SourceId) -> Result<(), RegistrationError> {
        if !source.synthetic && self.record(source).is_some_and(|record| record.live) {
            Ok(())
        } else {
            Err(RegistrationError::ForeignAuthority)
        }
    }

    pub fn issue_grant(
        &mut self,
        issuer: &IssuerHandle,
        connection: ConnectionIdentity,
    ) -> Result<(GrantId, GrantGeneration), RegistrationError> {
        self.check_issuer(issuer)?;
        if self.pending_revision.is_some() {
            return Err(RegistrationError::RoutingUnavailable);
        }
        let index = self
            .grants
            .iter()
            .position(|slot| !slot.live && slot.references == 0 && slot.request.is_none())
            .unwrap_or(self.grants.len());
        if index == self.capacity.grants {
            return Err(RegistrationError::Capacity(CapacityError::NoGrantSlot));
        }
        let generation = GrantGeneration(self.identity()?);
        let id = GrantId {
            authority: self.uid,
            slot: index,
            generation,
        };
        let slot = GrantSlot {
            id,
            control_epoch: self.epoch,
            connection,
            live: true,
            references: 0,
            request: None,
        };
        if index == self.grants.len() {
            self.grants.push(slot);
        } else {
            self.grants[index] = slot;
        }
        Ok((id, generation))
    }

    pub fn allocate_device(
        &mut self,
        issuer: &IssuerHandle,
        grant: GrantId,
        generation: GrantGeneration,
        device: DeviceId,
    ) -> Result<DeviceCapability, RegistrationError> {
        self.check_issuer(issuer)?;
        let slot = self.grant(grant)?;
        if !slot.live || generation != grant.generation {
            return Err(RegistrationError::StaleGeneration);
        }
        let connection = slot.connection;
        let owned = self
            .synthetic
            .iter()
            .filter(|r| r.owner == Some(grant))
            .count();
        if owned >= self.capacity.devices_per_grant {
            return Err(RegistrationError::Capacity(CapacityError::NoDeviceSlot));
        }
        let index = self
            .synthetic
            .iter()
            .position(|r| !r.live && r.references == 0)
            .unwrap_or(self.synthetic.len());
        if index == self.capacity.synthetic_sources() {
            return Err(RegistrationError::Capacity(CapacityError::NoDeviceSlot));
        }
        let source = SourceId {
            authority: self.uid,
            synthetic: true,
            index: index as u16,
            incarnation: self.identity()?,
        };
        let record = SourceRecord {
            id: source,
            owner: Some(grant),
            device,
            live: true,
            references: 0,
        };
        if index == self.synthetic.len() {
            self.synthetic.push(record);
        } else {
            self.synthetic[index] = record;
        }
        Ok(DeviceCapability {
            source,
            binding: self.binding,
            authority: self.uid,
            grant,
            generation,
            device,
            connection,
        })
    }

    pub fn register_physical(
        &mut self,
        issuer: &IssuerHandle,
        device: DeviceId,
    ) -> Result<SourceId, RegistrationError> {
        self.check_issuer(issuer)?;
        let index = self
            .physical
            .iter()
            .position(|r| !r.live && r.references == 0)
            .unwrap_or(self.physical.len());
        if index == self.capacity.physical_sources {
            return Err(RegistrationError::Capacity(CapacityError::NoPhysicalSlot));
        }
        let source = SourceId {
            authority: self.uid,
            synthetic: false,
            index: index as u16,
            incarnation: self.identity()?,
        };
        let record = SourceRecord {
            id: source,
            owner: None,
            device,
            live: true,
            references: 0,
        };
        if index == self.physical.len() {
            self.physical.push(record);
        } else {
            self.physical[index] = record;
        }
        Ok(source)
    }

    /// Revokes the exact issued generation. A delayed revoke cannot affect a
    /// replacement occupying the same numeric slot. Existing debt is retained.
    pub fn revoke_grant(
        &mut self,
        issuer: &IssuerHandle,
        grant: GrantId,
    ) -> Result<RetiredDebt, RegistrationError> {
        self.check_issuer(issuer)?;
        self.grant(grant)?;
        self.grants[grant.slot].live = false;
        if let Some(cell) = self.grants[grant.slot].request.as_mut()
            && cell.completion.is_none()
        {
            cell.completion = Some(RequestCompletion::Cancelled);
        }
        let mut debt = RetiredDebt::default();
        for index in 0..self.synthetic.len() {
            let record = self.synthetic[index];
            if record.owner == Some(grant) && record.live {
                let retired = self.retire_source(record.id);
                debt.owed_releases += retired.owed_releases;
                debt.survivors += retired.survivors;
            }
        }
        Ok(debt)
    }

    /// Physical unplug is privileged cleanup, independent of publication or a
    /// synthetic grant. It still retains both reconciliation obligations.
    pub fn retire_physical(
        &mut self,
        issuer: &IssuerHandle,
        source: SourceId,
    ) -> Result<RetiredDebt, RegistrationError> {
        self.check_issuer(issuer)?;
        self.check_physical(source)?;
        Ok(self.retire_source(source))
    }

    pub fn begin_transition(
        &mut self,
        issuer: &IssuerHandle,
        publication: u64,
        epoch: u64,
    ) -> Result<(), RegistrationError> {
        self.check_issuer(issuer)?;
        let floor = self
            .pending_revision
            .unwrap_or((self.publication, self.epoch));
        if publication <= floor.0 || epoch < floor.1 {
            return Err(RegistrationError::StaleExecution);
        }
        self.pending_revision = Some((publication, epoch));
        // A pending operation never resumes across a changed publication. Its
        // cancellation remains observable even if the wake coalesces.
        for slot in &mut self.grants {
            if let Some(cell) = slot.request.as_mut()
                && cell.completion.is_none()
            {
                cell.completion = Some(RequestCompletion::Cancelled);
            }
        }
        // Security-control epochs revoke authority, not just queued requests.
        // No fresh request may launder an old capability by restamping context.
        if epoch != self.epoch {
            for index in 0..self.grants.len() {
                if self.grants[index].live {
                    self.revoke_grant(issuer, self.grants[index].id)?;
                }
            }
        }
        Ok(())
    }

    pub fn publish(
        &mut self,
        issuer: &IssuerHandle,
        publication: u64,
        epoch: u64,
    ) -> Result<(), RegistrationError> {
        self.check_issuer(issuer)?;
        if self.pending_revision != Some((publication, epoch)) {
            return Err(RegistrationError::StaleExecution);
        }
        self.publication = publication;
        self.epoch = epoch;
        self.pending_revision = None;
        Ok(())
    }

    pub fn is_physical(&self, source: SourceId) -> bool {
        !source.synthetic && self.record(source).is_some_and(|record| record.live)
    }

    pub fn device_of(&self, source: SourceId) -> Option<DeviceId> {
        self.record(source).map(|record| record.device)
    }

    pub fn owner_of(&self, source: SourceId) -> Option<GrantId> {
        self.record(source).and_then(|record| record.owner)
    }

    pub fn origin_of(&self, source: SourceId) -> Option<Origin> {
        self.record(source).map(|_| {
            if source.synthetic {
                Origin::Synthetic
            } else {
                Origin::Physical
            }
        })
    }
}
