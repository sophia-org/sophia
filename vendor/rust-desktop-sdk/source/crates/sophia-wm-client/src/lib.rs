//! WM client implementation in progress. Typed candidates own immutable bytes;
//! file startup, negotiation, ordered events and custody use the 9P pipeline.
//! Snapshots are bound to their retained Cycle. Complete release qualification
//! remains pending; startup, journal and submission are provisional interfaces.
mod candidate;
mod connecting;
pub use candidate::{Candidate, EncodedCandidate};
pub use connecting::Connecting;
pub use sophia_wm_protocol as protocol;
/// Provisional ordered journal implementation for the session driver.
#[doc(hidden)]
pub mod journal;
mod session;
mod snapshot;
/// Startup implementation used by the developing session driver. Not a
/// negotiated WM session or a stable application entrypoint yet.
#[doc(hidden)]
pub mod startup;
/// Provisional submission state machine for the session driver.
#[doc(hidden)]
pub mod submission;
pub use session::{Obligations, Session, SessionError, SessionState};
