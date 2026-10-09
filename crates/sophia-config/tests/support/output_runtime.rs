use super::*;
use sophia_config::{
    DesktopOutputAdjustmentReason as Reason, DesktopOutputResolution,
    resolve_desktop_output_candidate,
};

fn active(
    profile: &DesktopOutputCandidate,
    heads: Vec<DesktopOutputTopologyConnector>,
    previous: Option<&sophia_config::DesktopOutputReconciliation>,
) -> sophia_config::DesktopOutputReconciliation {
    match resolve_desktop_output_candidate(profile, &topology(heads), previous).unwrap() {
        DesktopOutputResolution::Active(resolved) => resolved,
        DesktopOutputResolution::Waiting { .. } => panic!("expected an active output"),
    }
}

#[test]
fn absence_and_unusable_modes_wait_without_inventing_a_display() {
    let profile = daily(DesktopOutputAvailability::Adaptive);
    let mut unusable = head("DP-2", OFFICE, 0);
    unusable.modes.clear();
    unusable.preferred_mode = None;
    unusable.current.enabled = false;
    for heads in [vec![], vec![unplugged("DP-1")], vec![unusable.clone()]] {
        let resolution =
            resolve_desktop_output_candidate(&profile, &topology(heads), None).unwrap();
        assert!(
            matches!(resolution, DesktopOutputResolution::Waiting { generation, digest, .. }
            if generation == profile.generation && digest == profile.digest)
        );
    }
    let resolved = active(&profile, vec![unusable, head("DP-3", OFFICE, 0)], None);
    assert_eq!(enabled(&resolved), ["DP-3"]);
}

#[test]
fn sticky_fallback_restores_preferences_and_moves_the_only_affinity_claim() {
    let profile = daily(DesktopOutputAvailability::Adaptive);
    let first = active(&profile, vec![head("DP-3", OFFICE, 0)], None);
    let next = active(
        &profile,
        vec![head("DP-2", OFFICE, 0), head("DP-3", OFFICE, 1920)],
        Some(&first),
    );
    assert_eq!(next.fallback_connector.as_deref(), Some("DP-3"));
    assert_eq!(next.policy_keys, [("DP-3".into(), 1)].into());
    let returned = active(
        &profile,
        vec![head("DP-1", DAILY, 0), head("DP-3", OFFICE, 2560)],
        Some(&next),
    );
    assert_eq!(returned.policy_keys, [("DP-1".into(), 1)].into());
    assert_eq!(returned.focused_connector.as_deref(), Some("DP-1"));
    assert_eq!(returned.outputs[0].mode, DAILY);
    assert_eq!(returned.outputs[0].vrr, DesktopOutputVrrMode::Automatic);
    assert_eq!(enabled(&returned), ["DP-1"]);
    assert_eq!(profile, daily(DesktopOutputAvailability::Adaptive));
}

#[test]
fn exclusion_overrides_a_previously_committed_fallback() {
    let mut profile = daily(DesktopOutputAvailability::Adaptive);
    let first = active(&profile, vec![head("DP-2", OFFICE, 0)], None);
    profile.named.push(excluded("DP-2"));
    let next = active(
        &profile,
        vec![head("DP-2", OFFICE, 0), head("DP-3", OFFICE, 1920)],
        Some(&first),
    );
    assert_eq!(enabled(&next), ["DP-3"]);
    assert_eq!(next.policy_keys, [("DP-3".into(), 1)].into());
}

#[test]
fn returning_startup_focus_does_not_steal_runtime_focus() {
    let mut profile = daily(DesktopOutputAvailability::Adaptive);
    let mut second = excluded("DP-3");
    second.enabled = Some(true);
    second.policy_key = Some(2);
    second.position = Some((2560, 0));
    profile.named.push(second);
    let mut first = active(&profile, vec![head("DP-3", OFFICE, 0)], None);
    first.focused_connector = Some("DP-3".into());
    let next = active(
        &profile,
        vec![head("DP-1", DAILY, 0), head("DP-3", OFFICE, 2560)],
        Some(&first),
    );
    assert_eq!(next.focused_connector.as_deref(), Some("DP-3"));
    assert_eq!(next.policy_keys.len(), 2);
}

#[test]
fn safe_settings_restore_and_report_each_adjustment() {
    let mut profile = daily(DesktopOutputAvailability::Adaptive);
    profile.named[0].scale = Some(DesktopOutputScale::FixedMilli(1750));
    profile.named[0].transform = Some(DesktopOutputTransform::Rotate90);
    let mut reduced = head("DP-1", OFFICE, 0);
    reduced.scales.maximum_milli = 1500;
    reduced.transforms = DesktopOutputTransformSet::NORMAL;
    reduced.vrr_capable = false;
    let resolved = active(&profile, vec![reduced], None);
    let output = &resolved.outputs[0];
    assert_eq!(
        (
            output.mode,
            output.scale_milli,
            output.transform,
            output.vrr
        ),
        (
            OFFICE,
            1250,
            DesktopOutputTransform::Normal,
            DesktopOutputVrrMode::Disabled
        )
    );
    for reason in [Reason::Mode, Reason::Scale, Reason::Transform, Reason::Vrr] {
        assert!(
            resolved
                .adjustments
                .iter()
                .any(|item| item.connector == "DP-1" && item.reason == reason)
        );
    }
    let restored = active(&profile, vec![head("DP-1", DAILY, 0)], Some(&resolved));
    assert!(restored.adjustments.is_empty());
    assert_eq!(restored.outputs[0].scale_milli, 1750);
    assert_eq!(
        restored.outputs[0].transform,
        DesktopOutputTransform::Rotate90
    );
    assert_eq!(restored.outputs[0].mode, DAILY);
}

#[test]
fn safe_timing_is_independent_of_enumeration_and_stays_advertised() {
    let profile = daily(DesktopOutputAvailability::Adaptive);
    let larger = DesktopOutputTiming::new(2560, 1440, 60_000);
    let mut monitor = head("DP-2", OFFICE, 0);
    monitor.preferred_mode = None;
    monitor.modes = vec![DAILY, OFFICE, larger];
    let first = active(&profile, vec![monitor.clone()], None);
    monitor.modes.reverse();
    let second = active(&profile, vec![monitor], None);
    assert_eq!(first.outputs[0].mode, larger);
    assert_eq!(second.outputs[0].mode, larger);
}

#[test]
fn incomplete_mirror_is_not_partially_lit_and_return_restores_the_group() {
    let mut profile = daily(DesktopOutputAvailability::Adaptive);
    profile.named[0].mirror = vec!["DP-3".into()];
    let first = active(
        &profile,
        vec![head("DP-1", DAILY, 0), head("DP-2", OFFICE, 0)],
        None,
    );
    assert_eq!(enabled(&first), ["DP-2"]);
    assert!(
        first
            .adjustments
            .iter()
            .any(|item| item.reason == Reason::MirrorUnavailable)
    );
    let next = active(
        &profile,
        vec![
            head("DP-1", DAILY, 0),
            head("DP-2", OFFICE, 0),
            head("DP-3", OFFICE, 0),
        ],
        Some(&first),
    );
    assert_eq!(enabled(&next), ["DP-1", "DP-3"]);
    assert_eq!(next.outputs[2].mirror_of.as_deref(), Some("DP-1"));
    assert_eq!(next.policy_keys, [("DP-1".into(), 1)].into());
    profile.availability = DesktopOutputAvailability::Strict;
    profile.fallback_policy_key = None;
    assert!(matches!(
        reconcile_desktop_output_candidate(&profile, &topology(vec![head("DP-1", DAILY, 0)])),
        Err(DesktopOutputReconcileError::UnknownConnector(_))
    ));
}

#[test]
fn adaptive_repositions_overlaps_but_preserves_valid_coordinates() {
    let mut profile = daily(DesktopOutputAvailability::Adaptive);
    let mut second = excluded("DP-3");
    second.enabled = Some(true);
    second.position = Some((0, 0));
    profile.named.push(second);
    let heads = vec![head("DP-3", OFFICE, 0), head("DP-1", DAILY, 0)];
    let resolved = active(&profile, heads.clone(), None);
    assert_eq!(resolved.outputs[1].position, (0, 0));
    assert_eq!(resolved.outputs[0].position, (2048, 0));
    profile.named[1].position = Some((-1920, 17));
    let placed = active(&profile, heads, Some(&resolved));
    assert_eq!(placed.outputs[0].position, (-1920, 17));
    assert!(
        !placed
            .adjustments
            .iter()
            .any(|item| item.reason == Reason::Position)
    );
}

#[test]
fn fabricated_affinities_cannot_have_two_owners_or_a_dark_owner() {
    let profile = daily(DesktopOutputAvailability::Adaptive);
    let heads = topology(vec![head("DP-1", DAILY, 0), head("DP-2", OFFICE, 2560)]);
    let mut resolved = reconcile_desktop_output_candidate(&profile, &heads).unwrap();
    resolved.policy_keys.insert("DP-2".into(), 1);
    assert!(matches!(
        validate_desktop_output_reconciliation(&resolved, &heads),
        Err(DesktopOutputReconcileError::InvalidReconciliation(_))
    ));
    resolved.policy_keys.insert("DP-2".into(), 2);
    assert!(matches!(
        validate_desktop_output_reconciliation(&resolved, &heads),
        Err(DesktopOutputReconcileError::InvalidReconciliation(_))
    ));
}

#[test]
fn gpu_qualified_connectors_are_stable_and_bare_duplicates_refuse() {
    let a = "pci-0000:03:00.0/DP-1";
    let b = "pci-0000:16:00.0/DP-1";
    let profile = daily(DesktopOutputAvailability::Adaptive);
    let heads = vec![head(a, DAILY, 0), head(b, OFFICE, 2560)];
    assert_eq!(
        resolve_desktop_output_candidate(&profile, &topology(heads.clone()), None),
        Err(DesktopOutputReconcileError::AmbiguousConnector(
            "DP-1".into()
        ))
    );
    let qualified = parse("availability adaptive; inherit-sophia #false; named DP-1 { gpu \"pci-0000:03:00.0\"; policy-key 1; enabled #true; }; named DP-1 { gpu \"pci-0000:16:00.0\"; enabled #false; }").unwrap();
    let resolved = active(&qualified, heads, None);
    assert_eq!(enabled(&resolved), [a]);
    assert_eq!(resolved.policy_keys, [(a.into(), 1)].into());
    let single = active(&profile, vec![head(a, DAILY, 0)], None);
    assert_eq!(single.policy_keys, [(a.into(), 1)].into());
    for source in [
        "named DP-1 { gpu card0; enabled #true; }",
        "named DP-1 { gpu \"pci-0000:03:00.0\"; gpu \"pci-0000:16:00.0\"; enabled #true; }",
        "named \"pci-0000:03:00.0/DP-1\" { gpu \"pci-0000:16:00.0\"; enabled #true; }",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn reload_identity_ignores_preferences_but_keeps_exclusions_and_groups() {
    let profile = daily(DesktopOutputAvailability::Adaptive);
    let mut changed = profile.clone();
    changed.named[0].mode = Some(DesktopOutputMode::Preferred);
    changed.named[0].vrr = Some(DesktopOutputVrrMode::Disabled);
    assert!(profile.same_session_identity(&changed));
    changed.named.push(excluded("HDMI-A-2"));
    assert!(!profile.same_session_identity(&changed));
    changed = profile.clone();
    changed.named[0].mirror = vec!["DP-2".into()];
    assert!(!profile.same_session_identity(&changed));
    changed = profile.clone();
    changed.named[0].connector = "pci-0000:03:00.0/DP-1".into();
    assert!(!profile.same_session_identity(&changed));
}
