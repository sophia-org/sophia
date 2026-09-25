#![cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]

//! A queued Present composes the scene it plans, not the scene it was enqueued with.
//!
//! A Present parked behind a layout epoch used to carry the CPU layers captured when
//! it entered the queue. Every surface admitted while it waited was absent from that
//! set but present in the candidate it replanned on release, and the head plan then
//! named a buffer its own sources could not resolve.

use std::fs::File;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::Instant;

use sophia_backend_live::{
    LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888, LivePresentationResourceSession,
    LiveProductionAuthorityBatch, LiveProductionAuthorityGroup, LiveProductionPresentDisposition,
    LiveProductionPresentGate, LiveProductionPresentScheduler, LiveProductionPresentSubmission,
    live_present_head_composition_sources,
};
use sophia_engine::{
    HeadRenderTarget, HeadlessEngine, ProductionSessionCoordinator, RenderHeadId,
    SurfaceChromeStyle, build_output_head_plans, output_scene_snapshot_from_committed_in_view,
    surface_chrome_display_list,
};
use sophia_protocol::{
    AuthorityKind, BufferHandle, BufferSource, CommittedSurfaceState, DRM_FORMAT_MOD_INVALID,
    DmaBufDescriptor, DmaBufPlaneDescriptor, OutputHeadMapping, OutputId, OutputTransform, Rect,
    Region, Size, SurfaceContentSet, SurfaceId, SurfaceTransaction, SurfaceTransactionReadiness,
    TransactionId,
};
use sophia_renderer_live::{
    LiveCpuBufferSource, LiveCpuBufferUpdate, LiveHeadCompositionLoweringError,
    LiveOwnedDmaBufPlane, LiveOwnedHeadCompositionSource, LiveOwnedHeadCompositionSourceKind,
    LiveOwnedMultiPlaneDmaBufFrame, LiveProductionCpuScene, LiveRendererImageId,
    lower_head_composition_plan,
};

/// The Kitty guide surface, presenting through DMA-BUF.
const GUIDE: SurfaceId = SurfaceId::new(14, 1);
/// The browser admitted by Super+B while the guide's Present was parked.
const BROWSER: SurfaceId = SurfaceId::new(18, 1);
/// The browser's CPU authority raster; the handle named in `MissingCpuSource(4)`.
const BROWSER_HANDLE: u64 = 4;
const OUTPUT: OutputId = OutputId::from_raw(1);
const SIZE: Size = Size {
    width: 64,
    height: 48,
};

fn fd() -> OwnedFd {
    File::open("/dev/null").unwrap().into()
}

fn geometry() -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: SIZE.width,
        height: SIZE.height,
    }
}

fn descriptor(handle: BufferHandle) -> DmaBufDescriptor {
    DmaBufDescriptor {
        handle,
        size: SIZE,
        format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        modifier: DRM_FORMAT_MOD_INVALID,
        plane_count: 1,
        planes: [
            Some(DmaBufPlaneDescriptor {
                offset: 0,
                stride: 256,
            }),
            None,
            None,
            None,
        ],
    }
}

fn committed(
    surface: SurfaceId,
    generation: u64,
    content: SurfaceContentSet,
) -> CommittedSurfaceState {
    CommittedSurfaceState {
        surface,
        geometry: geometry(),
        committed_generation: generation,
        content,
        damage: Region::single(geometry()),
    }
}

fn dma_buf_content(handle: BufferHandle) -> SurfaceContentSet {
    SurfaceContentSet::singleton(
        BufferSource::DmaBuf {
            handle: handle.raw(),
        },
        SIZE,
    )
}

fn cpu_content(handle: u64) -> SurfaceContentSet {
    SurfaceContentSet::singleton(BufferSource::CpuBuffer { handle }, SIZE)
}

/// The guide's Present, staged behind a layout epoch exactly as the physical run did.
fn staged_present_batch(
    transaction: TransactionId,
    handle: BufferHandle,
    epoch: TransactionId,
) -> LiveProductionAuthorityBatch {
    LiveProductionAuthorityBatch {
        groups: vec![LiveProductionAuthorityGroup {
            transaction,
            transactions: vec![SurfaceTransaction {
                input_region: None,
                transaction,
                authority: AuthorityKind::SophiaX,
                surface: GUIDE,
                namespace: None,
                target_geometry: geometry(),
                presentation_extent: SIZE,
                content: dma_buf_content(handle),
                damage: Region::single(geometry()),
                readiness: SurfaceTransactionReadiness::Ready,
                timeout_msec: 250,
                previous_committed_generation: 0,
            }],
            cpu_buffer_updates: Vec::new(),
            removed_surfaces: Vec::new(),
            present_submissions: vec![LiveProductionPresentSubmission {
                transaction,
                surface: GUIDE,
                buffer: handle,
                x_offset: 0,
                y_offset: 0,
                acquire_fence: None,
                idle_fence: None,
                layout_disposition: LiveProductionPresentDisposition::StageLayout { epoch },
            }],
            software_present_submissions: Vec::new(),
        }],
        dma_buf_registrations: Vec::new(),
        fence_registrations: Vec::new(),
        released_dma_bufs: Vec::new(),
        released_fences: Vec::new(),
    }
}

/// The frame the renderer just composed for the surface being presented.
fn current_source(candidate: &SurfaceTransaction) -> LiveOwnedHeadCompositionSource {
    LiveOwnedHeadCompositionSource {
        surface: GUIDE,
        source: candidate.target_buffer(),
        kind: LiveOwnedHeadCompositionSourceKind::DmaBuf {
            image_id: LiveRendererImageId::from_raw(1),
            frame: LiveOwnedMultiPlaneDmaBufFrame {
                width: SIZE.width as u32,
                height: SIZE.height as u32,
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                modifier: DRM_FORMAT_MOD_INVALID,
                plane_count: 1,
                planes: [
                    Some(LiveOwnedDmaBufPlane {
                        fd: fd(),
                        offset: 0,
                        stride: 256,
                    }),
                    None,
                    None,
                    None,
                ],
            },
        },
    }
}

fn head_target() -> HeadRenderTarget {
    HeadRenderTarget {
        head: RenderHeadId::from_raw(1),
        output: OUTPUT,
        target_generation: 1,
        native_size: SIZE,
        scale: 1,
        refresh_millihz: 60_000,
        transform: OutputTransform::Normal,
        mapping: OutputHeadMapping::Fit,
    }
}

/// Drives the scheduler to the exact state the crashed run reached, then returns the
/// rebased candidate the Present plans against.
fn parked_present_released_after_a_late_admission() -> (
    Vec<CommittedSurfaceState>,
    SurfaceTransaction,
    LiveProductionCpuScene,
) {
    let epoch = TransactionId::from_raw(20);
    let transaction = TransactionId::from_raw(2009);
    let guide_handle = BufferHandle::from_raw(7);

    let mut resources = LivePresentationResourceSession::default();
    resources
        .register_source(descriptor(guide_handle), vec![fd()])
        .unwrap();
    let mut scheduler = LiveProductionPresentScheduler::default();

    // The Present is enqueued and parked while the browser does not yet exist. This
    // is the moment whose CPU layer set used to be frozen into the queue.
    let batch = staged_present_batch(transaction, guide_handle, epoch);
    scheduler
        .enqueue_group(&batch.groups[0], &[], &mut resources, Instant::now())
        .unwrap();

    // Super+B admits the browser with a CPU authority raster.
    let committed_scene = vec![
        committed(GUIDE, 2, dma_buf_content(guide_handle)),
        committed(BROWSER, 5, cpu_content(BROWSER_HANDLE)),
    ];
    let mut scene = LiveProductionCpuScene::new(SIZE);
    scene
        .apply_updates(vec![LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
            handle: BROWSER_HANDLE,
            size: SIZE,
            stride: SIZE.width as u32 * 4,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            generation: 1,
            bytes: Arc::new(vec![
                0xff;
                (SIZE.width as usize) * (SIZE.height as usize) * 4
            ]),
        })])
        .unwrap();

    // The layout epoch commits and the parked Present becomes runnable.
    assert_eq!(scheduler.commit_layout_epoch(epoch), 1);
    scheduler.release_layout_deferred_for_surfaces(&[GUIDE, BROWSER], &committed_scene);
    assert!(matches!(
        scheduler.poll_gate(&mut resources, Instant::now()).unwrap(),
        LiveProductionPresentGate::Ready(ready) if ready == transaction
    ));

    let queued = scheduler.front().unwrap().candidate.clone();
    let production = ProductionSessionCoordinator::new(HeadlessEngine::default())
        .with_committed_surfaces(committed_scene.clone());
    let prepared = production.prepare_present_transaction(&queued);
    assert!(prepared.is_ready());

    (prepared.candidate().to_vec(), queued, scene)
}

#[test]
fn a_released_present_resolves_a_surface_admitted_after_it_was_queued() {
    let (candidate, queued, scene) = parked_present_released_after_a_late_admission();
    let order = vec![BROWSER, GUIDE];

    let display_list = surface_chrome_display_list(
        OUTPUT,
        &order,
        &candidate,
        Some(BROWSER),
        SurfaceChromeStyle::default(),
    )
    .unwrap();
    // Sources are read from the same candidate the plan is built from.
    let cpu_layers = scene.presentation_variant_layers(&candidate, &order);
    let sources = live_present_head_composition_sources(
        GUIDE,
        current_source(&queued),
        sophia_backend_live::LivePresentSampling::Required,
        &candidate,
        [&display_list],
        &cpu_layers,
        |_| None,
        |_| None,
    )
    .unwrap();

    let snapshot = output_scene_snapshot_from_committed_in_view(
        OUTPUT,
        queued.transaction.raw(),
        geometry(),
        &candidate,
        display_list,
        None,
    )
    .unwrap();
    let plans = build_output_head_plans(&snapshot, &[head_target()]).unwrap();
    let frame = lower_head_composition_plan(&plans[0], &sources)
        .expect("a released Present must resolve every surface its plan names");

    assert!(
        frame.layers.iter().any(|layer| matches!(
            layer,
            sophia_renderer_live::LiveOwnedMixedCompositionLayer::Cpu { .. }
        )),
        "the browser's CPU authority raster must reach the composed frame"
    );
}

/// The defect, pinned executably: sourcing from the enqueue-time scene reproduces the
/// exact error the physical run died with.
#[test]
fn enqueue_time_sources_lose_the_late_admission() {
    let (candidate, queued, scene) = parked_present_released_after_a_late_admission();
    let order = vec![BROWSER, GUIDE];

    let display_list = surface_chrome_display_list(
        OUTPUT,
        &order,
        &candidate,
        Some(BROWSER),
        SurfaceChromeStyle::default(),
    )
    .unwrap();
    // What the queue used to carry: layers built when only the guide was committed.
    let enqueue_time = vec![committed(GUIDE, 2, queued.content.clone())];
    let frozen_layers = scene.presentation_variant_layers(&enqueue_time, &order);
    assert!(frozen_layers.is_empty());

    let sources = vec![current_source(&queued)];
    let snapshot = output_scene_snapshot_from_committed_in_view(
        OUTPUT,
        queued.transaction.raw(),
        geometry(),
        &candidate,
        display_list,
        None,
    )
    .unwrap();
    let plans = build_output_head_plans(&snapshot, &[head_target()]).unwrap();

    assert_eq!(
        lower_head_composition_plan(&plans[0], &sources).unwrap_err(),
        LiveHeadCompositionLoweringError::MissingCpuSource(BROWSER_HANDLE)
    );
}

#[test]
fn a_secondary_output_present_sources_each_output_without_primary_leakage() {
    let (mut candidate, queued, scene) = parked_present_released_after_a_late_admission();
    let secondary = OutputId::from_raw(2);
    candidate
        .iter_mut()
        .find(|state| state.surface == GUIDE)
        .unwrap()
        .geometry
        .x = SIZE.width;
    let order = [BROWSER, GUIDE];
    let owners = std::collections::BTreeMap::from([(BROWSER, OUTPUT), (GUIDE, secondary)]);
    let lists = [OUTPUT, secondary].map(|output| {
        surface_chrome_display_list(
            output,
            &sophia_backend_live::live_surfaces_owned_by_output(&order, &owners, output),
            &candidate,
            None,
            SurfaceChromeStyle::default(),
        )
        .unwrap()
    });
    let cpu_layers = scene.presentation_variant_layers(&candidate, &order);
    let primary_only = live_present_head_composition_sources(
        GUIDE,
        current_source(&queued),
        sophia_backend_live::LivePresentSampling::Required,
        &candidate,
        [&lists[0]],
        &cpu_layers,
        |_| None,
        |_| None,
    );
    assert_eq!(
        primary_only.unwrap_err().to_string(),
        "visible Present surface is missing from the presentation order",
        "an omitted owner must remain an error rather than silently lose the Present"
    );
    let secondary_only = live_present_head_composition_sources(
        GUIDE,
        current_source(&queued),
        sophia_backend_live::LivePresentSampling::Required,
        &candidate,
        [&lists[1]],
        &cpu_layers,
        |_| panic!("the unrelated primary output must not need a retained source"),
        |_| panic!("the unrelated primary output must not need a direct source"),
    )
    .unwrap();
    assert_eq!(secondary_only.len(), 1);
    let sources = live_present_head_composition_sources(
        GUIDE,
        current_source(&queued),
        sophia_backend_live::LivePresentSampling::Required,
        &candidate,
        &lists,
        &cpu_layers,
        |_| None,
        |_| None,
    )
    .expect("a secondary Present cannot use only the primary output's source list");
    assert_eq!(sources.len(), 2);
    for (index, list) in lists.into_iter().enumerate() {
        let output = [OUTPUT, secondary][index];
        let snapshot = output_scene_snapshot_from_committed_in_view(
            output,
            queued.transaction.raw(),
            Rect {
                x: if index == 0 { 0 } else { SIZE.width },
                ..geometry()
            },
            &candidate,
            list,
            None,
        )
        .unwrap();
        let target = HeadRenderTarget {
            output,
            head: RenderHeadId::from_raw(index as u64 + 1),
            ..head_target()
        };
        let plans = build_output_head_plans(&snapshot, &[target]).unwrap();
        let frame = lower_head_composition_plan(&plans[0], &sources).unwrap();
        let cpu = frame
            .layers
            .iter()
            .filter(|layer| {
                matches!(
                    layer,
                    sophia_renderer_live::LiveOwnedMixedCompositionLayer::Cpu { .. }
                )
            })
            .count();
        let gpu = frame
            .layers
            .iter()
            .filter(|layer| {
                matches!(
                    layer,
                    sophia_renderer_live::LiveOwnedMixedCompositionLayer::DmaBuf { .. }
                )
            })
            .count();
        assert_eq!((cpu, gpu), if index == 0 { (1, 0) } else { (0, 1) });
    }
}
