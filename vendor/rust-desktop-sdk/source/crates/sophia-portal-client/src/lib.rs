//! Developing recipient client components; callers supply authenticated transport.
pub mod capture;
pub mod capture_request;
pub mod frame;
pub mod read_window;
pub mod session;
pub mod snapshot;
pub mod startup;
pub mod transfer;
pub mod watch;
pub use sophia_portal_protocol as protocol;

pub mod requester;
