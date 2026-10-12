use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use sophia_protocol::{
    ClientAdmissionContext, ClientAdmissionId, ClientAuthProvenance, ClientAuthenticationMethod,
    IdAllocator, NamespaceCapabilities, NamespaceContext, NamespaceId, NamespaceProfile,
};

use crate::{SophiaErrorExt, SophiaErrorKind};

mod admission;
pub use admission::{AdmissionLease, PreparedAdmission};
use admission::{AdmissionRecord, NamespaceRecord};

/// Session-owned namespace and client-admission registry.
///
/// IDs are monotonic for the registry lifetime. Contexts are immutable; policy
/// changes revoke old admissions rather than mutating authority-visible facts.
#[derive(Debug)]
pub struct NamespaceRegistry {
    session_generation: u64,
    namespace_ids: IdAllocator<NamespaceId>,
    admission_ids: IdAllocator<ClientAdmissionId>,
    namespaces: BTreeMap<NamespaceId, NamespaceRecord>,
    admissions: BTreeMap<ClientAdmissionId, AdmissionRecord>,
    live: Arc<AtomicBool>,
}

impl NamespaceRegistry {
    pub fn new(session_generation: u64) -> Result<Self, NamespaceRegistryError> {
        if session_generation == 0 {
            return Err(NamespaceRegistryError::InvalidSessionGeneration);
        }
        Ok(Self {
            session_generation,
            namespace_ids: IdAllocator::new(),
            admission_ids: IdAllocator::new(),
            namespaces: BTreeMap::new(),
            admissions: BTreeMap::new(),
            live: Arc::new(AtomicBool::new(true)),
        })
    }

    /// A registry that already holds one explicit namespace.
    ///
    /// FOR A SERVICE TOLD WHICH NAMESPACE IT SERVES. `new` starts empty and
    /// allocates ids itself, so a caller with a namespace it must serve has no
    /// way to install it: admission would refuse every client as belonging to
    /// an unknown namespace. Calling `create_namespace` until the allocator
    /// happened to reach that id would be worse, minting namespaces nobody
    /// asked for.
    ///
    /// The allocator is seeded past the given id, so a later creation cannot
    /// hand the same one out again. A namespace at the end of the range is
    /// refused rather than accepted with an allocator that would immediately
    /// repeat it.
    pub fn with_namespace(
        session_generation: u64,
        namespace: NamespaceContext,
    ) -> Result<Self, NamespaceRegistryError> {
        if session_generation == 0 {
            return Err(NamespaceRegistryError::InvalidSessionGeneration);
        }
        if !namespace.is_valid() {
            return Err(NamespaceRegistryError::UnknownNamespace {
                namespace: namespace.id,
            });
        }
        let namespace_ids = IdAllocator::seeded_past(namespace.id.raw()).ok_or(
            NamespaceRegistryError::UnknownNamespace {
                namespace: namespace.id,
            },
        )?;
        let mut namespaces = BTreeMap::new();
        namespaces.insert(namespace.id, NamespaceRecord::new(namespace));
        Ok(Self {
            session_generation,
            namespace_ids,
            admission_ids: IdAllocator::new(),
            namespaces,
            admissions: BTreeMap::new(),
            live: Arc::new(AtomicBool::new(true)),
        })
    }

    pub const fn session_generation(&self) -> u64 {
        self.session_generation
    }

    pub fn create_namespace(
        &mut self,
        profile: NamespaceProfile,
        capabilities: NamespaceCapabilities,
    ) -> NamespaceContext {
        let context = NamespaceContext::new(self.namespace_ids.next_id(), profile, capabilities)
            .expect("namespace allocator returned an invalid ID");
        self.namespaces
            .insert(context.id, NamespaceRecord::new(context));
        context
    }

    pub fn namespace(&self, namespace: NamespaceId) -> Option<NamespaceContext> {
        self.namespaces.get(&namespace).map(|record| record.context)
    }

    pub fn admit(
        &mut self,
        namespace: NamespaceId,
        method: ClientAuthenticationMethod,
    ) -> Result<ClientAdmissionContext, NamespaceRegistryError> {
        Ok(self.prepare_admission(namespace, method)?.publish())
    }

    /// Reserve immutable facts without publishing an admission. The exclusive
    /// borrow keeps namespace policy fixed until the caller commits or aborts.
    pub fn prepare_admission(
        &mut self,
        namespace: NamespaceId,
        method: ClientAuthenticationMethod,
    ) -> Result<PreparedAdmission<'_>, NamespaceRegistryError> {
        let namespace = self
            .namespace(namespace)
            .ok_or(NamespaceRegistryError::UnknownNamespace { namespace })?;
        let auth_provenance = ClientAuthProvenance::new(method, self.session_generation)
            .ok_or(NamespaceRegistryError::InvalidAuthentication)?;
        // A transport can revoke its lease without taking this registry's lock.
        // Reclaim tombstones before allocating another admission, keeping them
        // bounded by the admissions present at the last allocation.
        self.admissions.retain(|_, record| record.live());
        let context =
            ClientAdmissionContext::new(self.admission_ids.next_id(), namespace, auth_provenance)
                .expect("admission allocator returned an invalid ID");
        Ok(PreparedAdmission::new(self, context))
    }

    pub fn admission(&self, client: ClientAdmissionId) -> Option<ClientAdmissionContext> {
        self.admissions
            .get(&client)
            .filter(|record| record.live())
            .map(|record| record.context)
    }

    pub fn is_current_admission(&self, context: ClientAdmissionContext) -> bool {
        context.auth_provenance.session_generation == self.session_generation
            && self.admission(context.client_id) == Some(context)
            && self.namespace(context.namespace.id) == Some(context.namespace)
    }

    pub fn revoke_admission(
        &mut self,
        client: ClientAdmissionId,
    ) -> Result<ClientAdmissionContext, NamespaceRegistryError> {
        let record = self
            .admissions
            .remove(&client)
            .ok_or(NamespaceRegistryError::UnknownAdmission { client })?;
        if !record.live() {
            return Err(NamespaceRegistryError::UnknownAdmission { client });
        }
        record.revoke();
        Ok(record.context)
    }

    pub fn revoke_namespace(
        &mut self,
        namespace: NamespaceId,
    ) -> Result<NamespaceRevocation, NamespaceRegistryError> {
        let record = self
            .namespaces
            .remove(&namespace)
            .ok_or(NamespaceRegistryError::UnknownNamespace { namespace })?;
        // One release makes every retained lease stale before map cleanup.
        record.active.store(false, Ordering::Release);
        let clients = self
            .admissions
            .iter()
            .filter_map(|(client, admission)| {
                (admission.context.namespace.id == namespace).then_some(*client)
            })
            .collect::<Vec<_>>();
        let admissions = clients
            .into_iter()
            .filter_map(|client| self.admissions.remove(&client))
            .filter_map(|record| {
                let was_live = record.live();
                record.revoke();
                was_live.then_some(record.context)
            })
            .collect();
        Ok(NamespaceRevocation {
            namespace: record.context,
            admissions,
        })
    }

    pub fn namespace_count(&self) -> usize {
        self.namespaces.len()
    }

    pub fn admission_count(&self) -> usize {
        self.admissions
            .values()
            .filter(|record| record.live())
            .count()
    }
}

impl Drop for NamespaceRegistry {
    fn drop(&mut self) {
        self.live.store(false, Ordering::Release);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamespaceRevocation {
    pub namespace: NamespaceContext,
    pub admissions: Vec<ClientAdmissionContext>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NamespaceRegistryError {
    InvalidSessionGeneration,
    InvalidAuthentication,
    UnknownNamespace { namespace: NamespaceId },
    UnknownAdmission { client: ClientAdmissionId },
}

impl core::fmt::Display for NamespaceRegistryError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidSessionGeneration => {
                formatter.write_str("namespace registry session generation must be nonzero")
            }
            Self::InvalidAuthentication => {
                formatter.write_str("invalid client authentication provenance")
            }
            Self::UnknownNamespace { namespace } => {
                write!(formatter, "unknown namespace {}", namespace.raw())
            }
            Self::UnknownAdmission { client } => {
                write!(formatter, "unknown client admission {}", client.raw())
            }
        }
    }
}

impl std::error::Error for NamespaceRegistryError {}

impl SophiaErrorExt for NamespaceRegistryError {
    fn kind(&self) -> SophiaErrorKind {
        SophiaErrorKind::InvalidNamespace
    }
}
