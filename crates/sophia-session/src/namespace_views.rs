//! Session policy and namespace publication ownership. Filesystem work belongs
//! on a launch/configuration worker, never the input/render owner turn.
use sophia_config::namespace_recipe::PinnedNamespaceRecipe;
use sophia_protocol::{ClientAuthenticationMethod, NamespaceId, namespace_view::*};
use sophia_runtime::namespace_publication::{
    NamespacePublisher, NamespaceViewLease, PublishedNamespace,
};
use sophia_runtime::namespace_recipe::{Input, compile};
use sophia_runtime::{
    EndpointGrantLease, EndpointGrantOwner, EndpointGrantPolicy, NamespaceRegistry,
};
use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, TryLockError};

/// Already approved by Session policy. A recipe may name this service; it
/// cannot supply this record or obtain a service absent from this catalog.
#[derive(Clone, Debug)]
pub struct ApprovedEndpoint {
    pub service: ServiceReference,
    pub audience: u64,
    pub availability: ServiceAvailability,
}

pub struct PublishedRecipe {
    pub namespace: PublishedNamespace,
    grants: Vec<EndpointGrantLease>,
}
impl PublishedRecipe {
    /// Dispatch uses the captured endpoint, not a later path/catalog lookup.
    pub fn grant(&self, endpoint: &GrantedEndpoint) -> Option<&EndpointGrantLease> {
        self.grants
            .iter()
            .find(|grant| grant.endpoint() == endpoint)
    }
}

/// One launch's naming policy owner. Dropping it revokes its endpoint grants;
/// retained views remain allocated until their last connection releases them.
/// Registry locks cover authority bookkeeping only, never filesystem work.
pub struct NamespaceRecipeOwner {
    registry: Arc<Mutex<NamespaceRegistry>>,
    publisher: NamespacePublisher,
    namespace: NamespaceId,
    principal: u64,
    launch_generation: u64,
    policy_revision: u64,
    grants: Vec<Arc<EndpointGrantOwner>>,
}
impl NamespaceRecipeOwner {
    pub fn new(
        parent: &Path,
        registry: Arc<Mutex<NamespaceRegistry>>,
        namespace: NamespaceId,
        principal: u64,
        launch_generation: u64,
    ) -> io::Result<Self> {
        if principal == 0 || launch_generation == 0 {
            return Err(refused("invalid recipe launch identity"));
        }
        if registry
            .try_lock()
            .map_err(registry_unavailable)?
            .namespace(namespace)
            .is_none()
        {
            return Err(refused("recipe namespace is unavailable"));
        }
        Ok(Self {
            publisher: NamespacePublisher::new(parent, namespace, principal)?,
            registry,
            namespace,
            principal,
            launch_generation,
            policy_revision: 0,
            grants: Vec::new(),
        })
    }
    pub fn current(&self) -> Option<NamespaceViewLease> {
        self.publisher.current()
    }
    pub fn directory(&self) -> &Path {
        self.publisher.directory()
    }

    /// A fresh trusted policy revision is required even after failure. Once a
    /// reduction is accepted, retrying stale policy cannot revive old grants.
    /// The caller must keep its policy snapshot current through this operation.
    pub fn publish(
        &mut self,
        revision: u64,
        recipe: &PinnedNamespaceRecipe,
        approved: &BTreeMap<String, ApprovedEndpoint>,
    ) -> io::Result<PublishedRecipe> {
        if revision <= self.policy_revision {
            return Err(refused("stale namespace policy revision"));
        }
        recipe.verify()?;
        let (services, created) = self.apply_policy(revision, approved)?;
        let result = (|| {
            let view = compile(Input {
                revision,
                namespace: self.namespace,
                principal: self.principal,
                source: recipe.source(),
                fragments: recipe.files(),
                services: &services,
            })
            .map_err(io::Error::other)?;
            let discovery = NamespaceDiscovery {
                revision,
                endpoints: view
                    .endpoints
                    .iter()
                    .map(|e| {
                        let available = approved
                            .values()
                            .find(|a| a.service == e.endpoint.service)
                            .expect("compiled reference came from approved catalog")
                            .availability;
                        DiscoveredEndpoint {
                            path: e.path.clone(),
                            service: e.endpoint.service.clone(),
                            availability: available,
                        }
                    })
                    .collect(),
            };
            let prepared = self.publisher.prepare(view, discovery)?;
            // Materialization may take time. Never publish bytes from a replaced
            // trusted source, even if authority reduction has already happened.
            recipe.verify()?;
            let namespace = self.publisher.publish(prepared)?;
            Ok(PublishedRecipe {
                namespace,
                grants: self
                    .grants
                    .iter()
                    .map(|owner| owner.lease().clone())
                    .collect(),
            })
        })();
        if result.is_err() {
            self.grants
                .retain(|owner| !created.contains(&owner.lease().endpoint().scope.grant));
        }
        result
    }

    fn apply_policy(
        &mut self,
        revision: u64,
        approved: &BTreeMap<String, ApprovedEndpoint>,
    ) -> io::Result<(BTreeMap<String, GrantedEndpoint>, Vec<u64>)> {
        if approved.len() > MAX_ENDPOINTS
            || approved.iter().any(|(name, endpoint)| {
                name.is_empty()
                    || name.len() > MAX_COMPONENT_BYTES
                    || !name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                    || endpoint.audience == 0
                    || !endpoint.service.is_valid()
            })
        {
            return Err(refused("invalid approved endpoint catalog"));
        }
        // One service incarnation must describe one interface. Its aliases
        // must not advertise contradictory availability either.
        for a in approved.values() {
            if approved.values().any(|b| {
                a.service.identity == b.service.identity
                    && a.service.generation == b.service.generation
                    && a.service != b.service
            }) {
                return Err(refused("conflicting service incarnation metadata"));
            }
            if approved
                .values()
                .any(|b| a.service == b.service && a.availability != b.availability)
            {
                return Err(refused("conflicting service availability"));
            }
        }
        let method = ClientAuthenticationMethod::FactotumLaunch {
            principal: self.principal,
            launch_generation: self.launch_generation,
        };
        let mut registry = self.registry.try_lock().map_err(registry_unavailable)?;
        let mut selected: Vec<Arc<EndpointGrantOwner>> = Vec::new();
        let mut created = Vec::new();
        let mut services = BTreeMap::new();
        for (name, endpoint) in approved {
            let existing = selected
                .iter()
                .chain(&self.grants)
                .find(|owner| {
                    let grant = owner.lease();
                    registry.endpoint_grant_is_current(grant)
                        && grant.endpoint().service == endpoint.service
                        && grant.permits(self.namespace, method, endpoint.audience)
                })
                .cloned();
            let grant = match existing {
                Some(grant) => grant,
                None => match registry.own_endpoint_grant(EndpointGrantPolicy {
                    namespace: self.namespace,
                    principal: self.principal,
                    launch_generation: self.launch_generation,
                    audience: endpoint.audience,
                    service: endpoint.service.clone(),
                }) {
                    Ok(owner) => {
                        created.push(owner.lease().endpoint().scope.grant);
                        Arc::new(owner)
                    }
                    Err(error) => {
                        return Err(io::Error::other(error));
                    }
                },
            };
            services.insert(name.clone(), grant.lease().endpoint().clone());
            if !selected
                .iter()
                .any(|g| g.lease().endpoint() == grant.lease().endpoint())
            {
                selected.push(grant);
            }
        }
        // This is the authority reduction point. Publication failure cannot
        // undo it, and a later addition needs a fresh grant incarnation.
        self.grants = selected;
        self.policy_revision = revision;
        Ok((services, created))
    }
}
impl Drop for NamespaceRecipeOwner {
    fn drop(&mut self) {
        // Revoke before filesystem teardown, without waiting on a registry lock.
        self.grants.clear();
    }
}
fn refused(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn registry_unavailable<T>(error: TryLockError<T>) -> io::Error {
    match error {
        TryLockError::WouldBlock => {
            io::Error::new(io::ErrorKind::WouldBlock, "namespace registry busy")
        }
        TryLockError::Poisoned(_) => io::Error::other("namespace registry unavailable"),
    }
}
