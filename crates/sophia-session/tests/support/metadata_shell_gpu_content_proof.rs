#![cfg(test)]

use super::*;
use sophia_config::ShellComponentEdge;
use std::cell::Cell;

fn proof() -> ShellGpuContentProof {
    ShellGpuContentProof {
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
