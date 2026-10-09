#![cfg(test)]

use super::super::tests::{probe, profile};
use super::*;

fn resolve(
    probes: &[LiveNativeOutputProbe],
    profile: &DesktopOutputCandidate,
) -> DesktopOutputReconciliation {
    let DesktopOutputResolution::Active(realized) =
        resolve_probe_policy(probes, profile, None).unwrap()
    else {
        panic!()
    };
    realized
}

#[test]
fn failed_test_then_failed_apply_spends_the_only_recovery_allowance() {
    let profile = profile();
    let mut attempt = OutputRecovery::default();
    // A failed TEST consumes Desired even though nothing was applied. A later
    // apply failure of that conservative owner must go directly to Waiting.
    assert!(attempt.refused(&profile));
    assert_eq!(attempt, OutputRecovery::Conservative);
    assert!(!attempt.refused(&profile));
    for _ in 0..100 {
        assert_eq!(attempt, OutputRecovery::Exhausted);
        assert!(!attempt.refused(&profile));
    }
    let mut strict = profile.clone();
    strict.availability = sophia_config::DesktopOutputAvailability::Strict;
    let mut attempt = OutputRecovery::default();
    assert!(!attempt.refused(&strict));
    assert_eq!(attempt, OutputRecovery::Exhausted);
}

#[test]
fn recovery_uses_advertised_settings_and_preserves_desired_preferences_and_affinity() {
    let mut a = probe("DP-1");
    a.vrr_capable = true;
    a.modes
        .push(LibdrmNativeOutputTiming::new(2560, 1440, 120_000));
    a.preferred_mode = Some(a.modes[1]);
    let mut b = a.clone();
    b.connector = "DP-2".into();
    let probes = [a, b];
    let mut profile = profile();
    profile.inherit_sophia = true;
    let mut desired = resolve(&probes, &profile);
    desired.focused_connector = Some("DP-2".into());
    desired.policy_keys = [("DP-1".into(), 1), ("DP-2".into(), 2)].into();
    desired.outputs[1].scale_milli = 2000;
    desired.outputs[1].vrr = DesktopOutputVrrMode::Always;
    let saved = desired.clone();
    let recovered = conservative_realization(&probes, &profile, desired.clone()).unwrap();
    assert_eq!(desired, saved);
    assert_eq!(recovered.generation, desired.generation);
    assert_eq!(recovered.digest, desired.digest);
    assert!(!recovered.outputs[0].enabled);
    let active = &recovered.outputs[1];
    assert!(active.enabled);
    assert_eq!(active.mode, DesktopOutputTiming::new(1920, 1080, 60_000));
    assert_eq!(active.scale_milli, 1000);
    assert_eq!(active.position, (0, 0));
    assert_eq!(active.vrr, DesktopOutputVrrMode::Disabled);
    assert_eq!(recovered.focused_connector.as_deref(), Some("DP-2"));
    assert_eq!(recovered.policy_keys, [("DP-2".into(), 2)].into());
    let requests = resolved_requests(&probes, &recovered).unwrap();
    assert_eq!(requests.len(), 1);
    assert!(probes[1].modes.contains(&requests[0].mode));
    // A fresh event resolves saved preferences again, not the recovery choice.
    let restored = resolve(&probes, &profile);
    assert_eq!(
        restored
            .outputs
            .iter()
            .filter(|state| state.enabled)
            .count(),
        2
    );
    assert_eq!(restored.outputs[1].mode.refresh_millihz, 120_000);
}

#[test]
fn recovery_keeps_mirror_groups_whole_and_does_not_reclaim_exclusions() {
    let probes = [probe("DP-1"), probe("DP-2"), probe("HDMI-A-1")];
    let mut profile = profile();
    profile.inherit_sophia = true;
    let mut desired = resolve(&probes, &profile);
    desired.outputs[1].mirror_of = Some("DP-1".into());
    desired.outputs[1].position = desired.outputs[0].position;
    desired.outputs[2].enabled = false;
    desired.focused_connector = Some("DP-2".into());
    let recovered = conservative_realization(&probes, &profile, desired).unwrap();
    assert!(recovered.outputs[0].enabled && recovered.outputs[1].enabled);
    assert!(!recovered.outputs[2].enabled);
    assert_eq!(recovered.outputs[1].mirror_of.as_deref(), Some("DP-1"));
    assert_eq!(recovered.focused_connector.as_deref(), Some("DP-1"));
    let mut lost = probes.clone();
    lost[1].connected = false;
    assert!(conservative_realization(&lost, &profile, recovered).is_err());
}

#[test]
fn recovery_never_invents_a_sixty_hertz_mode_and_keeps_fallback_key_unique() {
    let mut head = probe("DP-2");
    head.modes = vec![
        LibdrmNativeOutputTiming::new(800, 600, 75_000),
        LibdrmNativeOutputTiming::new(640, 480, 50_000),
    ];
    head.preferred_mode = Some(head.modes[0]);
    let profile = profile();
    let desired = resolve(std::slice::from_ref(&head), &profile);
    let recovered =
        conservative_realization(std::slice::from_ref(&head), &profile, desired.clone()).unwrap();
    assert_eq!(recovered.outputs[0].mode.refresh_millihz, 50_000);
    assert_eq!(recovered.fallback_connector, desired.fallback_connector);
    assert_eq!(recovered.policy_keys, desired.policy_keys);
    head.modes.reverse();
    assert_eq!(
        conservative_realization(&[head], &profile, desired).unwrap(),
        recovered
    );
}
