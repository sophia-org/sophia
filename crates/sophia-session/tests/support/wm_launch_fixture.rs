//! Explicit supplied-admission entry point for legacy role conformance fixtures.
#![cfg(test)]
use super::*;

impl LiveWmSession {
    pub(in crate::live_session) fn activate_public_launch(
        config: &mut PersistentXtermSessionConfig,
        prepared_launch: Option<PreparedPublicPolicyLaunch>,
    ) -> Result<Option<StartedPublicPolicyLaunch>, Box<dyn std::error::Error>> {
        Self::activate_public_launch_inner(config, prepared_launch, None)
    }
}
