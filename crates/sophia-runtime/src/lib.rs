//! Shared runtime conventions for Sophia processes.
//!
//! Libraries emit structured diagnostics through `tracing`; binaries decide
//! when and how to install a subscriber.

#[cfg(target_os = "linux")]
mod broker_transport;
#[cfg(target_os = "linux")]
mod control;
mod error;
#[cfg(target_os = "linux")]
mod host_domain;
#[cfg(target_os = "linux")]
pub mod inspection;
mod output_ipc;
#[cfg(target_os = "linux")]
mod output_service;
#[cfg(target_os = "linux")]
mod output_transport;
mod policy_capabilities;
mod policy_ipc;
mod policy_profile_handoff;
#[cfg(target_os = "linux")]
mod policy_profile_io;
#[cfg(target_os = "linux")]
mod policy_socket;
#[cfg(target_os = "linux")]
mod policy_transport;
mod session;
#[cfg(target_os = "linux")]
mod shell_transport;
mod supervisor;
mod tracing;

mod prelude {
    pub(crate) use core::fmt;
    pub(crate) use std::ffi::OsString;
    pub(crate) use std::process::{Child, Command};
    pub(crate) use std::time::Duration;

    pub(crate) use sophia_protocol::{
        BrokerHealthState, BrokerKind, SOPHIA_BROKER_HEALTH_MAX_MESSAGE_LEN, TransactionOutcome,
    };
    pub(crate) use tracing_subscriber::EnvFilter;

    pub(crate) use crate::SupervisedProcessKind;
    pub(crate) use crate::{SophiaErrorExt, SophiaErrorKind};
}

#[cfg(target_os = "linux")]
pub use broker_transport::*;
#[cfg(target_os = "linux")]
pub use control::*;
pub use error::*;
#[cfg(target_os = "linux")]
pub use inspection::SOPHIA_WM_INSPECT_SOCKET_ENV;
pub use output_ipc::*;
#[cfg(target_os = "linux")]
pub use output_service::*;
#[cfg(target_os = "linux")]
pub use output_transport::*;
pub use policy_capabilities::*;
pub use policy_ipc::*;
pub use policy_profile_handoff::*;
#[cfg(target_os = "linux")]
pub use policy_profile_io::*;
#[cfg(target_os = "linux")]
pub use policy_socket::*;
#[cfg(target_os = "linux")]
pub use policy_transport::*;
pub use session::*;
#[cfg(target_os = "linux")]
pub use shell_transport::*;
pub use supervisor::*;
pub use tracing::*;

mod shell_content;
pub use shell_content::*;
