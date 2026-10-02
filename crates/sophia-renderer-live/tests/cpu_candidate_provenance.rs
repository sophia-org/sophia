use sophia_engine::{
    CompositorDisplayCommand, CompositorDisplayList, HeadlessOutput, SurfaceDamageHistory,
    SurfaceDamageIdentity, output_frame_damage,
};
use sophia_protocol::{
    BufferSource, CommittedSurfaceState, OutputId, Rect, Region, Size, SurfaceContentSet, SurfaceId,
};
use sophia_renderer_live::{
    LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888, LiveCpuBufferSource, LiveCpuBufferUpdate,
    LiveProductionCpuScene,
};
use std::sync::Arc;

fn fixture() -> (HeadlessOutput, CommittedSurfaceState, CompositorDisplayList) {
    let size = Size {
        width: 4,
        height: 1,
    };
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        size,
        scale: 1,
    };
    let surface = SurfaceId::new(1, 1);
    let state = CommittedSurfaceState {
        surface,
        committed_generation: 2,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 1,
        },
        content: SurfaceContentSet::singleton(BufferSource::CpuBuffer { handle: 7 }, size),
        damage: Region::empty(),
    };
    let list = CompositorDisplayList {
        output: output.id,
        commands: vec![CompositorDisplayCommand::Surface { surface }],
    };
    (output, state, list)
}

fn upload(scene: &mut LiveProductionCpuScene, generation: u64, value: u8, production: bool) {
    let updates = [LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
        handle: 7,
        size: Size {
            width: 4,
            height: 1,
        },
        stride: 16,
        format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        generation,
        bytes: Arc::new(vec![value; 16]),
    })];
    if production {
        scene.apply_production_updates(updates).unwrap();
    } else {
        scene.apply_updates(updates).unwrap();
    }
}

#[test]
fn cpu_repaint_carries_candidate_identity_into_its_native_frame() {
    let (output, candidate, list) = fixture();
    let mut committed = candidate.clone();
    committed.committed_generation = 1;
    let mut scene = LiveProductionCpuScene::new(output.size);
    let mut history = SurfaceDamageHistory::default();
    upload(&mut scene, 10, 0x22, true);
    let pending = history
        .for_candidate(
            std::slice::from_ref(&committed),
            std::slice::from_ref(&candidate),
            Some(&SurfaceDamageIdentity::default()),
        )
        .unwrap();
    scene
        .compose_display_list_with_damage_history(
            output,
            std::slice::from_ref(&candidate),
            &list,
            None,
            pending,
        )
        .unwrap();
    let rejected = scene.frames_for_outputs(&[output]).unwrap().remove(0);

    // A different accepted preparation reuses the rejected candidate's public
    // generation and buffer handle. Its upload generation is independent.
    history.record_committed(
        &[committed],
        std::slice::from_ref(&candidate),
        &SurfaceDamageIdentity::default(),
    );
    upload(&mut scene, 11, 0x44, true);
    scene
        .compose_display_list_with_damage_history(
            output,
            std::slice::from_ref(&candidate),
            &list,
            None,
            history
                .for_candidate(
                    std::slice::from_ref(&candidate),
                    std::slice::from_ref(&candidate),
                    None,
                )
                .unwrap(),
        )
        .unwrap();
    let accepted = scene.frames_for_outputs(&[output]).unwrap().remove(0);
    let before = rejected.output_damage_snapshot.as_ref().unwrap();
    let after = accepted.output_damage_snapshot.as_ref().unwrap();
    assert_eq!(
        before.surfaces, after.surfaces,
        "the public facts really alias"
    );
    assert!(
        !output_frame_damage(Some(before), after)
            .unwrap()
            .rects
            .is_empty(),
        "candidate provenance must reach native frames"
    );
    assert_eq!(&rejected.frame.bytes[..], &[0x22; 16]);
    assert_eq!(&accepted.frame.bytes[..], &[0x44; 16]);
    // An unchanged accepted view still reuses the raster.
    scene
        .compose_display_list_with_damage_history(
            output,
            std::slice::from_ref(&candidate),
            &list,
            None,
            after.damage_history.clone(),
        )
        .unwrap();
    let same = scene.frames_for_outputs(&[output]).unwrap().remove(0);
    assert!(Arc::ptr_eq(&accepted.frame.bytes, &same.frame.bytes));
}

#[test]
fn repeated_upload_generation_and_evicted_handle_cannot_reuse_stale_cpu_pixels() {
    let (output, state, list) = fixture();
    for production in [false, true] {
        let mut scene = LiveProductionCpuScene::new(output.size);
        upload(&mut scene, 10, 0x22, production);
        let first = scene
            .compose_display_list(output, std::slice::from_ref(&state), &list, None)
            .unwrap()
            .frame
            .bytes
            .clone();
        upload(&mut scene, 10, 0x44, production);
        let second = scene
            .compose_display_list(output, std::slice::from_ref(&state), &list, None)
            .unwrap()
            .frame
            .bytes
            .clone();
        assert_eq!(
            &second[..],
            &[0x44; 16],
            "equal-generation upload is a new raster input"
        );
        scene.reconcile_buffer_residency(&[]);
        upload(&mut scene, 10, 0x66, production);
        let third = scene
            .compose_display_list(output, std::slice::from_ref(&state), &list, None)
            .unwrap()
            .frame
            .bytes
            .clone();
        assert_eq!(&third[..], &[0x66; 16], "an evicted handle may be reused");
        assert_eq!(&first[..], &[0x22; 16]);
        assert_eq!(&second[..], &[0x44; 16]);
    }
}
