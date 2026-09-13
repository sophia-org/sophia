// Work one operation queued on someone else, and how it reports its own end.
//
// Split from the completion records by subject: a record is what is owed for
// one operation, and this is what that operation set in motion elsewhere.
// Kept apart because the two are settled by different things -- an operation's
// own writer cannot answer for an effect sitting in another connection's
// queue, and nothing about the operation is settled while one can still run.

#[cfg(unix)]
impl ControlCompletionRegistry {
    /// Record that this operation has queued an effect on someone else.
    ///
    /// Taken at the moment the effect is queued, so there is no window where
    /// the work exists and nothing is counting it.
    pub fn note_dependent(&self, token: ControlCompletionToken) -> bool {
        if token.origin != self.origin {
            return false;
        }
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        let Some(record) = inner.records.iter_mut().find(|held| held.token == token) else {
            return false;
        };
        record.dependents = record.dependents.saturating_add(1);
        true
    }

    /// Record that one of this operation's queued effects has ended.
    ///
    /// Ended either way: run by the writer it was queued on, or given up
    /// unrun when that queue went. Both are ends, and only the count of
    /// outstanding ones decides whether anything about this operation can be
    /// settled -- which of the two it was is not a receipt and is not treated
    /// as one.
    pub fn dependent_ended(&self, token: ControlCompletionToken) {
        if token.origin != self.origin {
            return;
        }
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if let Some(record) = inner.records.iter_mut().find(|held| held.token == token) {
            record.dependents = record.dependents.saturating_sub(1);
        }
    }

    /// How many effects this operation queued elsewhere have not ended.
    ///
    /// `None` where the registry cannot be read: not knowing is not zero.
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
