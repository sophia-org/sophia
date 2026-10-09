#![cfg(test)]

use super::super::tests::{probe, profile};
use super::*;
use sophia_backend_live::LibdrmNativeVrrPropertyDiscoveryStatus;
use sophia_engine::{HeadlessOutput, RenderHeadId};
use sophia_protocol::{OutputId, Size};

fn fixture() -> (
    Vec<LibdrmNativeOutputCapability>,
    Vec<HeadlessOutput>,
    OutputAuthoritySnapshot,
) {
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 1920,
            height: 1080,
        },
        scale: 1,
    };
    let mode = LibdrmNativeOutputTiming::new(1920, 1080, 60_000);
    let capability = LibdrmNativeOutputCapability::new(
        output.id,
        1,
        "DP-2",
        [mode],
        Some(mode),
        mode,
        LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
    )
    .unwrap()
    .bind_head(RenderHeadId::from_raw(17))
    .unwrap();
    let snapshot = sophia_backend_live::project_live_output_authority_snapshot(
        std::slice::from_ref(&capability),
        &[output],
        4,
    )
    .unwrap();
    (vec![capability], vec![output], snapshot)
}

fn realization(probes: &[LiveNativeOutputProbe]) -> DesktopOutputReconciliation {
    let mut profile = profile();
    profile.inherit_sophia = true;
    let DesktopOutputResolution::Active(realization) =
        sophia_config::resolve_desktop_output_candidate(
            &profile,
            &project_profile_probes(probes, &profile, None),
            None,
        )
        .unwrap()
    else {
        panic!()
    };
    realization
}

#[test]
fn reload_sees_a_connected_head_missing_from_the_active_native_owner() {
    let (capabilities, outputs, snapshot) = fixture();
    let desired = realization(&[probe("DP-1"), probe("DP-2")]);
    assert!(matches!(
        classify_reload(
            desired,
            &capabilities,
            &outputs,
            &snapshot,
            OutputHeadMapping::Fit
        )
        .unwrap(),
        ReloadOutputReplacement::Rebuild
    ));
}

#[test]
fn same_head_settings_use_the_existing_rollback_transaction_and_keep_realization_identity() {
    let (capabilities, outputs, snapshot) = fixture();
    let mut desired = realization(&[probe("DP-2")]);
    desired.outputs[0].position = (32, 0);
    let ReloadOutputReplacement::Candidate {
        candidate,
        realization,
    } = classify_reload(
        desired.clone(),
        &capabilities,
        &outputs,
        &snapshot,
        OutputHeadMapping::Fit,
    )
    .unwrap()
    else {
        panic!()
    };
    assert_eq!(*realization, desired);
    assert_eq!(candidate.base_topology_epoch, 4);
    assert_eq!(candidate.groups[0].logical.x, 32);
    assert_eq!(candidate.heads[0].head.raw(), 17);
}

#[test]
fn stale_native_mode_inventory_refuses_before_a_candidate_is_admitted() {
    let (capabilities, outputs, snapshot) = fixture();
    let mut desired = realization(&[probe("DP-2")]);
    desired.outputs[0].mode.refresh_millihz = 120_000;
    assert!(
        classify_reload(
            desired,
            &capabilities,
            &outputs,
            &snapshot,
            OutputHeadMapping::Fit
        )
        .is_err()
    );
}

#[test]
fn the_same_connectors_with_a_different_mirror_group_or_mapping_require_rebuild() {
    let (mut capabilities, outputs, snapshot) = fixture();
    let desired = realization(&[probe("DP-2")]);
    assert!(matches!(
        classify_reload(
            desired,
            &capabilities,
            &outputs,
            &snapshot,
            OutputHeadMapping::Cover
        )
        .unwrap(),
        ReloadOutputReplacement::Rebuild
    ));
    let mode = capabilities[0].selected_mode();
    capabilities.push(
        LibdrmNativeOutputCapability::new(
            outputs[0].id,
            2,
            "DP-1",
            [mode],
            Some(mode),
            mode,
            LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
        )
        .unwrap()
        .bind_head(RenderHeadId::from_raw(18))
        .unwrap(),
    );
    let snapshot =
        sophia_backend_live::project_live_output_authority_snapshot(&capabilities, &outputs, 4)
            .unwrap();
    assert_eq!(snapshot.groups.len(), 1);
    let desired = realization(&[probe("DP-1"), probe("DP-2")]);
    assert!(matches!(
        classify_reload(
            desired,
            &capabilities,
            &outputs,
            &snapshot,
            OutputHeadMapping::Fit
        )
        .unwrap(),
        ReloadOutputReplacement::Rebuild
    ));
}
