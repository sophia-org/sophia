//! The carrier for test-only lifetime faults.
//!
//! cfg(test) ONLY, AND THERE IS NO RELEASE PATH TO ANY OF IT. This struct is
//! empty outside test builds, so a release binary has no field to set, no API
//! to call and nothing to configure. That is why it is shaped as a carrier
//! rather than as a flag on the config: a public configuration field would be
//! a fault-injection API however it was documented.
//!
//! The fault bodies themselves live outside production source, under
//! tests/support, mounted from the module root.

/// What faults a service was started with.
///
/// Empty in release builds, by construction.
#[derive(Clone, Default)]
pub(super) struct PrivateInputFaults {
    #[cfg(test)]
    pub(super) unwind: Option<std::sync::Arc<PrivateInputUnwindFault>>,
}

#[cfg(test)]
pub(super) use super::tests_faults::{PrivateInputUnwindFault, arm_globally};
