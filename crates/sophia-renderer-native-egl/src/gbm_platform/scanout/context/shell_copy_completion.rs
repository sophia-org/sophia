//! Exact-copy completion observation, private and experiment-only.
//!
//! The copy inserts and flushes its EGL fence in capture_pool/target.rs. Only
//! CONDITION_SATISFIED for that fence proves its preceding reads ended. Neither
//! a timeout, an error, fence destruction nor EGL teardown supplies this proof.
//! This module does not release a source, settle a shell job, or publish pixels.
//! Source custody and the native-owner teardown binding remain with the caller.

use super::{
    NativeCaptureAllocation, NativePooledCapture, NativeSnapshotGeneration, NativeSnapshotUse,
};
use std::rc::Rc;

#[derive(Default, Debug, Eq, PartialEq)]
pub(super) enum Observation {
    #[default]
    Pending,
    Issued,
    Uncertain,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Refusal {
    AlreadyIssued,
    Uncertain,
    MissingFence,
    WrongGeneration,
}

/// Non-cloneable and thread-bound. Fields and construction stay in this module.
/// Keeping allocation and generation alive prevents ABA even if EGL reuses a
/// sync handle or a future allocation reuses the same numeric IDs.
#[must_use]
pub(super) struct CompletedCopy<A, S> {
    allocation: Rc<A>,
    generation: Rc<NativeSnapshotGeneration>,
    sync: S,
}

impl<A, S: PartialEq> CompletedCopy<A, S> {
    pub(super) fn is_for(
        &self,
        allocation: &Rc<A>,
        generation: &Rc<NativeSnapshotGeneration>,
        sync: &S,
    ) -> bool {
        Rc::ptr_eq(&self.allocation, allocation)
            && Rc::ptr_eq(&self.generation, generation)
            && self.sync == *sync
    }
}

impl Observation {
    // Private fault seam. No parent/sibling or exported caller may supply a
    // successful poll result. Production always uses the real EGL call below.
    fn observe<A, S: Copy + PartialEq>(
        &mut self,
        allocation: &Rc<A>,
        generation: &Rc<NativeSnapshotGeneration>,
        sync: Option<S>,
        copy_use: Option<&NativeSnapshotUse>,
        poll: impl FnOnce(S) -> Result<khronos_egl::Int, ()>,
    ) -> Result<Option<CompletedCopy<A, S>>, Refusal> {
        match self {
            Self::Issued => return Err(Refusal::AlreadyIssued),
            Self::Uncertain => return Err(Refusal::Uncertain),
            Self::Pending => {}
        }
        let Some(sync) = sync else {
            *self = Self::Uncertain;
            generation.abandon_reuse();
            return Err(Refusal::MissingFence);
        };
        if !copy_use.is_some_and(|guard| Rc::ptr_eq(guard.generation(), generation)) {
            *self = Self::Uncertain;
            generation.abandon_reuse();
            return Err(Refusal::WrongGeneration);
        }
        match poll(sync) {
            Ok(khronos_egl::TIMEOUT_EXPIRED) => Ok(None),
            Ok(khronos_egl::CONDITION_SATISFIED) => {
                *self = Self::Issued;
                Ok(Some(CompletedCopy {
                    allocation: allocation.clone(),
                    generation: generation.clone(),
                    sync,
                }))
            }
            // Includes unexpected success codes: only CONDITION_SATISFIED is
            // sufficient. Leave the fence/use in its existing owner untouched.
            _ => {
                *self = Self::Uncertain;
                generation.abandon_reuse();
                Err(Refusal::Uncertain)
            }
        }
    }
}

impl NativePooledCapture {
    /// Zero-wait observation on the exact execution/display that made the copy.
    /// Does not take/destroy the fence, drop its GPU use, or change source custody.
    pub(super) fn observe_shell_copy_completion(
        &mut self,
    ) -> Result<Option<CompletedCopy<NativeCaptureAllocation, khronos_egl::Sync>>, Refusal> {
        let execution = &self.allocation.execution;
        self.shell_copy_observation.observe(
            &self.allocation,
            &self.generation,
            self.copy_completion,
            self.copy_use.as_ref(),
            |sync| unsafe {
                execution
                    .egl
                    .client_wait_sync(execution.display, sync, 0, 0)
                    .map_err(|_| ())
            },
        )
    }
}

#[cfg(test)]
#[path = "../../../../tests/support/shell_copy_completion.rs"]
mod tests;
