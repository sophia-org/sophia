//! Trusted requester components. The transport owner supplies admission, matches
//! replies, closes on terminal errors and never replays across connection epochs.
mod journal;
pub mod session;
pub mod startup;
mod submission;
pub mod transaction;
pub mod transfer;
pub mod upload;
pub use journal::Journal;
pub use submission::{Custody, Submission};
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
