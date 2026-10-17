//! Developing output session client with fixed-file startup and an immutable
//! submission lane, ordered journal and topology fetcher. The integrated
//! session is developing; production qualification remains incomplete.
pub use sophia_output_protocol as protocol;
mod connecting;
pub use connecting::Connecting;
mod session;
pub use session::{Obligations, Session, SessionError, SessionState};
/// Provisional ordered journal for the developing output driver.
#[doc(hidden)]
pub mod journal;
/// Provisional fixed-file startup for the developing output driver.
#[doc(hidden)]
pub mod startup;
/// Provisional immutable submission lane for the developing output driver.
#[doc(hidden)]
pub mod submission;
/// Provisional publication-bound topology fetcher.
#[doc(hidden)]
pub mod topology;
