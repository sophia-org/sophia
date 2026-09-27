#![cfg(test)]

use super::*;
use sophia_config::ShellComponentEdge;
use std::cell::Cell;

fn proof() -> ShellGpuContentProof {
    ShellGpuContentProof {
        transport: sophia_config::ShellTransportSelection::CurrentIpc,
        client: "/absent/shell-client".into(),
        client_args: Vec::new(),
        config: None,
        seat: "seat0".into(),
        render_node: "/absent/renderD128".into(),
        expected_device: None,
        output: ShellGpuProofExtent {
            width: 800,
            height: 600,
        },
        surface: ShellGpuProofSurface {
            edge: ShellComponentEdge::Left,
            width: 48,
            height: 600,
        },
        outcomes: vec![ShellGpuProofOutcome::PresentedSynthetic; 3],
        end: ShellGpuProofEnd::StopClient,
        pixels: ShellGpuProofPixels::FullSurfaceRaster,
        discrete_input: false,
        timeout: SHELL_GPU_PROOF_DEFAULT_TIMEOUT,
    }
}

#[test]
fn admission_policy_follows_the_discrete_input_parameter() {
    for discrete_input in [false, true] {
        assert!(matches!(
            proof_content_admission_policy(discrete_input),
            ShellContentAdmissionPolicy::Granted { discrete_input: granted }
                if granted == discrete_input
        ));
    }
}

#[test]
fn pinned_device_expectation_refuses_before_the_protected_launch() {
    let device = super::super::gpu::ShellGpuLaunchEvidence {
        epoch: 1,
        major: 226,
        minor: 128,
        render_node: "/dev/dri/renderD128".into(),
        pci_bus_id: Some("0000:03:00.0".into()),
        pci_vendor_id: Some(0x1002),
        pci_device_id: Some(0x744c),
    };
    require_expected_device(&device, Some("226:128@0000:03:00.0")).unwrap();
    for wrong in [
        "226:129@0000:03:00.0",
        "226:128@0000:04:00.0",
        "invalid",
        "",
    ] {
        assert!(require_expected_device(&device, Some(wrong)).is_err());
    }
}

/// Invalid parameters are refused before the render inventory is read, so no
/// device is enumerated, opened or granted.
#[test]
fn invalid_parameters_never_reach_the_render_inventory() {
    let mut invalid = Vec::new();
    let mut zero = proof();
    zero.surface.width = 0;
    invalid.push(zero);
    let mut outside = proof();
    outside.surface.height = 601;
    invalid.push(outside);
    let mut none = proof();
    none.outcomes.clear();
    invalid.push(none);
    let mut relative = proof();
    relative.render_node = "renderD128".into();
    invalid.push(relative);
    let mut slow = proof();
    slow.timeout = SHELL_GPU_PROOF_MAX_TIMEOUT + Duration::from_millis(1);
    invalid.push(slow);
    for parameters in invalid {
        let consulted = Cell::new(false);
        let error = run_with_inventory(&parameters, |_| {
            consulted.set(true);
            Err("inventory spy reached".into())
        })
        .unwrap_err();
        assert!(!consulted.get(), "inventory consulted for {parameters:?}");
        assert!(
            error.downcast_ref::<ShellGpuProofError>().is_some(),
            "not a parameter refusal: {error}"
        );
    }
}

/// Valid parameters pass validation; this client path is absent, so the file
/// check refuses next -- still before the inventory.
#[test]
fn valid_parameters_still_check_the_client_file_before_the_inventory() {
    let consulted = Cell::new(false);
    let error = run_with_inventory(&proof(), |_| {
        consulted.set(true);
        Err("inventory spy reached".into())
    })
    .unwrap_err();
    assert!(!consulted.get());
    assert_eq!(error.to_string(), "shell client must be an absolute file");
}

#[test]
fn inventory_is_read_only_after_every_local_check_passes() {
    let client = std::env::current_exe().unwrap();
    let mut parameters = proof();
    parameters.client = client;
    let consulted = Cell::new(false);
    let error = run_with_inventory(&parameters, |seat| {
        assert_eq!(seat, "seat0");
        consulted.set(true);
        Ok(Vec::new())
    })
    .unwrap_err();
    assert!(consulted.get());
    assert_eq!(
        error.to_string(),
        "proof requires exactly one admitted render-node identity"
    );
}

/// A fragmented candidate -- Begin, several chunks, End -- that arrives one
/// record per visit must be serviced through its End. A cap on intake across
/// visits (formerly three records per render) strands it after the third
/// record and the proof times out; this control fails under that cap.
#[test]
fn a_fragmented_candidate_across_visits_is_serviced_to_its_end() {
    let records = ["begin", "chunk", "chunk", "chunk", "chunk", "end"];
    let mut arriving = records.iter();
    let mut serviced = Vec::new();
    let mut intake = CandidateIntake::default();
    for _visit in 0..records.len() {
        intake
            .visit(1, || {
                Ok::<_, ()>(match arriving.next() {
                    Some(record) => {
                        serviced.push(*record);
                        1
                    }
                    None => 0,
                })
            })
            .unwrap();
    }
    assert_eq!(serviced, records);
    assert_eq!(intake.serviced, records.len());
}

#[test]
fn intake_waits_for_a_permit_and_propagates_owner_errors() {
    let mut intake = CandidateIntake::default();
    let mut called = false;
    intake
        .visit(0, || {
            called = true;
            Ok::<_, ()>(1)
        })
        .unwrap();
    assert!(!called, "a candidate cannot begin before a frame permit");
    assert_eq!(intake.visit(1, || Err::<usize, _>("owner")), Err("owner"));
    assert_eq!(intake.serviced, 0);
}

#[test]
fn full_surface_raster_requires_exact_size_and_varied_bytes() {
    let surface = ShellGpuProofSurface {
        edge: ShellComponentEdge::Top,
        width: 4,
        height: 2,
    };
    let varied = (0..32_u8).collect::<Vec<_>>();
    check_full_surface_raster(surface, 4, 2, &varied).unwrap();
    for (width, height, bytes) in [
        (3, 2, varied.clone()),
        (4, 1, varied.clone()),
        (4, 2, varied[..28].to_vec()),
        (4, 2, vec![0; 32]),
        (4, 2, [1, 2, 3, 4].repeat(8)),
        (4, 2, Vec::new()),
    ] {
        assert!(
            check_full_surface_raster(surface, width, height, &bytes).is_err(),
            "{width}x{height} with {} bytes",
            bytes.len()
        );
    }
}
