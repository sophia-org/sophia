//! Retired socket discovery names; admission lives in `role_endpoint`.
use crate::PolicyRole;

pub const SOPHIA_WM_SOCKET_ENV: &str = "SOPHIA_WM_SOCKET";
pub const SOPHIA_SHELL_SOCKET_ENV: &str = "SOPHIA_SHELL_SOCKET";
pub const SOPHIA_BROKER_SOCKET_ENV: &str = "SOPHIA_BROKER_SOCKET";
pub const SOPHIA_OUTPUT_SOCKET_ENV: &str = "SOPHIA_OUTPUT_SOCKET";

// Compatibility names for adapters that have not yet been retired.
pub use crate::{RoleEndpoint as PolicyRoleEndpoint, RoleEndpointError as PolicyRoleEndpointError};

impl PolicyRole {
    /// The environment variable advertising this role's socket path.
    pub const fn socket_env(self) -> &'static str {
        match self {
            Self::Wm => SOPHIA_WM_SOCKET_ENV,
            Self::Shell => SOPHIA_SHELL_SOCKET_ENV,
            Self::Broker => SOPHIA_BROKER_SOCKET_ENV,
            Self::Output => SOPHIA_OUTPUT_SOCKET_ENV,
        }
    }
}
