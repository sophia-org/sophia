//! A reloaded profile's output topology is Session's own transaction. It is
//! admitted with the output owner's epoch, not the WM's. Local settlement with a
//! connected file client is covered by output_file_reload_settlement.rs.
use super::*;
use sophia_backend_live::{
    LibdrmNativeOutputCapability, LibdrmNativeOutputTiming, LibdrmNativeVrrPropertyDiscoveryStatus,
    project_live_output_authority_snapshot,
};
use sophia_protocol::{
    OutputAuthoritySnapshot, OutputHeadTargetProposal, OutputLogicalGroupProposal,
    OutputTopologyCandidate, OutputTopologyIntent, OutputTransform, OutputVrrPolicy,
};

/// One connected head, its published snapshot and an apply candidate that
/// names it.
fn reload_inputs(
    public: &LivePublicPolicyState,
) -> (
    LibdrmNativeOutputCapability,
    OutputAuthoritySnapshot,
    OutputTopologyCandidate,
) {
    let output = public.outputs[0];
    let timing = LibdrmNativeOutputTiming::new(
        u32::try_from(output.size.width).unwrap(),
        u32::try_from(output.size.height).unwrap(),
        60_000,
    );
    let capability = LibdrmNativeOutputCapability::new(
        output.id,
        11,
        "DP-1",
        [timing],
        Some(timing),
        timing,
        LibdrmNativeVrrPropertyDiscoveryStatus::Discovered,
    )
    .unwrap()
    .bind_head(sophia_engine::RenderHeadId::from_raw(11))
    .unwrap();
    let snapshot =
        project_live_output_authority_snapshot(std::slice::from_ref(&capability), &[output], 7)
            .unwrap();
    let head = &snapshot.heads[0];
    let group = &snapshot.groups[0];
    let candidate = OutputTopologyCandidate {
        base_topology_epoch: snapshot.topology_epoch,
        intent: OutputTopologyIntent::Apply,
        primary_group_index: 0,
        heads: vec![OutputHeadTargetProposal {
            head: head.head,
            head_generation: head.generation,
            mode: head.current_mode.unwrap(),
            transform: OutputTransform::Normal,
            vrr: OutputVrrPolicy::Disabled,
        }],
        groups: vec![OutputLogicalGroupProposal {
            output: group.output,
            logical: group.logical,
            members: group.members.clone(),
        }],
    };
    (capability, snapshot, candidate)
}

/// The output role's connection epoch and the WM policy's connection epoch
/// advance independently: output on peer departure and assignee replacement,
/// the WM on policy restarts. A reload is admitted against the former in both
/// orders of divergence.
#[test]
fn output_reload_is_admitted_with_the_output_owner_epoch_not_the_wm_epoch() {
    for (output_epoch, wm_epoch) in [(3, 1), (1, 4)] {
        let mut fixture = ReloadFixture::new();
        let public = fixture.wm.public.as_mut().unwrap();
        let (capability, snapshot, candidate) = reload_inputs(public);
        public.connection_epoch = wm_epoch;
        public.output_authority = Some(
            crate::live_output_authority::LiveOutputAuthorityOwner::new(output_epoch, snapshot)
                .unwrap(),
        );
        public.output_capabilities = vec![capability];
        assert!(
            public.admit_reloaded_output_topology(candidate).unwrap(),
            "reload declined with output epoch {output_epoch} and WM epoch {wm_epoch}"
        );
        assert!(
            public
                .output_authority
                .as_ref()
                .unwrap()
                .active_transaction()
                .is_some()
        );
    }
}
