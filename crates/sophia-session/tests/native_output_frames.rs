#![cfg(feature = "native-session")]

use sophia_backend_live::{
    LibdrmNativeOutputCapability, LibdrmNativeOutputTiming, LibdrmNativeVrrPropertyDiscoveryStatus,
    LiveGbmEglFrameTargetRecord, LiveGbmEglFrameTargetStatus,
};
use sophia_config::{
    ConfigDigest, ConfigGeneration, DesktopNamedOutputCandidate, DesktopOutputCandidate,
    reconcile_desktop_output_candidate,
};
use sophia_engine::HeadlessOutput;
use sophia_protocol::{OutputId, Size};
use sophia_session::desktop_output_frames::{
    NativeOutputApplyAdmission, NativeOutputFrameTarget, native_output_apply_admission,
};
use sophia_session::desktop_output_topology::{
    NativeOutputActivationPlan, prepare_native_output_activation_plan,
    project_native_output_topology,
};

const MODE: (u32, u32) = (1920, 1080);

fn timing() -> LibdrmNativeOutputTiming {
    LibdrmNativeOutputTiming::new(MODE.0, MODE.1, 60_000)
}

fn capability(output: u64) -> LibdrmNativeOutputCapability {
    LibdrmNativeOutputCapability::new(
        OutputId::from_raw(output),
        u32::try_from(output).expect("test output ids are small"),
        format!("DP-{output}"),
        [timing()],
        Some(timing()),
        timing(),
        LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
    )
    .expect("capability fixture is valid")
}

fn headless(output: u64) -> HeadlessOutput {
    HeadlessOutput {
        id: OutputId::from_raw(output),
        size: Size {
            width: MODE.0 as i32,
            height: MODE.1 as i32,
        },
        scale: 1,
    }
}

fn plan(outputs: &[u64], disabled: &[u64]) -> NativeOutputActivationPlan {
    let capabilities = outputs.iter().copied().map(capability).collect::<Vec<_>>();
    let headless = outputs.iter().copied().map(headless).collect::<Vec<_>>();
    let topology =
        project_native_output_topology(&capabilities, &headless).expect("topology projects");
    let candidate = DesktopOutputCandidate {
        generation: ConfigGeneration::from_raw(3),
        digest: ConfigDigest::new([4; 32]),
        inherit_sophia: true,
        named: disabled
            .iter()
            .map(|output| DesktopNamedOutputCandidate {
                policy_key: None,
                mirror_fit: None,
                connector: format!("DP-{output}"),
                mode: None,
                scale: None,
                position: None,
                transform: None,
                enabled: Some(false),
                focus_at_startup: None,
                vrr: None,
                mirror: Vec::new(),
            })
            .collect(),
    };
    let reconciliation =
        reconcile_desktop_output_candidate(&candidate, &topology).expect("candidate reconciles");
    prepare_native_output_activation_plan(&capabilities, &topology, &reconciliation)
        .expect("plan prepares")
}

fn frame(output: u64, width: i32, height: i32) -> NativeOutputFrameTarget {
    NativeOutputFrameTarget {
        connector: format!("DP-{output}"),
        output: OutputId::from_raw(output),
        target: LiveGbmEglFrameTargetRecord::new(Size { width, height }),
    }
}

#[test]
fn a_frame_at_the_requested_mode_admits_apply() {
    let admission = native_output_apply_admission(
        &plan(&[1, 2], &[]),
        &[
            frame(1, MODE.0 as i32, MODE.1 as i32),
            frame(2, MODE.0 as i32, MODE.1 as i32),
        ],
    );

    assert_eq!(admission, NativeOutputApplyAdmission::Ready);
    assert!(admission.is_ready());
}

#[test]
fn an_output_with_no_composed_frame_is_not_ready() {
    // The state before anything has been rendered. Apply must not run here: there
    // is no committed frame to scan out, and inventing one is the speculative
    // bootstrap the renderer boundary forbids.
    assert_eq!(
        native_output_apply_admission(
            &plan(&[1, 2], &[]),
            &[frame(1, MODE.0 as i32, MODE.1 as i32)]
        ),
        NativeOutputApplyAdmission::NoFrameTarget { output: 2 }
    );
}

#[test]
fn a_frame_sized_for_the_old_mode_is_not_ready() {
    // The ordinary state partway through a mode change: the target still holds the
    // previous mode's size because it has not been resized and recomposed. Naming
    // this buffer in the commit is what the kernel would refuse with EINVAL, so it
    // is refused here with both sizes instead.
    assert_eq!(
        native_output_apply_admission(&plan(&[1], &[]), &[frame(1, 2560, 1440)]),
        NativeOutputApplyAdmission::SizeMismatch {
            output: 1,
            target_width: 2560,
            target_height: 1440,
            requested_width: MODE.0,
            requested_height: MODE.1,
        }
    );
}

#[test]
fn an_unusable_frame_target_is_not_ready() {
    let unusable = NativeOutputFrameTarget {
        connector: "DP-1".to_owned(),
        output: OutputId::from_raw(1),
        target: LiveGbmEglFrameTargetRecord::new(Size {
            width: 0,
            height: 0,
        }),
    };
    assert_eq!(
        unusable.target.status,
        LiveGbmEglFrameTargetStatus::InvalidSize
    );

    assert_eq!(
        native_output_apply_admission(&plan(&[1], &[]), &[unusable]),
        NativeOutputApplyAdmission::InvalidFrameTarget { output: 1 }
    );
}

#[test]
fn a_disabled_output_is_not_asked_for_a_frame() {
    // Disablement is expressed by omission, so a leaving output owes no frame.
    // Requiring one would block every apply that turns a screen off.
    assert_eq!(
        native_output_apply_admission(
            &plan(&[1, 2], &[2]),
            &[frame(1, MODE.0 as i32, MODE.1 as i32)]
        ),
        NativeOutputApplyAdmission::Ready
    );
}

#[test]
fn the_first_failing_output_is_the_reported_one() {
    // One precise cause beats a list: a mode change resizes targets one at a time,
    // so the rest of a list would restate the same transition.
    assert_eq!(
        native_output_apply_admission(&plan(&[1, 2], &[]), &[]),
        NativeOutputApplyAdmission::NoFrameTarget { output: 1 }
    );
}
