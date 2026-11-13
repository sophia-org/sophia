//! Real connection registry/negotiation, supplied protected-peer evidence and
//! afid verdicts. No process is spawned: this fixture does not prove launch,
//! protection or launch authentication. The authenticated negotiation is the
//! owner's own, so its sealed-image channel is configured by Session.
use super::*;
use std::path::PathBuf;

impl ShellComponentProcesses {
    pub(crate) fn reconnect_fixture_endpoint(
        &mut self,
        slot: usize,
        policy: ShellContentAdmissionPolicy,
    ) -> (ComponentConnectionKey, PathBuf) {
        let key = self.connections.reserve_attempt(slot).unwrap();
        self.slots[slot].key = Some(key);
        self.connections
            .begin_authenticated_negotiation(
                key,
                &ProtectionDomainEvidence {
                    backend: ProtectionBackendKind::Bubblewrap,
                    supervisor_pid: std::process::id(),
                    peer_pid: std::process::id(),
                    roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
                },
                Duration::from_secs(2),
                policy,
                Box::new(crate::test_image_authority::Factory),
            )
            .unwrap();
        (key, self.connections.socket_path(slot).unwrap().to_owned())
    }
}

/// G3-5b's controlled driver and its rows, through the process owner the
/// harness holds.
#[cfg(all(
    feature = "experimental-combined-frame-job",
    feature = "experimental-native-frame-transaction"
))]
impl ShellComponentProcesses {
    pub(crate) fn turn_combined<N: sophia_backend_live::ShellContentNative>(
        &mut self,
        turn: &mut crate::shell_component_connections::combined_row::CombinedTurn<'_, N>,
    ) -> [Option<(ComponentConnectionKey, ShellTransportError)>; MAX_SHELL_COMPONENTS] {
        self.connections.turn_combined(turn)
    }

    pub(crate) fn combined_row(
        &self,
        slot: usize,
    ) -> &crate::shell_component_connections::combined_row::CombinedRow {
        self.connections.combined_row(slot)
    }

    pub(crate) fn combined_row_mut(
        &mut self,
        slot: usize,
    ) -> &mut crate::shell_component_connections::combined_row::CombinedRow {
        self.connections.combined_row_mut(slot)
    }

    pub(crate) fn image_pending(&self, slot: usize) -> usize {
        self.connections.image_pending(slot)
    }

    /// G3-5d d-2b1: the split driver's images step.
    pub(crate) fn turn_combined_images<N: sophia_backend_live::ShellContentNative>(
        &mut self,
        turn: &mut crate::shell_component_connections::combined_row::CombinedTurn<'_, N>,
    ) -> [Option<(ComponentConnectionKey, ShellTransportError)>; MAX_SHELL_COMPONENTS] {
        self.connections.turn_combined_images(turn)
    }

    /// G3-5d d-2b1: the split driver's walk step.
    pub(crate) fn turn_combined_walk<N: sophia_backend_live::ShellContentNative>(
        &mut self,
        native: &mut N,
        instant: std::time::Instant,
    ) -> [Option<(ComponentConnectionKey, ShellTransportError)>; MAX_SHELL_COMPONENTS] {
        self.connections.turn_combined_walk(native, instant)
    }

    /// G3-5d d-2b3d: the slot's transport facts beside its attempt.
    pub(crate) fn transport_facts(
        &self,
        slot: usize,
    ) -> Option<(
        Option<(sophia_protocol::ContentGrant, ComponentConnectionPhase)>,
        sophia_runtime::TransportFacts,
    )> {
        self.connections.transport_facts(slot)
    }

    /// G3-5d d-2b3d: the slot grant's queued store events.
    pub(crate) fn store_event_facts(&self, slot: usize) -> Option<sophia_runtime::StoreEventFacts> {
        self.connections.store_event_facts(slot)
    }

    /// G3-5d d-2b3d: the slot's owned source work awaiting SlotReleased.
    pub(crate) fn owned_releasing(
        &self,
        slot: usize,
    ) -> Option<Vec<sophia_runtime::owned_ring_protocol::Identity>> {
        self.connections.owned_releasing(slot)
    }

    /// G3-5d d-2b3d: the exact candidate's internal prepared state.
    pub(crate) fn candidate_prepared(
        &self,
        slot: usize,
        output: sophia_protocol::ContentOutputId,
        generation: u64,
    ) -> Option<bool> {
        self.connections
            .candidate_prepared(slot, output, generation)
    }

    /// G3-5d d-2b2: the slot's validation worker, for a control's handles.
    pub(crate) fn combined_validator(
        &mut self,
        slot: usize,
    ) -> Option<&mut sophia_runtime::IsbValidationWorker<sophia_backend_live::CombinedRenderPlan>>
    {
        self.connections.combined_validator(slot)
    }
}
