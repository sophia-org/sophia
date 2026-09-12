//! What is held, by whom, and what is still owed.
//!
//! A press arrives as a reservation and becomes a hold only when the authority
//! applies it. That distinction is the whole reason revocation is safe: a
//! reservation dropped before execution owes nothing, because nothing was ever
//! delivered, while a hold applied before revocation owes a release even if no
//! byte ever reached the recipient.

use crate::identity::{HoldIncarnation, Input, SourceId};

/// A press the authority applied. Server state has moved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Applied {
    pub(crate) source: SourceId,
    pub(crate) input: Input,
    pub(crate) incarnation: HoldIncarnation,
}

/// What a release should do.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseOutcome {
    /// The source was not holding. Recorded as a refusal, delivered as nothing.
    NotHeld,
    /// Another source still holds this input. The aggregate is unchanged, so
    /// nothing is delivered and the surviving hold is untouched.
    SurvivorRemains,
    /// The last holder let go. The aggregate is now clear, and this release is
    /// owed to the recipient the first press was delivered to.
    DeliverTo(HoldIncarnation),
}

/// Two obligations that one receipt does not discharge together.
///
/// Native reconciliation is shared modifier, grab and ledger state settling
/// under the guard. Recipient transport is the release reaching the client it
/// was delivered to. A flush proves the second and says nothing about the
/// first; a disconnect proves the second differently and still says nothing
/// about the first.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SettlementBit {
    pub native_reconciled: bool,
    pub recipient_settled: bool,
}

impl SettlementBit {
    /// Debt is discharged only when both obligations are met.
    pub fn is_settled(self) -> bool {
        self.native_reconciled && self.recipient_settled
    }
}
