//! Existing mirror controls reuse the shared test-only target.
use super::*;
use crate::production_visual_runtime::mirrored_composition_test_target::MirroredTarget;

#[path = "presentation_geometry.rs"]
mod presentation_geometry;
#[path = "presentation_instances.rs"]
mod presentation_instances;
#[path = "presentation_present.rs"]
mod presentation_present;
#[path = "mirrored_intake_tests.rs"]
mod tests;

impl IntegrationTarget for MirroredTarget {
    fn queued(&self) -> &crate::DeferredNativeCompositions {
        &self.queue
    }
    fn drain(&mut self) {
        MirroredTarget::drain(self);
    }
    fn teardown(&mut self) {
        MirroredTarget::teardown(self);
    }
    fn backing_count(&self) -> usize {
        self.owners.get()
    }
}
