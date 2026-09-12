//! What is held, by whom, and what is still owed.
//!
//! A queued request has not applied a hold. The executor must retain its original
//! context and validate it at execution. A successful press reserves its record
//! before changing authority state; the record survives until native and
//! recipient obligations both settle, whether or not the press writer succeeded.

use crate::identity::{HoldIncarnation, Input, SourceId};

/// A press the authority applied. Server state has moved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Applied {
    pub(crate) source: SourceId,
    pub(crate) input: Input,
    pub(crate) incarnation: HoldIncarnation,
    pub(crate) first_press: bool,
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

impl Applied {
    pub fn source(self) -> SourceId {
        self.source
    }
    pub fn input(self) -> Input {
        self.input
    }
    pub fn incarnation(self) -> HoldIncarnation {
        self.incarnation
    }
    /// Only a first aggregate press asks the recipient to change state.
    pub fn first_press(self) -> bool {
        self.first_press
    }
}
