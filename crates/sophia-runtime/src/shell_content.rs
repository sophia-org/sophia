//! CPU content ownership for one admitted grant.
//!
//! This reducer does not grant capabilities, route bytes or report native
//! presentation. Its leases must outlive every consumer, including a retired
//! connection. The session-global admission owner must retain this store until
//! `quiescent`; dropping a peer is not a renderer completion.

mod accounting;
mod allocations;
mod candidates;
mod epoch_registry;
mod epochs;
mod resources;

pub use accounting::*;
pub use allocations::*;
pub use candidates::*;
pub use epoch_registry::*;
pub use epochs::*;
pub use resources::*;
