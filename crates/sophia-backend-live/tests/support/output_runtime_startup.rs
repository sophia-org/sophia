//! Simulates an inactive CRTC without opening a DRM device.
use super::*;

impl LiveProductionOutputRuntimeSet {
    pub(crate) fn require_native_initialization_for_test(&mut self, output: OutputId) {
        self.outputs.get_mut(&output).unwrap().native_initialized = false;
    }
}
