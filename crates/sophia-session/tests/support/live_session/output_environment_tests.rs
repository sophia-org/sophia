//! Supplied readback records only: no DRM device or real GL context is used.
use super::super::{
    OutputEnvironmentBatch, environment_percent_encode, output_environment_batch,
    output_environment_lines,
};
use sophia_backend_live::{
    LiveProductionOutputEnvironmentReadback, LiveProductionOutputKmsReadback,
};
use sophia_engine::RenderHeadId;
use sophia_protocol::OutputId;
use sophia_renderer_native_egl::{NativeGlIdentity, NativeGlIdentityState};

fn record() -> LiveProductionOutputEnvironmentReadback {
    LiveProductionOutputEnvironmentReadback {
        kms: LiveProductionOutputKmsReadback {
            head: RenderHeadId::from_raw(1),
            card: 0,
            connector: 10,
            crtc: 20,
            plane: 30,
            mode: Some([
                1920, 1080, 120, 296951, 2008, 2052, 2200, 1084, 1089, 1125, 0, 0, 0,
            ]),
            properties: [("crtc.ACTIVE".into(), 1), ("crtc.VRR_ENABLED".into(), 0)].into(),
        },
        output: OutputId::from_raw(3),
        card_node: Some("card1".into()),
        connector_name: Some("DP-2".into()),
        driver: Some("amdgpu".into()),
        nominal_refresh_millihz: Some(120000),
        timing_refresh_millihz: Some(119980),
        applied_scale_milli: Some(1000),
        vrr_enabled: Some(false),
        gl_identity: NativeGlIdentityState::Observed(std::sync::Arc::new(NativeGlIdentity {
            vendor: "Mesa".into(),
            renderer: "AMD (GPU)".into(),
            version: "4.6 Mesa 25.2".into(),
        })),
    }
}

#[test]
fn environment_records_preserve_programmed_timing_and_context_strings() {
    let lines = output_environment_lines(&record(), 4100, 123456).unwrap();
    assert_eq!(
        lines[0],
        "sophia_shell_environment_output schema=1 pid=4100 output=3 card=card1 connector=DP-2 width=1920 height=1080 refresh_millihz=120000 timing_refresh_millihz=119980 scale_milli=1000 vrr=off monotonic_usec=123456"
    );
    assert_eq!(
        lines[1],
        "sophia_shell_environment_renderer schema=1 pid=4100 output=3 card=card1 driver=amdgpu vendor=Mesa renderer=AMD%20%28GPU%29 version=4.6%20Mesa%2025.2 monotonic_usec=123456"
    );
    assert_eq!(environment_percent_encode("é%\n~"), "%C3%A9%25%0A~");
}

#[test]
fn environment_records_never_replace_missing_facts_with_policy() {
    for mutation in 0..12 {
        let mut r = record();
        match mutation {
            0 => r.card_node = None,
            1 => r.connector_name = None,
            2 => r.driver = None,
            3 => r.nominal_refresh_millihz = None,
            4 => r.timing_refresh_millihz = None,
            5 => r.applied_scale_milli = None,
            6 => r.vrr_enabled = None,
            7 => r.gl_identity = Default::default(),
            8 => r.kms.mode = None,
            9 => r.connector_name = Some("DP 2".into()),
            10 => r.applied_scale_milli = Some(0),
            _ => r.gl_identity.observe(std::sync::Arc::new(NativeGlIdentity {
                vendor: "another vendor".into(),
                renderer: "GPU".into(),
                version: "4.6".into(),
            })),
        }
        assert!(
            output_environment_lines(&r, 4100, 123456).is_none(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn environment_waits_for_every_active_context_without_publishing_a_partial_set() {
    let first = record();
    let mut second = record();
    second.kms.head = RenderHeadId::from_raw(2);
    second.output = OutputId::from_raw(4);
    second.gl_identity = Default::default();
    assert!(matches!(
        output_environment_batch(&[first.clone(), second.clone()], 4100, 1),
        OutputEnvironmentBatch::Pending
    ));
    second.gl_identity = first.gl_identity.clone();
    match output_environment_batch(&[first.clone(), second.clone()], 4100, 1) {
        OutputEnvironmentBatch::Ready(lines) => assert_eq!(lines.len(), 2),
        _ => panic!("both complete contexts must publish"),
    }
    second.vrr_enabled = None;
    assert!(matches!(
        output_environment_batch(&[first, second], 4100, 1),
        OutputEnvironmentBatch::Unavailable(_)
    ));
}
