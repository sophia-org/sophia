#![cfg(test)]

use super::*;
use sophia_config::{ConfigDigest, ConfigGeneration, DesktopOutputAvailability};

fn profile() -> DesktopOutputCandidate {
    DesktopOutputCandidate {
        generation: ConfigGeneration::INITIAL,
        digest: ConfigDigest::new([7; 32]),
        inherit_sophia: false,
        availability: DesktopOutputAvailability::Adaptive,
        fallback_policy_key: Some(1),
        named: Vec::new(),
    }
}

fn probe(name: &str) -> LiveNativeOutputProbe {
    LiveNativeOutputProbe {
        connector: name.into(),
        gpu_identity: Some("pci-0000:03:00.0".into()),
        connector_id: 1,
        connected: true,
        usable: true,
        vrr_capable: false,
        modes: vec![LibdrmNativeOutputTiming::new(1920, 1080, 60_000)],
        preferred_mode: None,
    }
}

fn resolve(
    probes: &[LiveNativeOutputProbe],
    profile: &DesktopOutputCandidate,
) -> DesktopOutputResolution {
    sophia_config::resolve_desktop_output_candidate(
        profile,
        &project_profile_probes(probes, profile, None),
        None,
    )
    .unwrap()
}

#[test]
fn unusable_atomic_heads_cannot_become_fallbacks() {
    let mut unusable = probe("pci-0000:03:00.0/DP-1");
    unusable.usable = false;
    assert!(matches!(
        resolve(&[unusable.clone()], &profile()),
        DesktopOutputResolution::Waiting { .. }
    ));
    let usable = probe("pci-0000:03:00.0/DP-2");
    let probes = [unusable, usable];
    let DesktopOutputResolution::Active(realized) = resolve(&probes, &profile()) else {
        panic!()
    };
    let requests = resolved_requests(&probes, &realized).unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].connector, "pci-0000:03:00.0/DP-2");
    assert_eq!(
        realized.policy_keys,
        [(requests[0].connector.clone(), 1)].into()
    );
}

#[test]
fn disconnected_sockets_do_not_break_an_otherwise_strict_native_profile() {
    let mut profile = profile();
    profile.availability = DesktopOutputAvailability::Strict;
    profile.fallback_policy_key = None;
    profile.inherit_sophia = true;
    let mut absent = probe("pci-0000:03:00.0/HDMI-A-1");
    absent.connected = false;
    absent.usable = false;
    absent.modes.clear();
    let DesktopOutputResolution::Active(realized) =
        resolve(&[absent, probe("pci-0000:03:00.0/DP-1")], &profile)
    else {
        panic!()
    };
    assert_eq!(realized.outputs.len(), 1);
    assert!(realized.outputs[0].enabled);
}

#[test]
fn native_requests_preserve_the_advertised_full_timing_and_ignore_enumeration_order() {
    let mut first = probe("pci-0000:03:00.0/DP-1");
    let mut measured = first.modes[0];
    measured.mode = Some(sophia_protocol::OutputModeTiming {
        clock_khz: 148_500,
        hdisplay: 1920,
        hsync_start: 2008,
        hsync_end: 2052,
        htotal: 2200,
        hskew: 0,
        vdisplay: 1080,
        vsync_start: 1084,
        vsync_end: 1089,
        vtotal: 1125,
        flags: 5,
    });
    let mut alternate = measured;
    alternate.mode.as_mut().unwrap().flags = 6;
    first.modes = vec![alternate, measured];
    first.preferred_mode = Some(alternate);
    let DesktopOutputResolution::Active(realized) = resolve(&[first.clone()], &profile()) else {
        panic!()
    };
    assert_eq!(
        resolved_requests(&[first.clone()], &realized).unwrap()[0].mode,
        alternate
    );
    first.preferred_mode = None;
    assert_eq!(
        resolved_requests(&[first.clone()], &realized).unwrap()[0].mode,
        measured
    );
    first.modes.reverse();
    assert_eq!(
        resolved_requests(&[first.clone()], &realized).unwrap()[0].mode,
        measured
    );
    first.modes.clear();
    assert!(resolved_requests(&[first], &realized).is_err());
}

#[test]
fn lost_capabilities_never_survive_in_the_projected_current_state() {
    let original = probe("pci-0000:03:00.0/DP-1");
    let DesktopOutputResolution::Active(mut old) =
        resolve(std::slice::from_ref(&original), &profile())
    else {
        panic!()
    };
    old.outputs[0].scale_milli = 1250;
    old.outputs[0].vrr = DesktopOutputVrrMode::Always;
    old.outputs[0].transform = DesktopOutputTransform::Rotate90;
    old.outputs[0].mode.width = 2560;
    let topology = project_probes(&[original], Some(&old));
    let current = &topology.connectors[0].current;
    assert_eq!(current.mode.width, 1920);
    assert_eq!(current.scale_milli, 1000);
    assert_eq!(current.vrr, DesktopOutputVrrMode::Disabled);
    assert_eq!(current.transform, DesktopOutputTransform::Normal);
    assert!(
        sophia_config::resolve_desktop_output_candidate(&profile(), &topology, Some(&old)).is_ok()
    );
}
