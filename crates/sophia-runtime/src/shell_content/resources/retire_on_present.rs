//! W2 prototype: a candidate's negotiated intent to retire one named
//! predecessor once the candidate is presented. Admission reserves the
//! retiring bytes and denies every new lease of the resource; only the
//! successful transfer of `Presented` to the output commits it.
//!
//! The owner holds a token for as long as it owns the intent: its pending
//! or submitted candidate, then its queued Presented. Every check reads the
//! token, so a superseded, expired or failed candidate stops holding its
//! intent at once, without waiting for a collect.
use std::sync::Weak;

use super::*;

/// One candidate's exact identity: the owner of an intent. A transaction ID
/// is only correlation, since two outputs may reuse one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentRetireOwner {
    pub grant: ContentGrant,
    pub output: ContentOutputId,
    pub candidate_generation: u64,
}

/// The intent a candidate's End carries (the r12/bit 17 codec value).
pub use sophia_protocol::shell::content::ContentCandidateRetire as ContentRetireOnPresent;

/// Held by the intent's owner; dropping it cancels an uncommitted intent.
pub type ContentRetireToken = Arc<()>;

#[derive(Clone, Debug)]
pub(super) struct Intent {
    owner: ContentRetireOwner,
    retire_transaction: TransactionId,
    token: Weak<()>,
    /// Committed intents keep their retire ID held until release.
    committed: bool,
}

impl Intent {
    fn holds(&self) -> bool {
        self.committed || self.token.strong_count() > 0
    }

    fn reserves(&self) -> bool {
        !self.committed && self.token.strong_count() > 0
    }
}

impl Accepted {
    pub(super) fn named(&self) -> bool {
        self.intent.as_ref().is_some_and(Intent::holds)
    }
}

impl ContentResourceStore {
    /// Bytes held for admitted, uncommitted intents whose owner still holds
    /// them.
    pub fn reserved_retiring(&self) -> u64 {
        if self.intent_records == 0 {
            return 0;
        }
        self.accepted
            .values()
            .filter(|value| value.intent.as_ref().is_some_and(Intent::reserves))
            .map(|value| value.pixels.description().total_bytes)
            .sum()
    }

    /// Whether an explicit retire would only be refused because admitted
    /// intents hold the retiring headroom. Such a refusal needs its own
    /// response credit; the resource's held release credit never pays it.
    pub(super) fn refused_by_reservation(&self, value: &Accepted) -> bool {
        let bytes = value.pixels.description().total_bytes;
        self.usage.retiring + bytes <= self.limits.max_retiring_bytes
            && self.usage.retiring + self.reserved_retiring() + bytes
                > self.limits.max_retiring_bytes
    }

    /// A retire ID W2 holds: named by a held intent, from admission until
    /// its resource is released. Legacy explicit retires keep their rules.
    pub(super) fn retire_id_held(&self, transaction: TransactionId) -> bool {
        self.intent_records != 0
            && self.accepted.values().any(|value| {
                value.intent.as_ref().is_some_and(|intent| {
                    intent.holds() && intent.retire_transaction == transaction
                })
            })
    }

    pub fn reserve_retire(
        &mut self,
        owner: ContentRetireOwner,
        intent: ContentRetireOnPresent,
        token: &ContentRetireToken,
    ) -> Result<(), ContentStoreError> {
        self.check(owner.grant, intent.retire_transaction)?;
        let retiring_on_id = self
            .accepted
            .values()
            .any(|value| value.retiring && value.transaction == intent.retire_transaction);
        if retiring_on_id
            || self.retire_id_held(intent.retire_transaction)
            || self.accepted.values().any(|value| {
                value
                    .intent
                    .as_ref()
                    .is_some_and(|held| held.holds() && held.owner == owner)
            })
        {
            return Err(ContentStoreError::Stale);
        }
        let value = self
            .accepted
            .get(&intent.resource)
            .ok_or(ContentStoreError::Stale)?;
        if value.retiring || value.named() {
            return Err(ContentStoreError::Stale);
        }
        let bytes = value.pixels.description().total_bytes;
        if self.usage.retiring + self.reserved_retiring() + bytes > self.limits.max_retiring_bytes {
            return Err(ContentStoreError::Budget);
        }
        let value = self.accepted.get_mut(&intent.resource).expect("checked");
        if value.intent.is_none() {
            self.intent_records += 1;
        }
        value.intent = Some(Intent {
            owner,
            retire_transaction: intent.retire_transaction,
            token: Arc::downgrade(token),
            committed: false,
        });
        Ok(())
    }

    /// Apply `owner`'s held intent exactly as an admitted retire, without new
    /// headroom: the reservation becomes the retiring charge. False when no
    /// such intent is held, because it was cancelled or already committed.
    pub fn commit_retire(&mut self, owner: &ContentRetireOwner) -> bool {
        let Some(value) = self.accepted.values_mut().find(|value| {
            value
                .intent
                .as_ref()
                .is_some_and(|intent| intent.reserves() && intent.owner == *owner)
        }) else {
            return false;
        };
        let intent = value.intent.as_mut().expect("selected intent");
        intent.committed = true;
        let bytes = value.pixels.description().total_bytes;
        value.retiring = true;
        value.transaction = intent.retire_transaction;
        self.usage.resident -= bytes;
        self.usage.retiring += bytes;
        self.collect();
        true
    }

    /// Drop the records of intents whose owner let go. Correctness never
    /// waits for this: every check already reads the token.
    pub(super) fn forget_released_intents(&mut self) {
        if self.intent_records == 0 {
            return;
        }
        for value in self.accepted.values_mut() {
            if value.intent.as_ref().is_some_and(|intent| !intent.holds()) {
                value.intent = None;
                self.intent_records -= 1;
            }
        }
    }

    pub(super) fn cancel_all_retire(&mut self) {
        for value in self.accepted.values_mut() {
            value.intent = None;
        }
        self.intent_records = 0;
    }
}
