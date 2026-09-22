//! What a direct-grant refusal does to a component selection, at prepare.
//!
//! A component declaring `gpu "direct"` used to be started, negotiated and
//! failed in service at every backoff interval for ever when the grant could
//! not be constituted, because the only decision made at prepare was
//! all-or-nothing and session-fatal. These pin the replacement: the refusal is
//! decided once, names every component it refuses, and leaves the components
//! that asked for no grant alone.

use super::super::component_lifecycle::{admitted_without_direct, refusal_records};
use sophia_config::{ShellComponentConfig, ShellComponentRole, ShellGpuMode};

fn component(id: &str, role: ShellComponentRole, gpu: ShellGpuMode) -> ShellComponentConfig {
    ShellComponentConfig {
        id: id.into(),
        role,
        executable: std::path::PathBuf::from("/usr/bin/component"),
        config: None,
        gpu,
        reservation: None,
    }
}

/// The three refusals the coordinator raises, each of which now reaches a
/// record rather than failing session start.
const REFUSALS: [&str; 3] = [
    "the active render device is unavailable",
    "the active render device is absent from the admitted inventory",
    "the admitted inventory contains an ambiguous active render device",
];

#[test]
fn every_coordinator_refusal_names_the_grant_rather_than_going_unclassified() {
    // `other` is a gap in the cause table, not a kind of failure. The
    // ambiguous case is the dual-GPU host the investigation opened on.
    for refusal in REFUSALS {
        let selected = [component(
            "bar",
            ShellComponentRole::Bar,
            ShellGpuMode::Direct,
        )];
        let records = refusal_records(&selected, refusal);
        assert_eq!(records.len(), 1, "{refusal}");
        assert!(
            records[0].contains("cause=gpu_grant"),
            "{refusal} classified as something else: {}",
            records[0]
        );
    }
}

#[test]
fn a_refusal_names_every_direct_component_and_no_denied_one() {
    let selected = [
        component("bar", ShellComponentRole::Bar, ShellGpuMode::Direct),
        component(
            "launcher",
            ShellComponentRole::ApplicationLauncher,
            ShellGpuMode::Denied,
        ),
        component("dock", ShellComponentRole::Dock, ShellGpuMode::Direct),
    ];
    let records = refusal_records(&selected, REFUSALS[2]);
    assert_eq!(records, vec![
        "sophia_shell_component schema=1 status=start_refused cause=gpu_grant slot=0 role=bar gpu_mode=direct".to_string(),
        "sophia_shell_component schema=1 status=start_refused cause=gpu_grant slot=2 role=dock gpu_mode=direct".to_string(),
    ]);

    // The denied launcher keeps the CPU rasterize path, which needs no
    // device, and the slots reported above are its neighbours' declared
    // positions rather than the runtime slots nobody now occupies.
    let admitted = admitted_without_direct(&selected);
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].id, "launcher");
}

#[test]
fn a_selection_that_asked_for_no_grant_is_refused_nothing() {
    let selected = [
        component("bar", ShellComponentRole::Bar, ShellGpuMode::Denied),
        component("dock", ShellComponentRole::Dock, ShellGpuMode::Denied),
    ];
    assert!(refusal_records(&selected, REFUSALS[0]).is_empty());
    assert_eq!(admitted_without_direct(&selected).len(), 2);
}

#[test]
fn a_selection_of_only_direct_components_keeps_nothing() {
    // Prepare returns no component session here rather than an error: a
    // component that cannot get a device is not a reason to withhold the
    // desktop.
    let selected = [
        component("bar", ShellComponentRole::Bar, ShellGpuMode::Direct),
        component("dock", ShellComponentRole::Dock, ShellGpuMode::Direct),
    ];
    assert_eq!(refusal_records(&selected, REFUSALS[1]).len(), 2);
    assert!(admitted_without_direct(&selected).is_empty());
}

#[test]
fn a_refused_record_survives_evidence_reduction_whole() {
    // The record is useless if reduction drops it, and a status the allowlist
    // does not admit is dropped silently -- which is the original defect in
    // miniature. Assert through the reducer, not against the format string.
    let selected = [component(
        "bar",
        ShellComponentRole::Bar,
        ShellGpuMode::Direct,
    )];
    let record = refusal_records(&selected, REFUSALS[2]).remove(0);
    assert_eq!(
        crate::diagnostics::reduced_record(&record).unwrap(),
        record,
        "reduction dropped a field the refusal needs"
    );
}
