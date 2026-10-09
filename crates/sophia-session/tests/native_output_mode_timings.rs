#![cfg(feature = "native-session")]

use sophia_backend_live::{
    LibdrmNativeModeResolutionStatus, LibdrmNativeOutputCapability, LibdrmNativeOutputTiming,
    LibdrmNativeVrrPropertyDiscoveryStatus, LiveLogicalOutputAllocator,
    project_live_output_authority_candidate_snapshot, project_live_output_authority_snapshot,
    resolve_live_output_topology_candidate, resolve_native_output_mode_index,
};
use sophia_config::{
    ConfigDigest, ConfigGeneration, DesktopOutputCandidate, reconcile_desktop_output_candidate,
};
use sophia_engine::{HeadlessOutput, RenderHeadId};
use sophia_protocol::*;
use sophia_session::desktop_output_publication::prepare_output_topology_publication;
use sophia_session::desktop_output_topology::{
    prepare_native_output_activation_plan, project_native_output_topology,
};

fn modeline(clock_khz: u32) -> LibdrmNativeOutputTiming {
    LibdrmNativeOutputTiming {
        mode: Some(OutputModeTiming {
            clock_khz,
            hdisplay: 2560,
            hsync_start: 2608,
            hsync_end: 2640,
            htotal: 2720,
            hskew: 0,
            vdisplay: 1440,
            vsync_start: 1443,
            vsync_end: 1448,
            vtotal: 1481,
            flags: 0,
        }),
        ..LibdrmNativeOutputTiming::new(2560, 1440, 60_000)
    }
}

fn fixture() -> (Vec<LibdrmNativeOutputCapability>, Vec<HeadlessOutput>) {
    let modes = [modeline(241_500), modeline(241_550)];
    let capability = LibdrmNativeOutputCapability::new(
        OutputId::from_raw(1),
        94,
        "DP-1",
        modes,
        Some(modes[0]),
        modes[1],
        LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
    )
    .unwrap()
    .bind_head(RenderHeadId::from_raw(11))
    .unwrap();
    (
        vec![capability],
        vec![HeadlessOutput {
            id: OutputId::from_raw(1),
            size: Size {
                width: 2560,
                height: 1440,
            },
            scale: 1,
        }],
    )
}

#[test]
fn modelines_sharing_a_nominal_timing_project_and_reconcile_once() {
    let (capabilities, outputs) = fixture();
    let topology = project_native_output_topology(&capabilities, &outputs).unwrap();
    assert_eq!(topology.connectors[0].modes.len(), 1);
    assert_eq!(
        topology.connectors[0].current.mode,
        topology.connectors[0].modes[0]
    );
    let profile = DesktopOutputCandidate {
        generation: ConfigGeneration::INITIAL,
        digest: ConfigDigest::new([1; 32]),
        inherit_sophia: true,
        availability: sophia_config::DesktopOutputAvailability::Strict,
        fallback_policy_key: None,
        named: vec![],
    };
    let reconciled = reconcile_desktop_output_candidate(&profile, &topology).unwrap();
    prepare_native_output_activation_plan(&capabilities, &topology, &reconciled).unwrap();
    // The backend still owns both complete timings; only the profile view deduplicates.
    assert_eq!(capabilities[0].modes().len(), 2);
}

#[test]
fn nominal_requests_resolve_real_modes_and_exact_requests_preserve_identity() {
    let modes = [modeline(241_500), modeline(241_550)];
    let nominal = LibdrmNativeOutputTiming::new(2560, 1440, 60_000);
    assert_eq!(
        resolve_native_output_mode_index(&modes, nominal).index,
        Some(0)
    );
    assert_eq!(
        resolve_native_output_mode_index(&modes, modes[1]).index,
        Some(1)
    );
    assert_eq!(
        resolve_native_output_mode_index(&modes, modeline(241_600)).status,
        LibdrmNativeModeResolutionStatus::UnknownTiming
    );
}

#[test]
fn opaque_authority_modes_resolve_to_the_advertised_full_modeline() {
    let (capabilities, outputs) = fixture();
    let snapshot = project_live_output_authority_snapshot(&capabilities, &outputs, 1).unwrap();
    let head = &snapshot.heads[0];
    // The selected modeline is advertised first, even though it is second in DRM order.
    assert_eq!(head.modes.len(), 2);
    for (descriptor, expected) in head
        .modes
        .iter()
        .zip([modeline(241_550), modeline(241_500)])
    {
        let candidate = OutputTopologyCandidate {
            base_topology_epoch: snapshot.topology_epoch,
            primary_group_index: 0,
            intent: OutputTopologyIntent::Apply,
            heads: vec![OutputHeadTargetProposal {
                head: head.head,
                head_generation: head.generation,
                mode: descriptor.mode,
                transform: OutputTransform::Normal,
                vrr: OutputVrrPolicy::Disabled,
            }],
            groups: vec![OutputLogicalGroupProposal {
                output: outputs[0].id,
                logical: Rect {
                    x: 0,
                    y: 0,
                    width: 2560,
                    height: 1440,
                },
                members: vec![OutputGroupMember {
                    head: head.head,
                    mapping: OutputHeadMapping::Exact,
                }],
            }],
        };
        let mut allocator = LiveLogicalOutputAllocator::after([outputs[0].id]).unwrap();
        let resolved = resolve_live_output_topology_candidate(
            &snapshot,
            &capabilities,
            &candidate,
            &mut allocator,
        )
        .unwrap();
        assert_eq!(resolved.targets[0].timing, expected);
        let applied =
            project_live_output_authority_candidate_snapshot(&snapshot, &candidate, &resolved, 2)
                .unwrap();
        let publication = prepare_output_topology_publication(&applied, &resolved, 2).unwrap();
        assert_eq!(publication.outputs[0].timing, expected.mode);
        assert_eq!(publication.outputs[0].refresh_millihz, 60_000);
        assert_eq!(publication.generation, 2);

        // Timing is backend metadata; logical geometry and nominal rate must
        // still agree with the authority before anything reaches the frontend.
        let mut drifted = resolved.clone();
        drifted.logical_viewports[0].logical.x += 1;
        assert!(prepare_output_topology_publication(&applied, &drifted, 2).is_err());
        let mut drifted = resolved.clone();
        drifted.targets[0].timing.refresh_millihz = 120_000;
        assert!(prepare_output_topology_publication(&applied, &drifted, 2).is_err());

        let mut synthetic = resolved.clone();
        synthetic.targets[0].timing.mode = None;
        assert!(
            prepare_output_topology_publication(&applied, &synthetic, 2)
                .unwrap()
                .outputs[0]
                .timing
                .is_none()
        );
        assert_eq!(
            resolve_native_output_mode_index(capabilities[0].modes(), resolved.targets[0].timing)
                .index,
            Some(if expected == modeline(241_550) { 1 } else { 0 })
        );
    }
}
