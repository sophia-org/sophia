use crate::{ClientAdmissionId, NamespaceId};

/// Session-selected resource-sharing behavior for one namespace.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NamespaceProfile {
    /// Trusted clients deliberately retain ordinary shared-X coordination.
    ClassicShared,
    /// Resource discovery and delivery fail closed outside this namespace.
    Confined,
}

/// A bounded portal operation that namespace policy may permit.
///
/// Participation capability does not authorize a particular cross-namespace
/// transfer. A live portal decision and grant remain required.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NamespacePortalCapability {
    Clipboard = 0,
    DragAndDrop = 1,
    FileHandoff = 2,
    ScreenCapture = 3,
    ScreenRecording = 4,
    UriOpen = 5,
    Notification = 6,
}

impl NamespacePortalCapability {
    const fn bit(self) -> u64 {
        1u64 << self as u8
    }
}

/// Directional, bounded portal participation capabilities for one namespace.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct NamespaceCapabilities {
    request_bits: u64,
    publish_bits: u64,
}

impl NamespaceCapabilities {
    const SUPPORTED_BITS: u64 = (1u64 << 7) - 1;

    pub const NONE: Self = Self {
        request_bits: 0,
        publish_bits: 0,
    };

    pub const ALL: Self = Self {
        request_bits: Self::SUPPORTED_BITS,
        publish_bits: Self::SUPPORTED_BITS,
    };

    pub const fn from_bits(request_bits: u64, publish_bits: u64) -> Option<Self> {
        if request_bits & !Self::SUPPORTED_BITS == 0 && publish_bits & !Self::SUPPORTED_BITS == 0 {
            Some(Self {
                request_bits,
                publish_bits,
            })
        } else {
            None
        }
    }

    pub const fn with_request(mut self, capability: NamespacePortalCapability) -> Self {
        self.request_bits |= capability.bit();
        self
    }

    pub const fn with_publish(mut self, capability: NamespacePortalCapability) -> Self {
        self.publish_bits |= capability.bit();
        self
    }

    pub const fn allows_request(self, capability: NamespacePortalCapability) -> bool {
        self.request_bits & capability.bit() != 0
    }

    pub const fn allows_publish(self, capability: NamespacePortalCapability) -> bool {
        self.publish_bits & capability.bit() != 0
    }

    pub const fn request_bits(self) -> u64 {
        self.request_bits
    }

    pub const fn publish_bits(self) -> u64 {
        self.publish_bits
    }
}

/// Immutable namespace facts supplied to a protocol authority by the session.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NamespaceContext {
    pub id: NamespaceId,
    pub profile: NamespaceProfile,
    pub capabilities: NamespaceCapabilities,
}

impl NamespaceContext {
    pub const fn new(
        id: NamespaceId,
        profile: NamespaceProfile,
        capabilities: NamespaceCapabilities,
    ) -> Option<Self> {
        if id.is_valid() {
            Some(Self {
                id,
                profile,
                capabilities,
            })
        } else {
            None
        }
    }

    pub const fn is_valid(self) -> bool {
        self.id.is_valid()
    }
}

/// Sanitized proof of how the session admitted a client.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ClientAuthenticationMethod {
    TrustedLocal,
    PeerCredentials,
    MitMagicCookie1,
    /// Factotum's verified principal joined to Session's live launch custody.
    /// These identifiers are provenance, not a transferable credential.
    FactotumLaunch {
        principal: u64,
        launch_generation: u64,
    },
}

impl ClientAuthenticationMethod {
    pub const fn is_valid(self) -> bool {
        match self {
            Self::FactotumLaunch {
                principal,
                launch_generation,
            } => principal != 0 && launch_generation != 0,
            _ => true,
        }
    }
}

/// Authentication facts safe to retain outside the secret-validation layer.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ClientAuthProvenance {
    pub method: ClientAuthenticationMethod,
    pub session_generation: u64,
}

impl ClientAuthProvenance {
    pub const fn new(method: ClientAuthenticationMethod, session_generation: u64) -> Option<Self> {
        if session_generation == 0 || !method.is_valid() {
            None
        } else {
            Some(Self {
                method,
                session_generation,
            })
        }
    }

    pub const fn is_valid(self) -> bool {
        self.session_generation != 0 && self.method.is_valid()
    }
}

/// Immutable session admission result consumed by a protocol frontend.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ClientAdmissionContext {
    pub client_id: ClientAdmissionId,
    pub namespace: NamespaceContext,
    pub auth_provenance: ClientAuthProvenance,
}

impl ClientAdmissionContext {
    pub const fn new(
        client_id: ClientAdmissionId,
        namespace: NamespaceContext,
        auth_provenance: ClientAuthProvenance,
    ) -> Option<Self> {
        if client_id.is_valid() && namespace.is_valid() && auth_provenance.is_valid() {
            Some(Self {
                client_id,
                namespace,
                auth_provenance,
            })
        } else {
            None
        }
    }

    pub const fn is_valid(self) -> bool {
        self.client_id.is_valid() && self.namespace.is_valid() && self.auth_provenance.is_valid()
    }
}
