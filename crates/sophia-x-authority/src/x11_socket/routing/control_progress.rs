// How far an operation has got, and who is allowed to say.
//
// Split from the records by subject: a record is what is owed for an
// operation, and this is what that operation has actually done. Kept apart
// because the whole point is that the second is not something a caller
// asserts -- it is a closed, ordered set recorded by the code performing each
// step, with beginning noted before the effect can happen.

/// How far one step of an operation got.
///
/// Three states, because two cannot hold what matters. An effect that was
/// interrupted and one that never happened are not the same thing, and a
/// record that says only "not reported" cannot tell them apart.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum ControlStepState {
    /// Nothing was begun. Begun is recorded before the effect can happen, so
    /// this is evidence that it did not, rather than an absence of evidence.
    #[default]
    NotStarted,
    /// Begun, and not known to have finished. The effect may have happened.
    InProgress,
    /// Finished, and reported by the code that finished it.
    Completed,
}

/// What an operation has done, in the order it has to happen.
///
/// A closed set owned by the code that performs each step, not an arbitrary
/// mutation. Handing a caller the state to set was the same assertion seam
/// under another name: both bits could be set without either effect
/// happening, a reported step could be taken back, and a projection could be
/// claimed without the runtime change it projects.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ControlProgress {
    /// About to change the shared runtime.
    RuntimeBegun,
    /// The shared runtime changed.
    RuntimeApplied,
    /// About to bring this connection's projection of it into agreement.
    ProjectionBegun,
    /// The projection agrees.
    ProjectionApplied,
}

/// Why progress could not be recorded.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlProgressRefusal {
    /// This registry did not issue the registration.
    Foreign,
    /// No record is held for it.
    NoLongerHeld,
    /// The operation is not being applied, so it is not doing anything.
    NotApplying,
    /// Impossible or out of order: a step cannot be taken back, and one
    /// cannot finish before it begins or begin before the one it depends on
    /// has finished.
    OutOfOrder,
    /// The registry cannot be reached, so nothing can be established.
    Unavailable,
}

/// How far each of an operation's steps got.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ControlSteps {
    /// The shared runtime, which outlives the connection.
    pub runtime: ControlStepState,
    /// This connection's projection of it, which does not.
    pub projection: ControlStepState,
}

#[cfg(unix)]
impl ControlSteps {
    /// Advance by one reported step, or refuse.
    ///
    /// Monotonic and ordered. Nothing goes backwards, nothing finishes before
    /// it begins, and the projection cannot begin before the runtime change it
    /// projects has finished.
    fn advance(&mut self, progress: ControlProgress) -> Result<(), ControlProgressRefusal> {
        let ordered = match progress {
            ControlProgress::RuntimeBegun => {
                self.runtime == ControlStepState::NotStarted
            }
            ControlProgress::RuntimeApplied => self.runtime == ControlStepState::InProgress,
            ControlProgress::ProjectionBegun => {
                self.runtime == ControlStepState::Completed
                    && self.projection == ControlStepState::NotStarted
            }
            ControlProgress::ProjectionApplied => {
                self.projection == ControlStepState::InProgress
            }
        };
        if !ordered {
            return Err(ControlProgressRefusal::OutOfOrder);
        }
        match progress {
            ControlProgress::RuntimeBegun => self.runtime = ControlStepState::InProgress,
            ControlProgress::RuntimeApplied => self.runtime = ControlStepState::Completed,
            ControlProgress::ProjectionBegun => self.projection = ControlStepState::InProgress,
            ControlProgress::ProjectionApplied => self.projection = ControlStepState::Completed,
        }
        Ok(())
    }
}

#[cfg(unix)]
impl ControlCompletionRegistry {
    /// Record that this operation has reached one step.
    ///
    /// Recorded by the code that performs the step. Beginning is recorded
    /// before the effect can happen and finishing after it has, so an effect
    /// that was interrupted is distinguishable from one that never happened --
    /// reporting only after success cannot tell those apart, and the gap
    /// between them is exactly where an operation dies.
    ///
    /// Ordered and monotonic: nothing goes backwards, nothing finishes before
    /// it begins, and no step begins before the one it depends on has
    /// finished. Anything else is refused rather than recorded.
    pub fn record_progress(
        &self,
        token: ControlCompletionToken,
        progress: ControlProgress,
    ) -> Result<(), ControlProgressRefusal> {
        if token.origin != self.origin {
            return Err(ControlProgressRefusal::Foreign);
        }
        let Ok(mut inner) = self.inner.lock() else {
            return Err(ControlProgressRefusal::Unavailable);
        };
        let Some(record) = inner.records.iter_mut().find(|held| held.token == token) else {
            return Err(ControlProgressRefusal::NoLongerHeld);
        };
        if !matches!(record.phase, ControlPhase::Applying(_)) {
            return Err(ControlProgressRefusal::NotApplying);
        }
        record.steps.advance(progress)
    }

    /// What one operation reported doing, or `None` where there is no record
    /// to ask or the registry cannot be read.
    pub fn steps_of(&self, token: ControlCompletionToken) -> Option<ControlSteps> {
        if token.origin != self.origin {
            return None;
        }
        let inner = self.inner.lock().ok()?;
        inner
            .records
            .iter()
            .find(|held| held.token == token)
            .map(|held| held.steps)
    }
}
