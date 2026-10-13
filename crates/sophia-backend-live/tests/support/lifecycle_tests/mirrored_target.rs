//! Existing mirror controls reuse the shared test-only target.
use super::*;
use crate::production_visual_runtime::mirrored_composition_test_target::MirroredTarget;

#[path = "presentation_geometry.rs"]
mod presentation_geometry;
#[path = "presentation_installation.rs"]
mod presentation_installation;
#[path = "presentation_instances.rs"]
mod presentation_instances;
#[path = "presentation_present.rs"]
mod presentation_present;
#[path = "presentation_retirement.rs"]
mod presentation_retirement;
#[path = "session_lock_coverage.rs"]
mod session_lock_coverage;
#[path = "software_native_cycle.rs"]
mod software_native_cycle;
#[path = "software_output_idle.rs"]
mod software_output_idle;
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
