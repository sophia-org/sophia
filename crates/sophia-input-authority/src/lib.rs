//! Who is holding which input, and by what authority.
//!
//! One authority instance owns one seat inside one Sophia instance. Physical
//! and synthetic sources live in the same ledger here, because a key held by
//! both is one key: the question "is this still held" has a single answer, and
//! answering it in two places is how a stuck modifier or a stolen release
//! happens.
//!
//! This crate depends on `sophia-protocol` and `std`, and nothing else. It
//! names no adapter's types and takes no adapter's locks, which is what allows
//! every adapter to reach it in the same order.

mod capacity;
mod grant;
mod identity;
mod ledger;
mod registry;

pub use capacity::{Capacity, CapacityError};
pub use grant::{GrantGeneration, GrantId, IssuerHandle, SubmitHandle};
pub use identity::{
    DeviceCapability, HoldIncarnation, Input, InputError, InstanceId, Origin, Recipient,
    SeatBinding, SourceId,
};
pub use ledger::{Applied, ReleaseOutcome, SettlementBit};
pub use registry::{AuthorityInstance, ExecutionContext, RegistrationError, RetiredDebt};
