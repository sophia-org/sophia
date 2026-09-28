//! Independent C file client, production protected Session launch and CPU
//! presentation. Broker disclosure is a sanitized table fixture; this does not
//! launch a broker or claim native scanout/physical display evidence.
#![cfg(test)]
use super::super::metadata_broker::descriptor_fixture::broker_fixture;
use super::*;
use sophia_backend_live::{
    LiveProductionAuthorityBatch, LiveProductionCpuScene, LiveProductionCursorPresentation,
    LiveProductionCycleRequest, LiveProductionVisualRuntime,
};
use sophia_config::{ShellFileProfile, ShellGpuMode, ShellTransportSelection};
use sophia_engine::{DescriptorOverlayProjection, HeadlessOutput, reduce_output_work_areas};
use sophia_protocol::Rect;

use crate::live_session::c_sdk_fixture_process as process;

fn present(
    runtime: &mut LiveProductionVisualRuntime,
    scene: &mut LiveProductionCpuScene,
    output: HeadlessOutput,
) {
    let batch = LiveProductionAuthorityBatch {
        groups: vec![],
        dma_buf_registrations: vec![],
        fence_registrations: vec![],
        released_dma_bufs: vec![],
        released_fences: vec![],
    };
    let (submission, _, _) = runtime
        .run_cpu_production_cycle(LiveProductionCycleRequest {
            batch: &batch,
            scene,
            raised_surface: None,
            focused_surface: None,
            cursor_presentation: LiveProductionCursorPresentation::Software(None),
            defer_frame: false,
            output_descriptors: &[output],
            native_scanout: None,
            wm_update: None,
            presentation_layout: &[],
            geometry_routed_surfaces: &[],
            chrome_surfaces: &[],
            indicator_publication: None,
            staged_cpu_buffer_handles: &[],
        })
        .unwrap();
    assert!(submission.composed);
}

fn candidate(
    shell: &mut LiveMetadataShell,
    broker: &LiveMetadataBroker,
    output: HeadlessOutput,
    bounds: Rect,
    surface: SurfaceId,
) -> Option<DescriptorOverlayProjection> {
    shell
        .request_candidate(
            broker,
            output,
            bounds,
            bounds,
            &[(output.id, bounds)],
            &[surface].into(),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(overlay) = shell.poll_candidate(broker).unwrap() {
            return overlay;
        }
        assert!(Instant::now() < deadline, "C descriptor candidate timeout");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn assert_depth(shell: &LiveMetadataShell, output: HeadlessOutput, bounds: Rect, depth: i32) {
    let bands = shell.work_area_bands();
    let work = reduce_output_work_areas(bounds, [(output.id, bounds)], &[], &bands)[0]
        .work
        .unwrap();
    assert_eq!(
        work,
        Rect {
            y: depth,
            height: bounds.height - depth,
            ..bounds
        }
    );
    assert_eq!(bands.len(), usize::from(depth != 0));
}

#[test]
fn protected_c_descriptor_work_area_changes_only_after_matching_presentation() {
    let scratch = process::Scratch::new();
    let executable = process::compile(
        &scratch.0,
        &["nine_p", "shell_files", "shell_session"],
        "descriptor_work_area_peer.c",
    );
    let surface = SurfaceId::new(1, 1);
    let broker = broker_fixture(&scratch.0.join("broker"), surface);
    let mut shell = LiveMetadataShell::start(
        executable.to_str().unwrap(),
        ShellTransportSelection::NineP2000L,
        ShellFileProfile::Descriptor,
        Some(32),
        false,
        false,
        ShellGpuMode::Denied,
        None,
        None,
    )
    .unwrap();
    let output = HeadlessOutput::deterministic();
    let bounds = Rect {
        x: 0,
        y: 0,
        width: output.size.width,
        height: output.size.height,
    };
    shell.observe_outputs(&[output]).unwrap();
    let mut runtime = LiveProductionVisualRuntime::new(&[output], None).unwrap();
    let mut scene = LiveProductionCpuScene::new(output.size);

    let first = candidate(&mut shell, &broker, output, bounds, surface).unwrap();
    assert_eq!(first.generation, 1);
    assert_depth(&shell, output, bounds, 0);
    assert!(!shell.observe_presentation(&runtime).unwrap());
    // Present an unrelated generation/projection. Neither an arbitrary frame
    // nor simply staging the right overlay may commit the pending claim.
    let mut unrelated = first.clone();
    unrelated.generation = 99;
    for command in &mut unrelated.commands {
        if let sophia_engine::CompositorDisplayCommand::Rect(rect) = command
            && let sophia_engine::CompositorNodeId::DescriptorOverlay { projection, .. } =
                &mut rect.node
        {
            *projection = 99;
        }
    }
    runtime
        .set_descriptor_overlay(Some(unrelated), &scene, None)
        .unwrap();
    present(&mut runtime, &mut scene, output);
    assert!(!shell.observe_presentation(&runtime).unwrap());
    assert_depth(&shell, output, bounds, 0);
    runtime
        .set_descriptor_overlay(Some(first), &scene, None)
        .unwrap();
    assert!(!shell.observe_presentation(&runtime).unwrap());
    assert_depth(&shell, output, bounds, 0);
    present(&mut runtime, &mut scene, output);
    assert!(shell.observe_presentation(&runtime).unwrap());
    assert_depth(&shell, output, bounds, 24);
    assert!(!shell.observe_presentation(&runtime).unwrap());

    // Rejecting a prepared replacement keeps the last presented reservation.
    let rejected = candidate(&mut shell, &broker, output, bounds, surface).unwrap();
    assert_eq!(rejected.generation, 2);
    assert!(!shell.observe_presentation(&runtime).unwrap());
    assert_depth(&shell, output, bounds, 24);
    assert!(shell.reject_pending().unwrap());
    assert_depth(&shell, output, bounds, 24);

    let replacement = candidate(&mut shell, &broker, output, bounds, surface).unwrap();
    assert_eq!(replacement.generation, 3);
    runtime
        .set_descriptor_overlay(Some(replacement), &scene, None)
        .unwrap();
    assert!(!shell.observe_presentation(&runtime).unwrap());
    assert_depth(&shell, output, bounds, 24);
    present(&mut runtime, &mut scene, output);
    assert!(shell.observe_presentation(&runtime).unwrap());
    assert_depth(&shell, output, bounds, 32);

    assert!(candidate(&mut shell, &broker, output, bounds, surface).is_none());
    assert!(!shell.observe_presentation(&runtime).unwrap());
    assert_depth(&shell, output, bounds, 32);
    runtime.set_descriptor_overlay(None, &scene, None).unwrap();
    assert!(!shell.observe_presentation(&runtime).unwrap());
    assert_depth(&shell, output, bounds, 32);
    present(&mut runtime, &mut scene, output);
    assert!(shell.observe_presentation(&runtime).unwrap());
    assert_depth(&shell, output, bounds, 0);

    // The C peer sends this receipt candidate only after it independently
    // decodes all four ordered Prepared + Presented/Rejected pairs, including
    // strictly increasing nonzero presentation epochs. No Rust codec in peer.
    assert!(candidate(&mut shell, &broker, output, bounds, surface).is_none());
    assert!(shell.reject_pending().unwrap());
    assert_depth(&shell, output, bounds, 0);
    // Flush the final outcomes and observe normal child exit without reaping
    // the supervisor's child ourselves. Drop remains responsible on failure.
    let pid = rustix::process::Pid::from_raw(shell.supervisor.child_id().unwrap() as i32).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let status = rustix::process::waitid(
            rustix::process::WaitId::Pid(pid),
            rustix::process::WaitIdOptions::EXITED
                | rustix::process::WaitIdOptions::NOHANG
                | rustix::process::WaitIdOptions::NOWAIT,
        )
        .unwrap();
        if let Some(status) = status {
            assert_eq!(
                status.exit_status(),
                Some(0),
                "protected C peer failed: {status:?}"
            );
            break;
        }
        match shell.transport.poll_io() {
            Ok(()) | Err(sophia_runtime::ShellTransportError::NotConnected) => {}
            Err(error) => panic!("final descriptor outcomes: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "C peer did not finish its outcome checks"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        shell.supervisor.poll().unwrap(),
        Some(sophia_runtime::SupervisorEvent::ProcessExited)
    );
}
