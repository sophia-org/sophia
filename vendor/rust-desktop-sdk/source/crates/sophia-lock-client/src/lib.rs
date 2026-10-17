//! Experimental lock-provider session. The pinned contract is draft;
//! production and release qualification remain open. Providers only render.
pub use sophia_lock_protocol as protocol;
mod candidate;
pub use candidate::{Candidate, Prepared};
/// Provisional ordered journal; session integration remains in development.
#[doc(hidden)]
pub mod journal;
/// Provisional exact publication fetch, including supersession.
#[doc(hidden)]
pub mod object;
mod session;
/// Provisional fixed-file startup for the experimental session.
#[doc(hidden)]
pub mod startup;
/// Provisional submission lane; public admission policy belongs to Session.
#[doc(hidden)]
pub mod submission;
/// Provisional upload lifecycle and bounded write window.
#[doc(hidden)]
pub mod upload;
pub use session::{Obligations, Session, SessionError, SessionState};
mod connecting;
pub use connecting::Connecting;
