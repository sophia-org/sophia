#![cfg(test)]

use super::*;

#[test]
fn production_lom_proof_admits_the_discrete_input_contract() {
    assert!(matches!(
        proof_content_admission_policy(),
        ShellContentAdmissionPolicy::Granted {
            discrete_input: true
        }
    ));
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
