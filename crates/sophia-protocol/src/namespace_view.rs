//! Passive naming facts. A service reference, path or grant number is never
//! evidence of permission; the admitting role owner checks live authority.
use crate::NamespaceId;

mod discovery;
pub use discovery::*;

pub const DISCOVERY_NAME: &str = "discovery";
pub const NAMESPACE_DIRECTORY_ENV: &str = "SOPHIA_NAMESPACE_DIR";
pub const MAX_ENDPOINTS: usize = 64;
pub const MAX_PATH_BYTES: usize = 1024;
pub const MAX_COMPONENT_BYTES: usize = 255;
pub const MAX_DISCOVERY_BYTES: usize = 128 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndpointProtocol {
    NineP2000L,
    X11,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceReference {
    pub identity: u64,
    pub generation: u64,
    pub protocol: EndpointProtocol,
    pub interface: String,
    pub version: u32,
}

/// The owner's opaque grant incarnation, scoped to the selected identity.
/// Its contents and revocation remain with that owner, not the compiler.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EndpointGrantScope {
    pub namespace: NamespaceId,
    pub principal: u64,
    pub grant: u64,
    pub generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GrantedEndpoint {
    pub service: ServiceReference,
    pub scope: EndpointGrantScope,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamespaceEndpoint {
    /// Normalized relative path, below the view's structural root.
    pub path: String,
    pub endpoint: GrantedEndpoint,
}

/// Naming snapshot only. Authentication must bind it to the admission before
/// any protected operation; compiler success does not constitute admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledNamespace {
    pub revision: u64,
    pub namespace: NamespaceId,
    pub principal: u64,
    pub endpoints: Vec<NamespaceEndpoint>,
}

/// A bounded observation, not a grant or proof that pixels are presented.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceAvailability {
    Available,
    Waiting,
    Recovering,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredEndpoint {
    pub path: String,
    pub service: ServiceReference,
    pub availability: ServiceAvailability,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamespaceDiscovery {
    pub revision: u64,
    pub endpoints: Vec<DiscoveredEndpoint>,
}
