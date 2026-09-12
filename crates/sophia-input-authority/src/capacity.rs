//! Every bound this authority holds, checked once at construction.
//!
//! Reserved before mutation rather than allocated at need, because the moment
//! capacity matters most is revocation, and revocation that fails for want of
//! space leaves holds nobody will release.

/// Why a capacity was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapacityError {
    /// The advertised button domain does not match what this authority
    /// preallocated for. Checked at construction rather than trusted, because
    /// the record count is derived from it.
    ButtonDomainMismatch { advertised: u16, expected: u16 },
    /// No grant slot is free. Retiring grants still hold theirs until their
    /// debt settles, so this counts them.
    NoGrantSlot,
    /// No synthetic device slot is free for this grant.
    NoDeviceSlot,
    /// No physical source slot is free. Counted separately so a saturating
    /// injector cannot consume what a real device needs.
    NoPhysicalSlot,
    /// The holder set cannot address every source this capacity allows.
    HolderWidthExceeded { sources: usize, width: usize },
    /// No hold or debt record is free.
    NoHoldRecord,
    /// No completion cell is free for this grant.
    NoCompletionCell,
}

/// The fixed shape of one authority instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capacity {
    /// Grant slots, counting those retiring with unsettled debt.
    pub grants: usize,
    /// Synthetic devices each grant may hold: one keyboard, one pointer.
    pub devices_per_grant: usize,
    /// X keycodes 8..=255.
    pub keys: usize,
    /// Core pointer buttons. Verified against the advertised domain.
    pub buttons: usize,
    /// One retained completion cell per grant slot.
    pub completions: usize,
    /// Attempt records scheduling over the debt population.
    pub attempts: usize,
    /// Physical sources, counted apart from synthetic ones.
    pub physical_sources: usize,
}

impl Capacity {
    /// The shape the approved plan fixes.
    pub const PLANNED: Self = Self {
        grants: 16,
        devices_per_grant: 2,
        keys: 248,
        buttons: 9,
        completions: 16,
        attempts: 64,
        physical_sources: 16,
    };

    /// Synthetic device slots: every grant's full allowance.
    pub const fn synthetic_sources(&self) -> usize {
        self.grants * self.devices_per_grant
    }

    /// Addressable inputs: every X keycode plus every advertised button, with
    /// keys and buttons in disjoint ranges so neither can alias the other.
    pub const fn input_slots(&self) -> usize {
        self.keys + self.buttons
    }

    /// Retained debts: one per input that can owe a release.
    pub const fn debt_records(&self) -> usize {
        self.input_slots()
    }

    /// Refuse a capacity whose sources cannot all be addressed in the holder
    /// set. Without this a source silently aliases another and one release
    /// clears a hold it never took.
    pub fn verify_holder_width(&self) -> Result<(), CapacityError> {
        let sources = self.synthetic_sources() + self.physical_sources;
        if sources <= u64::BITS as usize {
            Ok(())
        } else {
            Err(CapacityError::HolderWidthExceeded {
                sources,
                width: u64::BITS as usize,
            })
        }
    }

    /// Confirm the server's advertised button domain matches what this
    /// capacity preallocated for.
    ///
    /// The count is what the record population is derived from. A permutation
    /// of the same buttons is not a capacity change; a different number of them
    /// is, and it must be refused rather than silently under-allocated.
    pub fn verify_button_domain(&self, advertised: u16) -> Result<(), CapacityError> {
        let expected = u16::try_from(self.buttons).unwrap_or(u16::MAX);
        if advertised == expected {
            Ok(())
        } else {
            Err(CapacityError::ButtonDomainMismatch {
                advertised,
                expected,
            })
        }
    }
}
