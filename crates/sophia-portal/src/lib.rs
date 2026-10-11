//! Cross-namespace portal policy reducers.
//!
//! Portal code is intentionally off the compositor hot path. It turns
//! namespaced transfer requests into bounded commands that the runtime or
//! X Authority adapter can execute without granting policy code raw X authority.

mod broker;
mod clipboard;
mod drag_and_drop;
pub mod file_client;
mod file_handoff;
pub mod files;
mod lifecycle;
mod notification;
mod screen_capture;
mod socket;
mod types;
mod uri_open;

mod prelude {
    pub(crate) use std::collections::BTreeMap;

    pub(crate) use sophia_protocol::{
        NamespaceId, PortalDecision, PortalTransfer, PortalTransferId, PortalTransferKind,
    };
}

pub use broker::*;
pub use clipboard::*;
pub use drag_and_drop::*;
pub use file_handoff::*;
pub use lifecycle::*;
pub use notification::*;
pub use screen_capture::*;
pub use socket::*;
pub use types::*;
pub use uri_open::*;
