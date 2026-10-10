#![cfg(test)]

use super::*;
use sophia_config::{
    ConfigDigest, ConfigGeneration, DesktopOutputAvailability, DesktopOutputState,
    DesktopOutputTiming, DesktopOutputTransform, DesktopOutputVrrMode,
};
use std::collections::BTreeMap;

#[path = "output_realization/publication.rs"]
mod publication;

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
fn rollback_or_a_late_policy_transaction_cannot_publish_a_prepared_reload() {
    use sophia_protocol::TransactionId;
    let mut ledger = OutputRealizationLedger::default();
    ledger.prepare_policy(TransactionId::from_raw(9), realized("DP-2"));
    assert!(ledger.committed().is_none());
    assert!(
        ledger
            .take_policy(TransactionId::from_raw(8), &profile())
            .is_none()
    );
    let desired = ledger
        .take_policy(TransactionId::from_raw(9), &profile())
        .unwrap();
    // Taking a refused transaction only disposes of speculation.
    assert!(ledger.committed().is_none());
    ledger.prepare_policy(TransactionId::from_raw(10), desired);
    let changed = DesktopOutputCandidate {
        generation: ConfigGeneration::from_raw(2),
        ..profile()
    };
    assert!(
        ledger
            .take_policy(TransactionId::from_raw(10), &changed)
            .is_none()
    );
    assert!(
        ledger
            .take_policy(TransactionId::from_raw(10), &profile())
            .is_none()
    );
    assert!(ledger.committed().is_none());
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

#[test]
fn resolved_geometry_is_normalized_once_for_wm_frontend_and_authority() {
    use sophia_backend_live::{LibdrmNativeOutputTiming, LibdrmNativeVrrPropertyDiscoveryStatus};
    use sophia_engine::HeadlessOutput;
    use sophia_protocol::{OutputHeadMapping, Size};
    let outputs = [1, 2].map(|id| HeadlessOutput {
        id: OutputId::from_raw(id),
        size: Size {
            width: 1920,
            height: 1080,
        },
        scale: 1,
    });
    let mode = LibdrmNativeOutputTiming::new(1920, 1080, 120_000);
    let capabilities = outputs
        .iter()
        .enumerate()
        .map(|(index, output)| {
            LibdrmNativeOutputCapability::new(
                output.id,
                index as u32 + 1,
                format!("DP-{}", index + 1),
                [mode],
                Some(mode),
                mode,
                LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
            )
            .unwrap()
            .bind_head(sophia_engine::RenderHeadId::from_raw(index as u64 + 1))
            .unwrap()
        })
        .collect::<Vec<_>>();
    let mut realization = realized("DP-1");
    realization.fallback_connector = None;
    realization.outputs.push(realized("DP-2").outputs.remove(0));
    for (index, output) in realization.outputs.iter_mut().enumerate() {
        output.mode = DesktopOutputTiming::new(1920, 1080, 120_000);
        output.position = (-1920 + index as i32 * 1920, 32);
    }
    realization.focused_connector = Some("DP-2".into());
    let policy = OutputPolicyLayout::prepare(
        &realization,
        &capabilities,
        &outputs,
        OutputHeadMapping::Exact,
    )
    .unwrap();
    let frontend = policy.frontend_snapshot(&outputs, 8).unwrap();
    let mut authority =
        sophia_backend_live::project_live_output_authority_snapshot(&capabilities, &outputs, 7)
            .unwrap();
    sophia_backend_live::apply_live_output_authority_head_mappings(
        &mut authority,
        &capabilities
            .iter()
            .map(|capability| (capability.head().unwrap(), OutputHeadMapping::Exact))
            .collect(),
    )
    .unwrap();
    policy.apply_authority_geometry(&mut authority).unwrap();
    assert!(
        matches_presented(
            &realization,
            &capabilities,
            &outputs,
            &authority,
            OutputHeadMapping::Exact
        )
        .unwrap()
    );
    let mut wrong = authority.clone();
    wrong.groups[0].logical.y += 8;
    assert!(
        !matches_presented(
            &realization,
            &capabilities,
            &outputs,
            &wrong,
            OutputHeadMapping::Exact
        )
        .unwrap()
    );
    assert_eq!(policy.primary, OutputId::from_raw(2));
    assert_eq!(frontend.primary, policy.primary);
    assert_eq!(authority.primary_output, policy.primary);
    assert_eq!(policy.bounds[0].1.x, 0);
    assert_eq!(policy.bounds[1].1.x, 1920);
    for (id, bounds) in &policy.bounds {
        assert_eq!(
            frontend
                .outputs
                .iter()
                .find(|output| output.output == *id)
                .unwrap()
                .logical,
            *bounds
        );
        assert_eq!(
            authority
                .groups
                .iter()
                .find(|group| group.output == *id)
                .unwrap()
                .logical,
            *bounds
        );
    }
    assert!(
        frontend
            .outputs
            .iter()
            .all(|output| output.refresh_millihz == 120_000)
    );
}
