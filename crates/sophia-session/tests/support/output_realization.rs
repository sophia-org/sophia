#![cfg(test)]

use super::*;
use sophia_config::{
    ConfigDigest, ConfigGeneration, DesktopOutputAvailability, DesktopOutputState,
    DesktopOutputTiming, DesktopOutputTransform, DesktopOutputVrrMode,
};
use std::collections::BTreeMap;

fn profile() -> DesktopOutputCandidate {
    DesktopOutputCandidate {
        generation: ConfigGeneration::INITIAL,
        digest: ConfigDigest::new([1; 32]),
        inherit_sophia: false,
        availability: DesktopOutputAvailability::Adaptive,
        fallback_policy_key: Some(1),
        named: Vec::new(),
    }
}

fn realized(connector: &str) -> DesktopOutputReconciliation {
    DesktopOutputReconciliation {
        generation: profile().generation,
        digest: profile().digest,
        outputs: vec![DesktopOutputState {
            connector: connector.into(),
            enabled: true,
            mode: DesktopOutputTiming::new(1920, 1080, 60_000),
            scale_milli: 1000,
            position: (0, 0),
            transform: DesktopOutputTransform::Normal,
            vrr: DesktopOutputVrrMode::Disabled,
            mirror_of: None,
        }],
        focused_connector: Some(connector.into()),
        fallback_connector: Some(connector.into()),
        policy_keys: BTreeMap::from([(connector.into(), 1)]),
        adjustments: Vec::new(),
    }
}

fn binding(transition: u64, notice: u64, owner: u64) -> OutputRealizationBinding {
    OutputRealizationBinding {
        transition,
        notice_sequence: notice,
        native_owner: owner,
    }
}

#[test]
fn no_affinity_moves_until_the_exact_replacement_presents_and_waiting_retains_the_old_one() {
    let mut ledger = OutputRealizationLedger::default();
    let initial = binding(0, 0, 1);
    ledger.stage(initial, realized("DP-1")).unwrap();
    assert!(ledger.committed().is_none());
    assert!(ledger.commit(initial, &profile()));
    let moved = binding(1, 8, 2);
    ledger.stage(moved, realized("DP-2")).unwrap();
    assert_eq!(
        ledger.committed().unwrap().policy_keys,
        [("DP-1".into(), 1)].into()
    );
    ledger.abandon();
    assert!(!ledger.commit(moved, &profile()));
    assert_eq!(
        ledger.committed().unwrap().policy_keys,
        [("DP-1".into(), 1)].into()
    );
    ledger.stage(binding(1, 9, 3), realized("DP-3")).unwrap();
    assert!(!ledger.commit(moved, &profile()));
    assert!(ledger.commit(binding(1, 9, 3), &profile()));
    assert_eq!(
        ledger.committed().unwrap().policy_keys,
        [("DP-3".into(), 1)].into()
    );
}

#[test]
fn late_notice_owner_and_profile_completions_cannot_consume_a_newer_realization() {
    let mut ledger = OutputRealizationLedger::default();
    let current = binding(4, 10, 12);
    ledger.stage(current, realized("DP-2")).unwrap();
    for stale in [binding(3, 10, 12), binding(4, 9, 12), binding(4, 10, 11)] {
        assert!(!ledger.commit(stale, &profile()));
        assert!(ledger.pending(current, &profile()).is_some());
    }
    for changed in [
        DesktopOutputCandidate {
            generation: ConfigGeneration::from_raw(2),
            ..profile()
        },
        DesktopOutputCandidate {
            digest: ConfigDigest::new([2; 32]),
            ..profile()
        },
    ] {
        assert!(!ledger.commit(current, &changed));
        assert!(ledger.pending(current, &profile()).is_some());
    }
    assert!(ledger.stage(binding(4, 9, 13), realized("DP-3")).is_err());
    assert!(ledger.stage(current, realized("DP-3")).is_err());
    assert!(ledger.commit(current, &profile()));
    assert!(!ledger.commit(current, &profile()));
}

#[test]
fn live_focus_is_observed_through_connector_identity_not_enumeration() {
    use sophia_backend_live::{LibdrmNativeOutputTiming, LibdrmNativeVrrPropertyDiscoveryStatus};
    let mut initial = realized("DP-1");
    initial.fallback_connector = None;
    initial.outputs.push(realized("DP-2").outputs.remove(0));
    let mut ledger = OutputRealizationLedger::default();
    ledger.stage(binding(0, 0, 1), initial).unwrap();
    assert!(ledger.commit(binding(0, 0, 1), &profile()));
    let mode = LibdrmNativeOutputTiming::new(1920, 1080, 60_000);
    let capability = LibdrmNativeOutputCapability::new(
        OutputId::from_raw(7),
        22,
        "DP-2",
        [mode],
        Some(mode),
        mode,
        LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
    )
    .unwrap();
    ledger.observe_focus(OutputId::from_raw(7), &[capability]);
    assert_eq!(
        ledger.committed().unwrap().focused_connector.as_deref(),
        Some("DP-2")
    );
    ledger.observe_focus(OutputId::from_raw(1), &[]);
    assert_eq!(
        ledger.committed().unwrap().focused_connector.as_deref(),
        Some("DP-2")
    );
}
