//! Bounded broker client components. No automatic connection or authority.
pub use sophia_broker_protocol as protocol;
mod journal;
mod submission;
pub use journal::Journal;
pub use submission::{Candidate, Custody, Submission};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    Busy,
    ClockRegression,
    Deadline,
    Closed,
    Protocol,
    Remote(u32),
    Overflow,
}

pub mod phase;
pub mod session;
pub mod startup;
pub mod transaction;
