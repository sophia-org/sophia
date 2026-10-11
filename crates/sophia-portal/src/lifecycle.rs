use std::collections::BTreeMap;

use sophia_protocol::{
    NamespaceId, PortalDecision, PortalGrant, PortalGrantState, PortalRequest, PortalTransfer,
    PortalTransferId,
};

pub const PORTAL_ACTIVE_TRANSFER_CAPACITY: usize = 64;
pub const PORTAL_DEFAULT_DEADLINE_MSEC: u64 = 2_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortalPolicyDecision {
    Allow,
    Deny,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PortalLifecycleError {
    InvalidRequest,
    DeadlineElapsed,
    CapacityExceeded,
    DuplicateTransfer,
    UnknownTransfer,
    NotPending,
    GrantNotActive,
    StaleSourceGeneration,
    InvalidBrokerGeneration,
}

#[derive(Debug)]
pub struct PortalRequestGrantLifecycle {
    broker_generation: u64,
    capacity: usize,
    requests: BTreeMap<PortalTransferId, PortalRequest>,
    grants: BTreeMap<PortalTransferId, PortalGrant>,
}

impl PortalRequestGrantLifecycle {
    pub fn new(broker_generation: u64) -> Result<Self, PortalLifecycleError> {
        Self::with_capacity(broker_generation, PORTAL_ACTIVE_TRANSFER_CAPACITY)
    }

    pub fn with_capacity(
        broker_generation: u64,
        capacity: usize,
    ) -> Result<Self, PortalLifecycleError> {
        if broker_generation == 0 || capacity == 0 {
            return Err(PortalLifecycleError::InvalidBrokerGeneration);
        }
        Ok(Self {
            broker_generation,
            capacity,
            requests: BTreeMap::new(),
            grants: BTreeMap::new(),
        })
    }

    pub fn submit(
        &mut self,
        request: PortalRequest,
        now_msec: u64,
    ) -> Result<(), PortalLifecycleError> {
        if !valid_request(&request) {
            return Err(PortalLifecycleError::InvalidRequest);
        }
        if request.deadline_msec <= now_msec {
            return Err(PortalLifecycleError::DeadlineElapsed);
        }
        if self.requests.contains_key(&request.transfer.transfer) {
            return Err(PortalLifecycleError::DuplicateTransfer);
        }
        if self.pending_or_active_count() >= self.capacity {
            return Err(PortalLifecycleError::CapacityExceeded);
        }
        self.requests.insert(request.transfer.transfer, request);
        Ok(())
    }

    /// Admit and decide a new request atomically. File owners reserve their
    /// journal first; a refused precondition must not leave a Pending request
    /// behind when that reservation is dropped.
    pub fn submit_decided(
        &mut self,
        mut request: PortalRequest,
        decision: PortalPolicyDecision,
        source_generation: u64,
        now_msec: u64,
    ) -> Result<Option<PortalGrant>, PortalLifecycleError> {
        if !valid_request(&request) {
            return Err(PortalLifecycleError::InvalidRequest);
        }
        if request.deadline_msec <= now_msec {
            return Err(PortalLifecycleError::DeadlineElapsed);
        }
        if request.transfer.generation != source_generation {
            return Err(PortalLifecycleError::StaleSourceGeneration);
        }
        if self.requests.contains_key(&request.transfer.transfer) {
            return Err(PortalLifecycleError::DuplicateTransfer);
        }
        if self.pending_or_active_count() >= self.capacity {
            return Err(PortalLifecycleError::CapacityExceeded);
        }
        let grant = if decision == PortalPolicyDecision::Allow {
            request.transfer.decision = PortalDecision::Allowed;
            Some(PortalGrant {
                transfer: request.transfer.transfer,
                source_namespace: request.transfer.source_namespace,
                target_namespace: request.transfer.target_namespace,
                kind: request.transfer.kind,
                source_generation,
                broker_generation: self.broker_generation,
                deadline_msec: request.deadline_msec,
                state: PortalGrantState::Active,
            })
        } else {
            request.transfer.decision = PortalDecision::Denied;
            None
        };
        self.requests.insert(request.transfer.transfer, request);
        if let Some(grant) = &grant {
            self.grants.insert(grant.transfer, grant.clone());
        }
        Ok(grant)
    }

    pub fn decide(
        &mut self,
        transfer: PortalTransferId,
        decision: PortalPolicyDecision,
        source_generation: u64,
        now_msec: u64,
    ) -> Result<Option<&PortalGrant>, PortalLifecycleError> {
        let request = self
            .requests
            .get_mut(&transfer)
            .ok_or(PortalLifecycleError::UnknownTransfer)?;
        if request.transfer.decision != PortalDecision::Pending {
            return Err(PortalLifecycleError::NotPending);
        }
        if request.deadline_msec <= now_msec {
            request.transfer.decision = PortalDecision::Revoked;
            return Err(PortalLifecycleError::DeadlineElapsed);
        }
        if request.transfer.generation != source_generation {
            request.transfer.decision = PortalDecision::Revoked;
            return Err(PortalLifecycleError::StaleSourceGeneration);
        }
        match decision {
            PortalPolicyDecision::Deny => {
                request.transfer.decision = PortalDecision::Denied;
                Ok(None)
            }
            PortalPolicyDecision::Allow => {
                request.transfer.decision = PortalDecision::Allowed;
                self.grants.insert(
                    transfer,
                    PortalGrant {
                        transfer,
                        source_namespace: request.transfer.source_namespace,
                        target_namespace: request.transfer.target_namespace,
                        kind: request.transfer.kind,
                        source_generation,
                        broker_generation: self.broker_generation,
                        deadline_msec: request.deadline_msec,
                        state: PortalGrantState::Active,
                    },
                );
                Ok(self.grants.get(&transfer))
            }
        }
    }

    pub fn complete(&mut self, transfer: PortalTransferId) -> Result<(), PortalLifecycleError> {
        self.active_grant_mut(transfer)?.state = PortalGrantState::Completed;
        Ok(())
    }

    pub fn executor_failed(
        &mut self,
        transfer: PortalTransferId,
    ) -> Result<(), PortalLifecycleError> {
        self.active_grant_mut(transfer)?.state = PortalGrantState::Revoked;
        Ok(())
    }

    /// Settle one deadline-expired grant without changing unrelated grants.
    pub fn expire_transfer(
        &mut self,
        transfer: PortalTransferId,
        now_msec: u64,
    ) -> Result<(), PortalLifecycleError> {
        let grant = self.active_grant_mut(transfer)?;
        if grant.deadline_msec > now_msec {
            return Err(PortalLifecycleError::InvalidRequest);
        }
        grant.state = PortalGrantState::Expired;
        Ok(())
    }

    pub fn expire(&mut self, now_msec: u64) -> Vec<PortalTransferId> {
        let mut expired = Vec::new();
        for (transfer, request) in &mut self.requests {
            if request.deadline_msec <= now_msec {
                if let Some(grant) = self.grants.get_mut(transfer) {
                    if grant.state == PortalGrantState::Active {
                        grant.state = PortalGrantState::Expired;
                        expired.push(*transfer);
                    }
                } else if request.transfer.decision == PortalDecision::Pending {
                    request.transfer.decision = PortalDecision::Revoked;
                    expired.push(*transfer);
                }
            }
        }
        expired
    }

    pub fn source_owner_changed(
        &mut self,
        source_namespace: NamespaceId,
        generation: u64,
    ) -> Vec<PortalTransferId> {
        self.revoke_where(|request| {
            request.transfer.source_namespace == source_namespace
                && request.transfer.generation != generation
        })
    }

    pub fn namespace_disconnected(&mut self, namespace: NamespaceId) -> Vec<PortalTransferId> {
        self.revoke_where(|request| {
            request.transfer.source_namespace == namespace
                || request.transfer.target_namespace == namespace
        })
    }

    pub fn broker_restarted(
        &mut self,
        broker_generation: u64,
    ) -> Result<Vec<PortalTransferId>, PortalLifecycleError> {
        if broker_generation <= self.broker_generation {
            return Err(PortalLifecycleError::InvalidBrokerGeneration);
        }
        let revoked = self.revoke_where(|_| true);
        self.broker_generation = broker_generation;
        Ok(revoked)
    }

    pub fn request(&self, transfer: PortalTransferId) -> Option<&PortalRequest> {
        self.requests.get(&transfer)
    }

    pub fn grant(&self, transfer: PortalTransferId) -> Option<&PortalGrant> {
        self.grants.get(&transfer)
    }

    fn active_grant_mut(
        &mut self,
        transfer: PortalTransferId,
    ) -> Result<&mut PortalGrant, PortalLifecycleError> {
        let grant = self
            .grants
            .get_mut(&transfer)
            .ok_or(PortalLifecycleError::UnknownTransfer)?;
        if grant.state != PortalGrantState::Active {
            return Err(PortalLifecycleError::GrantNotActive);
        }
        Ok(grant)
    }

    fn revoke_where(
        &mut self,
        mut predicate: impl FnMut(&PortalRequest) -> bool,
    ) -> Vec<PortalTransferId> {
        let mut revoked = Vec::new();
        for (transfer, request) in &mut self.requests {
            if !predicate(request) {
                continue;
            }
            let changed = if let Some(grant) = self.grants.get_mut(transfer) {
                if grant.state == PortalGrantState::Active {
                    grant.state = PortalGrantState::Revoked;
                    true
                } else {
                    false
                }
            } else if request.transfer.decision == PortalDecision::Pending {
                request.transfer.decision = PortalDecision::Revoked;
                true
            } else {
                false
            };
            if changed {
                revoked.push(*transfer);
            }
        }
        revoked
    }

    fn pending_or_active_count(&self) -> usize {
        self.requests
            .values()
            .filter(|request| request.transfer.decision == PortalDecision::Pending)
            .count()
            + self
                .grants
                .values()
                .filter(|grant| grant.state == PortalGrantState::Active)
                .count()
    }
}

fn valid_request(request: &PortalRequest) -> bool {
    let PortalTransfer {
        transfer,
        source_namespace,
        target_namespace,
        decision,
        generation,
        ..
    } = &request.transfer;
    transfer.is_valid()
        && source_namespace.is_valid()
        && target_namespace.is_valid()
        && source_namespace != target_namespace
        && *decision == PortalDecision::Pending
        && *generation != 0
        && request.deadline_msec != 0
}
