use std::collections::BTreeMap;

use sophia_protocol::{
    ClientAdmissionContext, ClientAdmissionId, ClientAuthProvenance, ClientAuthenticationMethod,
    IdAllocator, NamespaceCapabilities, NamespaceContext, NamespaceId, NamespaceProfile,
};

use crate::{SophiaErrorExt, SophiaErrorKind};

/// Session-owned namespace and client-admission registry.
///
/// IDs are monotonic for the registry lifetime. Contexts are immutable; policy
/// changes revoke old admissions rather than mutating authority-visible facts.
#[derive(Debug)]
pub struct NamespaceRegistry {
    session_generation: u64,
    namespace_ids: IdAllocator<NamespaceId>,
    admission_ids: IdAllocator<ClientAdmissionId>,
    namespaces: BTreeMap<NamespaceId, NamespaceContext>,
    admissions: BTreeMap<ClientAdmissionId, ClientAdmissionContext>,
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
        namespaces.insert(namespace.id, namespace);
        Ok(Self {
            session_generation,
            namespace_ids,
            admission_ids: IdAllocator::new(),
            namespaces,
            admissions: BTreeMap::new(),
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
        self.namespaces.insert(context.id, context);
        context
    }

    pub fn namespace(&self, namespace: NamespaceId) -> Option<NamespaceContext> {
        self.namespaces.get(&namespace).copied()
    }

    pub fn admit(
        &mut self,
        namespace: NamespaceId,
        method: ClientAuthenticationMethod,
    ) -> Result<ClientAdmissionContext, NamespaceRegistryError> {
        let namespace = self
            .namespace(namespace)
            .ok_or(NamespaceRegistryError::UnknownNamespace { namespace })?;
        let auth_provenance = ClientAuthProvenance::new(method, self.session_generation)
            .expect("validated session generation became invalid");
        let context =
            ClientAdmissionContext::new(self.admission_ids.next_id(), namespace, auth_provenance)
                .expect("admission allocator returned an invalid ID");
        self.admissions.insert(context.client_id, context);
        Ok(context)
    }

    pub fn admission(&self, client: ClientAdmissionId) -> Option<ClientAdmissionContext> {
        self.admissions.get(&client).copied()
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
        self.admissions
            .remove(&client)
            .ok_or(NamespaceRegistryError::UnknownAdmission { client })
    }

    pub fn revoke_namespace(
        &mut self,
        namespace: NamespaceId,
    ) -> Result<NamespaceRevocation, NamespaceRegistryError> {
        let context = self
            .namespaces
            .remove(&namespace)
            .ok_or(NamespaceRegistryError::UnknownNamespace { namespace })?;
        let clients = self
            .admissions
            .iter()
            .filter_map(|(client, admission)| {
                (admission.namespace.id == namespace).then_some(*client)
            })
            .collect::<Vec<_>>();
        let admissions = clients
            .into_iter()
            .filter_map(|client| self.admissions.remove(&client))
            .collect();
        Ok(NamespaceRevocation {
            namespace: context,
            admissions,
        })
    }

    pub fn namespace_count(&self) -> usize {
        self.namespaces.len()
    }

    pub fn admission_count(&self) -> usize {
        self.admissions.len()
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
    UnknownNamespace { namespace: NamespaceId },
    UnknownAdmission { client: ClientAdmissionId },
}

impl core::fmt::Display for NamespaceRegistryError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidSessionGeneration => {
                formatter.write_str("namespace registry session generation must be nonzero")
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
