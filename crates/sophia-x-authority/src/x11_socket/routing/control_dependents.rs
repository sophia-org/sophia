// Work one operation queued on someone else, and how it reports its own end.
//
// Split from the completion records by subject: a record is what is owed for
// one operation, and this is what that operation set in motion elsewhere.
// Kept apart because the two are settled by different things -- an operation's
// own writer cannot answer for an effect sitting in another connection's
// queue, and nothing about the operation is settled while one can still run.

/// One effect an operation queued on someone else, held by the queued work
/// itself.
///
/// Counted by taking one of these and ended by giving it up, so every count
/// has exactly one holder and no caller can end an effect it does not hold.
/// Raw increment and decrement were reachable by anyone, which meant one
/// caller could end another's still-live work by counting down.
///
/// Giving it up is the same event whether the work ran or was given up unrun.
/// Both are ends, and which it was is deliberately not recorded: that would be
/// a receipt for a delivery nobody observed. What it establishes is only that
/// this particular effect can no longer happen.
#[cfg(unix)]
#[must_use = "work that is not held is work nothing is waiting for"]
pub struct ControlDependent {
    held: Option<ControlDependentHold>,
}

#[cfg(unix)]
struct ControlDependentHold {
    registry: ControlCompletionRegistry,
    origin: ControlCompletionToken,
}

/// Names the origin and nothing else.
///
/// Written rather than derived so that a routed control's debug output cannot
/// grow to include what any client asked for: the registry behind this holds
/// every accepted command.
#[cfg(unix)]
impl core::fmt::Debug for ControlDependent {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("ControlDependent")
            .field("origin", &self.held.as_ref().map(|held| held.origin))
            .finish_non_exhaustive()
    }
}

#[cfg(unix)]
impl Drop for ControlDependent {
    fn drop(&mut self) {
        if let Some(held) = self.held.take() {
            held.registry.dependent_ended(held.origin);
        }
    }
}

#[cfg(unix)]
impl ControlCompletionRegistry {
    /// Take responsibility for an effect this operation is about to queue
    /// elsewhere.
    ///
    /// Checked, and taken before the work is published, so there is no moment
    /// where the effect exists and nothing is waiting for it -- and no way to
    /// queue governed work that nothing is counting.
    pub fn track_dependent(
        &self,
        origin: ControlCompletionToken,
    ) -> Result<ControlDependent, ControlDependentRefusal> {
        if origin.origin != self.origin {
            return Err(ControlDependentRefusal::Foreign);
        }
        let Ok(mut inner) = self.inner.lock() else {
            return Err(ControlDependentRefusal::Unavailable);
        };
        let Some(record) = inner.records.iter_mut().find(|held| held.token == origin) else {
            return Err(ControlDependentRefusal::NoLongerHeld);
        };
        // Only an operation being applied is in a position to start something.
        if !matches!(record.phase, ControlPhase::Applying(_)) {
            return Err(ControlDependentRefusal::NotApplying);
        }
        let Some(counted) = record.dependents.checked_add(1) else {
            return Err(ControlDependentRefusal::Exhausted);
        };
        record.dependents = counted;
        Ok(ControlDependent {
            held: Some(ControlDependentHold {
                registry: self.clone(),
                origin,
            }),
        })
    }

    /// One of this operation's queued effects has ended.
    ///
    /// Private: only giving up the guard reaches it, so a count cannot be
    /// ended by anything that does not hold it.
    fn dependent_ended(&self, origin: ControlCompletionToken) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let Some(position) = inner.records.iter().position(|held| held.token == origin) else {
            return;
        };
        let record = &mut inner.records[position];
        record.dependents = record.dependents.saturating_sub(1);
        // Answered already, and now nothing it started can still happen. The
        // acknowledgement is not sent again: it went out when the outcome was
        // published, and this is only the end of the obligation behind it.
        if record.dependents == 0 && matches!(record.phase, ControlPhase::Settled(_)) {
            inner.records.remove(position);
        }
    }

    /// How many effects this operation queued elsewhere have not ended.
    ///
    /// `None` where there is no record to ask about, or the registry cannot be
    /// read: not knowing is not zero.
    pub fn dependents_outstanding(&self, token: ControlCompletionToken) -> Option<usize> {
        if token.origin != self.origin {
            return None;
        }
        let inner = self.inner.lock().ok()?;
        inner
            .records
            .iter()
            .find(|held| held.token == token)
            .map(|held| held.dependents)
    }
}
