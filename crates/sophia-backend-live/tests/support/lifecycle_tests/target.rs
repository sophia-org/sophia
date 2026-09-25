pub(super) use super::super::composition_test_target::Target;
use super::super::composition_test_target::{CopiedBacking, Device, copy_submission};
use super::*;
#[path = "mirror_completion_tests.rs"]
mod mirror_completion_tests;
#[path = "mirrored_target.rs"]
mod mirrored_target;
use std::{cell::Cell, num::NonZeroU32, rc::Rc};
impl IntegrationTarget for Target {
    fn queued(&self) -> &crate::DeferredNativeCompositions {
        &self.queue
    }
    fn drain(&mut self) {
        Target::drain(self);
    }
    fn teardown(&mut self) {
        Target::teardown(self);
    }
    fn backing_count(&self) -> usize {
        self.backing_owners.get()
    }
}
