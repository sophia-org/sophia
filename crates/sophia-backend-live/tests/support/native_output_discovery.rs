#![cfg(test)]

use super::*;

fn probe(name: &str) -> LiveNativeOutputProbe {
    LiveNativeOutputProbe {
        connector: name.into(),
        gpu_identity: None,
        connector_id: 1,
        connected: true,
        modes: vec![LibdrmNativeOutputTiming::new(1920, 1080, 60_000)],
        preferred_mode: Some(LibdrmNativeOutputTiming::new(1920, 1080, 60_000)),
        usable: true,
        vrr_capable: true,
    }
}

fn request(name: &str) -> LiveNativeOutputRequest {
    LiveNativeOutputRequest {
        connector: name.into(),
        mode: LibdrmNativeOutputTiming::new(1920, 1080, 60_000),
        scale: 1,
        vrr: sophia_protocol::OutputVrrPolicy::Disabled,
        mirror_of: None,
    }
}

#[test]
fn resolved_requests_cannot_add_devices_or_exceed_probed_capabilities() {
    let probes = vec![probe("DP-1")];
    assert!(validate_requests(&probes, vec![request("DP-1")]).is_ok());
    for requests in [
        vec![],
        vec![request("DP-2")],
        vec![request("DP-1"), request("DP-1")],
    ] {
        assert!(validate_requests(&probes, requests).is_err());
    }
    for variant in 0..4 {
        let mut probes = probes.clone();
        let mut requested = request("DP-1");
        match variant {
            0 => probes[0].connected = false,
            1 => probes[0].usable = false,
            2 => requested.mode.width = 1234,
            _ => requested.scale = 0,
        }
        assert!(validate_requests(&probes, vec![requested]).is_err());
    }
}

#[test]
fn resolved_mirrors_require_one_enabled_primary_and_consistent_settings() {
    let probes = vec![probe("DP-1"), probe("DP-2")];
    let primary = request("DP-1");
    let mut member = request("DP-2");
    member.mirror_of = Some("DP-1".into());
    assert!(validate_requests(&probes, vec![primary.clone(), member.clone()]).is_ok());
    assert!(validate_requests(&probes, vec![member.clone()]).is_err());
    member.scale = 2;
    assert!(validate_requests(&probes, vec![primary.clone(), member.clone()]).is_err());
    member.scale = 1;
    let mut cycle = primary;
    cycle.mirror_of = Some("DP-2".into());
    assert!(validate_requests(&probes, vec![cycle, member]).is_err());
}
