//! A temporary borrow of the two actual owners, never another epoch registry.
use super::{ShellComponentTransport, ShellSessionTransport};
use crate::ContentEpochRegistry;

/// Connection service borrows the Session's common registry. Dropping this view
/// neither disconnects the peer nor releases resources. No mutable registry or
/// transport escapes independently through this façade.
pub struct ShellTransportConnection<'a> {
    pub(super) state: &'a mut ShellComponentTransport,
    pub(super) content_epochs: &'a mut ContentEpochRegistry,
}

impl ShellTransportConnection<'_> {
    /// Service the component's persistent image owner using the same clock as
    /// ordinary content resources. The owner itself must outlive this borrow.
    pub fn service_isb_images<P: Send + 'static>(
        &mut self,
        owner: &mut super::ShellIsbImageOwner<P>,
        now_msec: u64,
        now: std::time::Instant,
    ) -> Result<(), super::ShellTransportError> {
        owner.service(self.state, self.content_epochs, now_msec, now)
    }

    /// Observe existing admission; this never enables negotiation or changes a
    /// store profile. Persistent launch cannot inherit a transient menu grant.
    pub fn supports_persistent_catalog(&self) -> bool {
        self.state.supports_persistent_catalog(self.content_epochs)
    }

    /// Where a combined job's candidate stands, read from existing state
    /// only (G3-5c C4); `None` when the binding's grant has no active store.
    /// Absence is never death: see `frame_candidate_state`.
    #[cfg(all(
        feature = "experimental-combined-frame-job",
        feature = "experimental-native-frame-transaction"
    ))]
    pub fn content_frame_candidate_state(
        &self,
        binding: crate::FrameCopyBinding,
        job: u64,
    ) -> Option<crate::FrameCandidateState> {
        self.content_epochs
            .active_candidates(binding.key.grant)
            .map(|store| store.frame_candidate_state(&binding, job))
    }

    /// The exact frame stream's owner-side phase and close-notice
    /// transaction (`frame_stream_fact`); `None` without its active store
    /// or stream. Test support only (G3-5d d-2b3a).
    #[cfg(all(
        feature = "experimental-combined-frame-job",
        feature = "experimental-native-frame-transaction",
        any(test, feature = "test-support")
    ))]
    pub fn content_frame_stream_fact(
        &self,
        key: crate::frame_credits::FrameStreamKey,
    ) -> Option<(
        crate::frame_credits::FrameStreamPhase,
        sophia_protocol::TransactionId,
    )> {
        self.content_epochs
            .active_candidates(key.grant)?
            .frame_stream_fact(key)
    }
}

impl ShellComponentTransport {
    pub(super) fn supports_persistent_catalog(&self, epochs: &ContentEpochRegistry) -> bool {
        use sophia_protocol::*;
        let required = SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG
            | SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
            | SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION
            | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
            | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT;
        self.content_grant().is_some_and(|grant| {
            self.capabilities & required == required
                && epochs.profile(grant) == Some(crate::ContentStoreProfile::PersistentCatalog)
        })
    }
}

impl ShellComponentTransport {
    pub fn connection<'a>(
        &'a mut self,
        epochs: &'a mut ContentEpochRegistry,
    ) -> ShellTransportConnection<'a> {
        ShellTransportConnection {
            state: self,
            content_epochs: epochs,
        }
    }
}

impl ShellSessionTransport {
    /// Compatibility callers may lend the existing owner to the same service
    /// functions used by independently owned component connections.
    pub fn connection(&mut self) -> ShellTransportConnection<'_> {
        self.state.connection(&mut self.content_epochs)
    }
}
