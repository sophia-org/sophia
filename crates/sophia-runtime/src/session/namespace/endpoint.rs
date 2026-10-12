//! Session-owned endpoint authority. Naming snapshots carry only the passive
//! scope; workers retain these leases and never infer permission from a path.
use super::*;
use sophia_protocol::namespace_view::{EndpointGrantScope, GrantedEndpoint, ServiceReference};

/// 64 live launches with at most 64 distinct endpoint grants each. Aliases
/// retain one grant rather than consuming another authority slot.
pub const MAX_ENDPOINT_GRANTS: usize = 4096;

#[derive(Clone, Debug)]
pub struct EndpointGrantPolicy {
    pub namespace: NamespaceId,
    pub principal: u64,
    pub launch_generation: u64,
    pub audience: u64,
    pub service: ServiceReference,
}

#[derive(Debug)]
pub(super) struct EndpointGrantRecord {
    endpoint: GrantedEndpoint,
    launch_generation: u64,
    audience: u64,
    active: AtomicBool,
    namespace: Arc<AtomicBool>,
    registry: Arc<AtomicBool>,
}
impl EndpointGrantRecord {
    pub(super) fn is_live(&self) -> bool {
        self.registry.load(Ordering::Acquire)
            && self.namespace.load(Ordering::Acquire)
            && self.active.load(Ordering::Acquire)
    }
}

/// Cloneable authority reference, issued only by the Session registry. Dropping
/// a listener/alias cannot revoke its peers; the registry owns revocation.
#[derive(Clone, Debug)]
pub struct EndpointGrantLease(Arc<EndpointGrantRecord>);

/// Optional bounded policy custody. Workers receive only cloned leases;
/// dropping this owner fences them without locking the namespace registry.
/// The registry reclaims the resulting tombstone on its next grant allocation.
#[derive(Debug)]
pub struct EndpointGrantOwner(EndpointGrantLease);
impl EndpointGrantOwner {
    pub fn lease(&self) -> &EndpointGrantLease {
        &self.0
    }
}
impl Drop for EndpointGrantOwner {
    fn drop(&mut self) {
        self.0.0.active.store(false, Ordering::Release);
    }
}
impl EndpointGrantLease {
    pub fn endpoint(&self) -> &GrantedEndpoint {
        &self.0.endpoint
    }
    pub fn is_live(&self) -> bool {
        self.0.is_live()
    }
    pub fn permits(
        &self,
        namespace: NamespaceId,
        method: ClientAuthenticationMethod,
        audience: u64,
    ) -> bool {
        self.is_live()
            && namespace == self.0.endpoint.scope.namespace
            && audience == self.0.audience
            && method
                == ClientAuthenticationMethod::FactotumLaunch {
                    principal: self.0.endpoint.scope.principal,
                    launch_generation: self.0.launch_generation,
                }
    }
}

impl NamespaceRegistry {
    pub fn own_endpoint_grant(
        &mut self,
        policy: EndpointGrantPolicy,
    ) -> Result<EndpointGrantOwner, NamespaceRegistryError> {
        self.grant_endpoint(policy).map(EndpointGrantOwner)
    }
    /// Trusted policy issuance, before recipe compilation. No client-supplied
    /// scope or grant ID is adopted, and service restarts require a fresh grant.
    pub fn grant_endpoint(
        &mut self,
        policy: EndpointGrantPolicy,
    ) -> Result<EndpointGrantLease, NamespaceRegistryError> {
        if policy.principal == 0
            || policy.launch_generation == 0
            || policy.audience == 0
            || !policy.service.is_valid()
        {
            return Err(NamespaceRegistryError::InvalidEndpointGrant);
        }
        let namespace = self.namespaces.get(&policy.namespace).ok_or(
            NamespaceRegistryError::UnknownNamespace {
                namespace: policy.namespace,
            },
        )?;
        self.endpoint_grants.retain(|_, grant| grant.is_live());
        if self.endpoint_grants.len() >= MAX_ENDPOINT_GRANTS {
            return Err(NamespaceRegistryError::EndpointGrantCapacity);
        }
        let id = self
            .next_endpoint_grant
            .checked_add(1)
            .ok_or(NamespaceRegistryError::EndpointGrantExhausted)?;
        let record = Arc::new(EndpointGrantRecord {
            endpoint: GrantedEndpoint {
                service: policy.service,
                scope: EndpointGrantScope {
                    namespace: policy.namespace,
                    principal: policy.principal,
                    grant: id,
                    generation: self.session_generation,
                },
            },
            launch_generation: policy.launch_generation,
            audience: policy.audience,
            active: AtomicBool::new(true),
            namespace: namespace.active.clone(),
            registry: self.live.clone(),
        });
        self.next_endpoint_grant = id;
        self.endpoint_grants.insert(id, record.clone());
        Ok(EndpointGrantLease(record))
    }

    pub fn endpoint_grant_is_current(&self, grant: &EndpointGrantLease) -> bool {
        grant.is_live()
            && self
                .endpoint_grants
                .get(&grant.endpoint().scope.grant)
                .is_some_and(|record| Arc::ptr_eq(record, &grant.0))
    }

    pub fn revoke_endpoint_grant(
        &mut self,
        grant: &EndpointGrantLease,
    ) -> Result<(), NamespaceRegistryError> {
        if !self.endpoint_grant_is_current(grant) {
            return Err(NamespaceRegistryError::StaleEndpointGrant);
        }
        // Fence all clones and retained admissions before removing bookkeeping.
        grant.0.active.store(false, Ordering::Release);
        self.endpoint_grants.remove(&grant.endpoint().scope.grant);
        Ok(())
    }

    pub fn prepare_granted_admission(
        &mut self,
        grant: &EndpointGrantLease,
        method: ClientAuthenticationMethod,
        audience: u64,
    ) -> Result<PreparedAdmission<'_>, NamespaceRegistryError> {
        let namespace = grant.endpoint().scope.namespace;
        if !self.endpoint_grant_is_current(grant) || !grant.permits(namespace, method, audience) {
            return Err(NamespaceRegistryError::StaleEndpointGrant);
        }
        Ok(self
            .prepare_admission(namespace, method)?
            .with_grant(grant.clone()))
    }

    pub fn endpoint_grant_count(&self) -> usize {
        self.endpoint_grants
            .values()
            .filter(|grant| grant.is_live())
            .count()
    }

    /// Session calls this when the launch owner retires. Namespace and registry
    /// retirement already fence leases through their shared liveness cells.
    pub fn revoke_launch_endpoint_grants(
        &mut self,
        namespace: NamespaceId,
        principal: u64,
        launch_generation: u64,
    ) -> usize {
        let mut revoked = 0;
        self.endpoint_grants.retain(|_, grant| {
            if grant.endpoint.scope.namespace == namespace
                && grant.endpoint.scope.principal == principal
                && grant.launch_generation == launch_generation
            {
                revoked += usize::from(grant.is_live());
                grant.active.store(false, Ordering::Release);
                false
            } else {
                true
            }
        });
        revoked
    }
}
