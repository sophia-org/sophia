//! Authority intake must select native framing, not headless settlement.
use super::presentation_instances::rect;
use super::*;

#[test]
fn native_cpu_cycle_frames_software_present_before_retirement() {
    let outputs = [outputs()[0]];
    let output = outputs[0].id;
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = MirroredTarget::new(&outputs);
    target.track_retirements();
    runtime.outputs.mark_native_initialized(output).unwrap();
    let surface = SurfaceId::new(51, 1);
    let size = Size {
        width: 8,
        height: 8,
    };
    let geometry = rect(0, 0, 8, 8);
    let transaction = SurfaceTransaction {
        transaction: TransactionId::from_raw(801),
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: geometry,
        presentation_extent: size,
        content: SurfaceContentSet::singleton(BufferSource::CpuBuffer { handle: 51 }, size),
        damage: Region::single(geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
        input_region: None,
    };
    let candidate = transaction.key();
    let batch = LiveProductionAuthorityBatch {
        groups: vec![LiveProductionAuthorityGroup {
            transaction: transaction.transaction,
            transactions: vec![transaction.clone()],
            cpu_buffer_updates: vec![LiveProductionCpuBufferUpdate::new(
                transaction.transaction,
                surface,
                LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
                    handle: 51,
                    generation: 1,
                    size,
                    stride: 32,
                    format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                    bytes: std::sync::Arc::new(vec![91; 256]),
                }),
            )],
            removed_surfaces: vec![],
            present_submissions: vec![],
            software_present_submissions: vec![LiveProductionSoftwarePresentSubmission {
                candidate,
                source_size: size,
                transaction: transaction.transaction,
                surface,
                acquire_fence: None,
                idle_fence: None,
            }],
        }],
        dma_buf_registrations: vec![],
        fence_registrations: vec![],
        released_dma_bufs: vec![],
        released_fences: vec![],
    };
    let layout = [LayerSnapshot {
        translation: None,
        surface,
        authority_local_id: None,
        namespace: None,
        stack_rank: 0,
        geometry,
        source: candidate.target_buffer,
        source_size: size,
        damage: Region::single(geometry),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: 1,
        resize_sync: ResizeSyncCapability::ImplicitOnly,
        output: Some(output),
        input_region: None,
    }];
    let (report, committed, _) = runtime
        .run_cpu_cycle_on_target(
            LiveProductionCycleRequest {
                batch: &batch,
                scene: &mut scene,
                raised_surface: None,
                focused_surface: None,
                cursor_presentation: LiveProductionCursorPresentation::Software(None),
                defer_frame: false,
                output_descriptors: &outputs,
                native_scanout: None,
                wm_update: None,
                presentation_layout: &layout,
                geometry_routed_surfaces: &[],
                chrome_surfaces: &[],
                indicator_publication: None,
                staged_cpu_buffer_handles: &[],
            },
            Some(&mut target),
        )
        .unwrap();
    assert!(report.composed);
    assert_eq!(committed.len(), 1);
    assert_eq!(committed[0].committed_generation, 1);
    assert!(
        runtime.retired_software_presents.is_empty(),
        "native cycle must not settle headlessly"
    );
    assert_eq!(
        runtime.software_present_frames_waiting.len(),
        1,
        "native cycle must frame the admitted software Present"
    );
    assert!(
        runtime
            .stage_software_present_frame(&mut target, output)
            .unwrap()
    );
    target.install(output).unwrap();
    target.prepare(output);
    let frame = NativeRetirementTarget::submitted_frame(&target, output).unwrap();
    target.completion_inputs.push_back((output, 0));
    runtime
        .retire_native_output_on_target(&mut target, output, false)
        .unwrap();
    let mut retired = Vec::new();
    runtime
        .drain_retired_software_presents_into(&mut retired)
        .unwrap();
    assert_eq!(retired.len(), 1);
    assert_eq!(retired[0].candidate, candidate);
    assert_eq!(retired[0].frame, frame);
    assert_ne!(retired[0].native_submission, 0);
    target.completion_inputs.push_back((output, 1));
    runtime
        .retire_native_output_on_target(&mut target, output, false)
        .unwrap();
    assert!(runtime.retired_software_presents.is_empty());
    target.teardown();
}
