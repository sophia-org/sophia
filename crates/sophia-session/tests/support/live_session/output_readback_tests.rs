use super::super::{OutputOwnerHeadReadback, OutputOwnerReadback, OutputReadbackCandidate};
use sophia_backend_live::LiveProductionOutputKmsReadback;
use sophia_engine::{HeadlessOutput, RenderHeadId};
use sophia_protocol::{OutputHeadMapping, OutputTransform, OutputVrrPolicy, TransactionId};

fn before() -> OutputReadbackCandidate {
    let output = HeadlessOutput::deterministic();
    let head = RenderHeadId::from_raw(1);
    OutputReadbackCandidate {
        connection_epoch: 3,
        base_topology_epoch: 7,
        transaction: TransactionId::from_raw(11),
        kms: vec![LiveProductionOutputKmsReadback {
            head,
            card: 0,
            connector: 10,
            crtc: 20,
            plane: 30,
            mode: Some([800, 600, 60, 40_000, 840, 968, 1056, 601, 605, 628, 0, 0, 0]),
            properties: [("crtc.ACTIVE".into(), 1)].into(),
        }],
        owner: OutputOwnerReadback {
            heads: vec![OutputOwnerHeadReadback {
                head,
                enabled: true,
                output,
                scale: 1,
                refresh_millihz: 60_000,
                transform: OutputTransform::Normal,
                mapping: OutputHeadMapping::Fit,
                vrr: OutputVrrPolicy::Disabled,
            }],
            outputs: vec![output],
        },
    }
}

#[test]
fn output_readback_requires_changed_timing_and_rejects_uncovered_selection_changes() {
    let before = before();
    assert!(before.check("applied", &before.kms, &before.owner).is_err());
    let mut applied = before.kms.clone();
    applied[0].mode.as_mut().unwrap()[2] = 75;
    assert!(before.check("applied", &applied, &before.owner).is_ok());
    assert!(before.check("installed", &applied, &before.owner).is_ok());
    applied[0].plane += 1;
    assert!(before.check("installed", &applied, &before.owner).is_err());
    let mut disabled = before.owner.clone();
    disabled.heads[0].enabled = false;
    assert!(before.check("installed", &before.kms, &disabled).is_err());
}

#[test]
fn output_readback_restoration_requires_both_kernel_and_software_state() {
    let before = before();
    assert!(before.check("restored", &before.kms, &before.owner).is_ok());
    let mut actual = before.kms.clone();
    actual[0].properties.insert("crtc.ACTIVE".into(), 0);
    assert!(before.check("restored", &actual, &before.owner).is_err());
    for mutation in 0..5 {
        let mut owner = before.owner.clone();
        match mutation {
            0 => owner.heads[0].transform = OutputTransform::Rotate90,
            1 => owner.heads[0].mapping = OutputHeadMapping::Cover,
            2 => owner.heads[0].vrr = OutputVrrPolicy::Automatic,
            3 => owner.heads[0].scale = 2,
            _ => owner.outputs[0].size.width += 1,
        }
        assert!(
            before.check("restored", &before.kms, &owner).is_err(),
            "mutation {mutation}"
        );
    }
}
