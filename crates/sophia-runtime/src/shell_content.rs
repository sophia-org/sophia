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
mod isb_jobs;
mod isb_pixels;
mod isb_worker;
mod owned_ring;
mod profile;
mod reconnect_budget;
mod resources;

pub use accounting::*;
pub use allocations::*;
pub use candidates::*;
pub use epoch_registry::*;
pub use epochs::*;
pub use isb_jobs::*;
pub use isb_pixels::*;
pub use isb_worker::*;
pub use owned_ring::*;
pub use profile::*;
pub use reconnect_budget::*;
pub use resources::*;
