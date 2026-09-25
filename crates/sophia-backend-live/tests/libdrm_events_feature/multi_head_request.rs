fn head_objects(
    connector: u32,
    crtc: u32,
    plane: u32,
    framebuffer: u32,
    size: Size,
) -> LibdrmNativePrimaryPlaneObjects {
    LibdrmNativePrimaryPlaneObjects::new(
        drm::control::from_u32(connector).expect("connector handle should be nonzero"),
        drm::control::from_u32(crtc).expect("crtc handle should be nonzero"),
        drm::control::from_u32(plane).expect("plane handle should be nonzero"),
        drm::control::from_u32(framebuffer).expect("framebuffer handle should be nonzero"),
        15,
        size,
    )
}

fn head(
    connector: u32,
    crtc: u32,
    plane: u32,
    framebuffer: u32,
    size: Size,
) -> LibdrmNativeAtomicHead {
    LibdrmNativeAtomicHead::new(
        head_objects(connector, crtc, plane, framebuffer, size),
        primary_plane_properties(),
    )
}

fn disabled_head(
    connector: u32,
    crtc: u32,
    plane: u32,
) -> sophia_backend_live::LibdrmNativeAtomicDisabledHead {
    sophia_backend_live::LibdrmNativeAtomicDisabledHead::new(
        LibdrmNativePrimaryPlaneSelection::new(
            drm::control::from_u32(connector).unwrap(),
            drm::control::from_u32(crtc).unwrap(),
            drm::control::from_u32(plane).unwrap(),
            scanout_size(1, 1),
            None,
        ),
        primary_plane_properties(),
    )
}

fn scanout_size(width: i32, height: i32) -> Size {
    Size { width, height }
}

#[test]
fn topology_change_request_combines_enabled_and_disabled_heads_on_one_card() {
    use sophia_backend_live::{
        LibdrmNativeAtomicTopologyChange, LibdrmNativeMultiHeadRequestBuildStatus,
        build_native_topology_change_atomic_request,
    };

    let mut build = build_native_topology_change_atomic_request(&[
        LibdrmNativeAtomicTopologyChange::Enabled(head(
            1,
            11,
            21,
            31,
            scanout_size(1920, 1080),
        )),
        LibdrmNativeAtomicTopologyChange::Disabled(disabled_head(2, 12, 22)),
    ]);
    assert_eq!(build.status, LibdrmNativeMultiHeadRequestBuildStatus::Built);
    assert_eq!(build.heads, 2);
    let request = build
        .request
        .take()
        .expect("complete topology change should own one request")
        .allow_modeset()
        .without_page_flip_event()
        .blocking();
    assert_eq!(
        request.reduced_flags(),
        LibdrmNativeAtomicCommitFlagsReport {
            page_flip_event: false,
            nonblocking: false,
            allow_modeset: true,
            test_only: false,
        }
    );
    assert_eq!(
        request.reduced_scope(),
        LibdrmNativeAtomicCommitRequestScope::Modeset
    );
}

#[test]
fn topology_change_request_rejects_object_overlap_before_building_properties() {
    use sophia_backend_live::{
        LibdrmNativeAtomicTopologyChange, LibdrmNativeMultiHeadRequestBuildStatus,
        build_native_topology_change_atomic_request,
    };

    let build = build_native_topology_change_atomic_request(&[
        LibdrmNativeAtomicTopologyChange::Enabled(head(
            1,
            11,
            21,
            31,
            scanout_size(1920, 1080),
        )),
        LibdrmNativeAtomicTopologyChange::Disabled(disabled_head(1, 12, 22)),
    ]);
    assert_eq!(
        build.status,
        LibdrmNativeMultiHeadRequestBuildStatus::OverlappingObjects
    );
    assert!(build.request.is_none());
}

#[test]
fn topology_change_submit_is_one_blocking_modeset_without_page_flip_event() {
    use sophia_backend_live::{
        LibdrmNativeAtomicTopologyChange, NativeTopologySubmitOutcome,
        submit_native_topology_change_on_device,
    };

    let device = RecordingCommitDevice::default();
    let outcome = submit_native_topology_change_on_device(
        &device,
        &[
            LibdrmNativeAtomicTopologyChange::Enabled(head(
                1,
                11,
                21,
                31,
                scanout_size(1920, 1080),
            )),
            LibdrmNativeAtomicTopologyChange::Disabled(disabled_head(2, 12, 22)),
        ],
    );

    assert_eq!(outcome, NativeTopologySubmitOutcome::Accepted);
    let submissions = device.submissions.borrow();
    assert_eq!(submissions.len(), 1);
    let flags = submissions[0];
    assert!(flags.contains(drm::control::AtomicCommitFlags::ALLOW_MODESET));
    assert!(!flags.contains(drm::control::AtomicCommitFlags::PAGE_FLIP_EVENT));
    assert!(!flags.contains(drm::control::AtomicCommitFlags::NONBLOCK));
    assert!(!flags.contains(drm::control::AtomicCommitFlags::TEST_ONLY));
}

#[test]
fn disabled_topology_head_is_prepared_before_any_card_commit() {
    use sophia_backend_live::{
        LibdrmNativeDisabledTopologyHeadPrepareStatus, prepare_native_disabled_topology_head,
    };

    let selection = LibdrmNativePrimaryPlaneSelection::new(
        connector_handle(),
        crtc_handle(),
        plane_handle(),
        scanout_size(1920, 1080),
        None,
    );
    let prepared = prepare_native_disabled_topology_head(&full_property_lookup_device(), selection);

    assert_eq!(
        prepared.status,
        LibdrmNativeDisabledTopologyHeadPrepareStatus::Prepared
    );
    let atomic = prepared
        .prepared
        .expect("complete property discovery should prepare a disabled head")
        .atomic_head();
    assert_eq!(atomic.selection, selection);

    let missing = prepare_native_disabled_topology_head(
        &FakeNativePropertyLookupDevice {
            connector: Err(io::Error::from(io::ErrorKind::NotFound)),
            crtc: Ok(LibdrmNativePropertyHandleSet::new(Vec::<(
                String,
                drm::control::property::Handle,
            )>::new())),
            plane: Ok(LibdrmNativePropertyHandleSet::new(Vec::<(
                String,
                drm::control::property::Handle,
            )>::new())),
            connector_value: Ok(None),
        },
        selection,
    );
    assert_eq!(
        missing.status,
        LibdrmNativeDisabledTopologyHeadPrepareStatus::PropertyDiscoveryUnavailable
    );
    assert!(missing.prepared.is_none());
}

#[test]
fn mirror_damage_projection_preserves_identity_and_targets_each_head() {
    use sophia_engine::{
        CompositorBorder, CompositorDisplayCommand, CompositorDisplayList, CompositorNodeId,
        CompositorRgb8, HeadlessOutput, OutputFrameDamageSnapshot,
        OutputFramePresentationState, OutputFrameSurfaceState, SurfaceChromeRole,
    };

    let output = OutputId::from_raw(1);
    let surface = SurfaceId::new(7, 3);
    let source = scanout_size(2560, 1440);
    let destination = HeadlessOutput {
        id: output,
        size: scanout_size(1920, 1200),
        scale: 2,
    };
    let snapshot = OutputFrameDamageSnapshot {
        output: HeadlessOutput {
            id: output,
            size: source,
            scale: 1,
        },
        surfaces: vec![OutputFrameSurfaceState {
            surface,
            committed_generation: 11,
            // These heads share one origin-anchored viewport, so the root
            // placement and the head's are the same rectangle.
            logical_geometry: Rect {
                x: 128,
                y: 72,
                width: 640,
                height: 360,
            },
            geometry: Rect {
                x: 128,
                y: 72,
                width: 640,
                height: 360,
            },
            buffer: BufferSource::CpuBuffer { handle: 0x77 },
            source_size: Size {
                width: 640,
                height: 360,
            },
        }],
        compositor_display_list: CompositorDisplayList {
            output,
            commands: vec![
                CompositorDisplayCommand::Border(CompositorBorder {
                    node: CompositorNodeId::SurfaceChrome {
                        surface,
                        role: SurfaceChromeRole::FocusRing,
                    },
                    generation: 13,
                    outer: Rect {
                        x: 120,
                        y: 64,
                        width: 656,
                        height: 376,
                    },
                    inner: Rect {
                        x: 128,
                        y: 72,
                        width: 640,
                        height: 360,
                    },
                    color: CompositorRgb8 {
                        red: 0x70,
                        green: 0xb7,
                        blue: 0xff,
                    },
                }),
                CompositorDisplayCommand::Surface { surface },
            ],
        },
        software_cursor: Some(Rect {
            x: 2400,
            y: 1280,
            width: 32,
            height: 32,
        }),
    };

    let projected = project_mirror_output_damage_snapshot(
        &snapshot,
        source,
        destination,
        NativeMirrorFit::Fit,
    )
    .expect("valid per-head damage should project");

    assert_eq!(projected.output, destination);
    assert_eq!(projected.surfaces[0].surface, surface);
    assert_eq!(projected.surfaces[0].committed_generation, 11);
    assert_eq!(
        projected.surfaces[0].logical_geometry,
        snapshot.surfaces[0].logical_geometry,
        "mirror projection changes native damage geometry, preserving root input geometry"
    );
    assert_eq!(projected.surfaces[0].buffer, snapshot.surfaces[0].buffer);
    assert_eq!(
        projected.surfaces[0].geometry,
        Rect {
            x: 96,
            y: 114,
            width: 480,
            height: 270,
        }
    );
    let CompositorDisplayCommand::Border(border) = projected.compositor_display_list.commands[0]
    else {
        panic!("the projected display list should preserve command order");
    };
    assert_eq!(border.node, snapshot_border(&snapshot).node);
    assert_eq!(border.generation, 13);
    assert_eq!(
        border.outer,
        Rect {
            x: 90,
            y: 108,
            width: 492,
            height: 282,
        }
    );
    assert_eq!(border.inner, projected.surfaces[0].geometry);
    assert_eq!(
        projected.software_cursor,
        Some(Rect {
            x: 1800,
            y: 1020,
            width: 24,
            height: 24,
        })
    );

    let mut presentation = OutputFramePresentationState::new(destination).unwrap();
    let queued = presentation
        .queue(projected)
        .expect("the destination head must accept its projected snapshot");
    assert_eq!(queued.snapshot.output, destination);
}

fn snapshot_border(
    snapshot: &sophia_engine::OutputFrameDamageSnapshot,
) -> sophia_engine::CompositorBorder {
    let sophia_engine::CompositorDisplayCommand::Border(border) =
        snapshot.compositor_display_list.commands[0]
    else {
        panic!("fixture should start with a border");
    };
    border
}

fn mirror_projection_snapshot(
    source: Size,
    geometry: Rect,
) -> sophia_engine::OutputFrameDamageSnapshot {
    use sophia_engine::{
        CompositorBorder, CompositorDisplayCommand, CompositorDisplayList, CompositorNodeId,
        CompositorRgb8, HeadlessOutput, OutputFrameDamageSnapshot, OutputFrameSurfaceState,
        SurfaceChromeRole,
    };

    let output = OutputId::from_raw(1);
    let surface = SurfaceId::new(9, 2);
    OutputFrameDamageSnapshot {
        output: HeadlessOutput {
            id: output,
            size: source,
            scale: 1,
        },
        surfaces: vec![OutputFrameSurfaceState {
            surface,
            committed_generation: 5,
            logical_geometry: geometry,
            geometry,
            buffer: BufferSource::CpuBuffer { handle: 0x99 },
            source_size: Size {
                width: geometry.width,
                height: geometry.height,
            },
        }],
        compositor_display_list: CompositorDisplayList {
            output,
            commands: vec![CompositorDisplayCommand::Border(CompositorBorder {
                node: CompositorNodeId::SurfaceChrome {
                    surface,
                    role: SurfaceChromeRole::Frame,
                },
                generation: 6,
                outer: geometry,
                inner: geometry,
                color: CompositorRgb8 {
                    red: 255,
                    green: 255,
                    blue: 255,
                },
            })],
        },
        software_cursor: Some(geometry),
    }
}

#[test]
fn mirror_damage_cover_projection_preserves_the_negative_crop_origin() {
    use sophia_engine::HeadlessOutput;

    let source = scanout_size(4, 3);
    let snapshot = mirror_projection_snapshot(
        source,
        Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 3,
        },
    );
    let projected = project_mirror_output_damage_snapshot(
        &snapshot,
        source,
        HeadlessOutput {
            id: OutputId::from_raw(1),
            size: scanout_size(16, 9),
            scale: 1,
        },
        NativeMirrorFit::Cover,
    )
    .unwrap();
    let crop = Rect {
        x: 0,
        y: -1,
        width: 16,
        height: 12,
    };

    assert_eq!(projected.surfaces[0].geometry, crop);
    assert_eq!(projected.software_cursor, Some(crop));
    assert_eq!(snapshot_border(&projected).outer, crop);
    assert_eq!(snapshot_border(&projected).inner, crop);
}

#[test]
fn mirror_damage_exact_projection_preserves_both_crop_origins() {
    use sophia_engine::HeadlessOutput;

    let source = scanout_size(20, 10);
    let snapshot = mirror_projection_snapshot(
        source,
        Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 10,
        },
    );
    let projected = project_mirror_output_damage_snapshot(
        &snapshot,
        source,
        HeadlessOutput {
            id: OutputId::from_raw(1),
            size: scanout_size(8, 6),
            scale: 1,
        },
        NativeMirrorFit::Exact,
    )
    .unwrap();

    assert_eq!(
        projected.surfaces[0].geometry,
        Rect {
            x: -6,
            y: -2,
            width: 20,
            height: 10,
        }
    );
}

#[test]
fn mirror_damage_projection_keeps_tiny_rectangles_that_round_to_zero() {
    use sophia_engine::HeadlessOutput;

    let source = scanout_size(100, 100);
    let snapshot = mirror_projection_snapshot(
        source,
        Rect {
            x: 1,
            y: 1,
            width: 1,
            height: 1,
        },
    );
    let projected = project_mirror_output_damage_snapshot(
        &snapshot,
        source,
        HeadlessOutput {
            id: OutputId::from_raw(1),
            size: scanout_size(1, 1),
            scale: 1,
        },
        NativeMirrorFit::Fit,
    )
    .unwrap();
    let rounded = Rect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };

    assert_eq!(projected.surfaces[0].geometry, rounded);
    assert_eq!(projected.software_cursor, Some(rounded));
    assert_eq!(snapshot_border(&projected).outer, rounded);
    assert_eq!(snapshot_border(&projected).inner, rounded);
}

#[test]
fn invalid_mirror_damage_projection_fails_before_changing_the_snapshot() {
    use sophia_engine::{CompositorDisplayList, HeadlessOutput, OutputFrameDamageSnapshot};

    let output = OutputId::from_raw(1);
    let source = scanout_size(2560, 1440);
    let snapshot = OutputFrameDamageSnapshot {
        output: HeadlessOutput {
            id: output,
            size: source,
            scale: 1,
        },
        surfaces: Vec::new(),
        compositor_display_list: CompositorDisplayList::empty(output),
        software_cursor: None,
    };
    let original = snapshot.clone();
    let wrong_source = scanout_size(1920, 1080);

    assert!(
        project_mirror_output_damage_snapshot(
            &snapshot,
            wrong_source,
            HeadlessOutput {
                id: output,
                size: scanout_size(1920, 1080),
                scale: 1,
            },
            NativeMirrorFit::Fit,
        )
        .is_err()
    );
    assert_eq!(snapshot, original);
}

#[test]
fn independent_heads_combine_into_one_atomic_request() {
    let first = head(21, 41, 51, 61, scanout_size(1920, 1080));
    let second = head(22, 42, 52, 62, scanout_size(2560, 1440));

    let build = build_native_multi_head_atomic_request(
        &[first, second],
        LibdrmNativeAtomicCommitRequestScope::Modeset,
    );

    assert_eq!(
        build.status,
        LibdrmNativeMultiHeadRequestBuildStatus::Built
    );
    assert_eq!(build.heads, 2);
    assert!(build.request.is_some());
}

#[test]
fn a_mirror_group_shares_one_framebuffer_across_heads() {
    // Two connectors, two CRTCs, two planes, one framebuffer: this is mirroring.
    let primary = head(21, 41, 51, 61, scanout_size(1920, 1080));
    let mirrored = head(22, 42, 52, 61, scanout_size(1920, 1080));

    let build = build_native_multi_head_atomic_request(
        &[primary, mirrored],
        LibdrmNativeAtomicCommitRequestScope::Modeset,
    );

    assert_eq!(
        build.status,
        LibdrmNativeMultiHeadRequestBuildStatus::Built
    );
    assert_eq!(build.heads, 2);
}

#[test]
fn a_mirror_group_with_mismatched_sizes_fails_closed() {
    // Same framebuffer, different scanout size. No primary plane scaling exists,
    // so this must be rejected rather than letterboxed or sent to the kernel.
    let primary = head(21, 41, 51, 61, scanout_size(1920, 1080));
    let mirrored = head(22, 42, 52, 61, scanout_size(1280, 720));

    let build = build_native_multi_head_atomic_request(
        &[primary, mirrored],
        LibdrmNativeAtomicCommitRequestScope::Modeset,
    );

    assert_eq!(
        build.status,
        LibdrmNativeMultiHeadRequestBuildStatus::MismatchedMirrorSize
    );
    assert!(build.request.is_none());
}

#[test]
fn differing_sizes_are_allowed_when_heads_do_not_share_a_framebuffer() {
    // The same-size rule is a mirror-group rule, not a desktop-wide one.
    let first = head(21, 41, 51, 61, scanout_size(1920, 1080));
    let second = head(22, 42, 52, 62, scanout_size(1280, 720));

    let build = build_native_multi_head_atomic_request(
        &[first, second],
        LibdrmNativeAtomicCommitRequestScope::Modeset,
    );

    assert_eq!(
        build.status,
        LibdrmNativeMultiHeadRequestBuildStatus::Built
    );
}

#[test]
fn heads_sharing_a_kms_object_fail_closed() {
    let size = scanout_size(1920, 1080);
    for (label, second) in [
        ("connector", head(21, 42, 52, 62, size)),
        ("crtc", head(22, 41, 52, 62, size)),
        ("plane", head(22, 42, 51, 62, size)),
    ] {
        let build = build_native_multi_head_atomic_request(
            &[head(21, 41, 51, 61, size), second],
            LibdrmNativeAtomicCommitRequestScope::Modeset,
        );
        assert_eq!(
            build.status,
            LibdrmNativeMultiHeadRequestBuildStatus::OverlappingObjects,
            "a shared {label} must be rejected"
        );
        assert!(build.request.is_none());
    }
}

#[test]
fn an_empty_head_set_is_rejected() {
    let build = build_native_multi_head_atomic_request(
        &[],
        LibdrmNativeAtomicCommitRequestScope::Modeset,
    );

    assert_eq!(build.status, LibdrmNativeMultiHeadRequestBuildStatus::NoHeads);
    assert!(build.request.is_none());
}

#[test]
fn an_invalid_size_on_any_head_rejects_the_whole_request() {
    let build = build_native_multi_head_atomic_request(
        &[
            head(21, 41, 51, 61, scanout_size(1920, 1080)),
            head(22, 42, 52, 62, scanout_size(0, 1080)),
        ],
        LibdrmNativeAtomicCommitRequestScope::Modeset,
    );

    assert_eq!(
        build.status,
        LibdrmNativeMultiHeadRequestBuildStatus::InvalidSize
    );
    assert!(build.request.is_none());
}

#[test]
fn a_modeset_requires_a_mode_blob_on_every_head() {
    let size = scanout_size(1920, 1080);
    let without_blob = LibdrmNativeAtomicHead::new(
        LibdrmNativePrimaryPlaneObjects::new_with_optional_mode_blob(
            drm::control::from_u32(22).expect("connector handle should be nonzero"),
            drm::control::from_u32(42).expect("crtc handle should be nonzero"),
            drm::control::from_u32(52).expect("plane handle should be nonzero"),
            drm::control::from_u32(62).expect("framebuffer handle should be nonzero"),
            None,
            size,
        ),
        primary_plane_properties(),
    );

    let modeset = build_native_multi_head_atomic_request(
        &[head(21, 41, 51, 61, size), without_blob],
        LibdrmNativeAtomicCommitRequestScope::Modeset,
    );
    assert_eq!(
        modeset.status,
        LibdrmNativeMultiHeadRequestBuildStatus::MissingModeBlob
    );

    // A page flip changes no mode, so it needs no blob.
    let page_flip = build_native_multi_head_atomic_request(
        &[head(21, 41, 51, 61, size), without_blob],
        LibdrmNativeAtomicCommitRequestScope::PageFlip,
    );
    assert_eq!(
        page_flip.status,
        LibdrmNativeMultiHeadRequestBuildStatus::Built
    );
}

#[test]
fn a_combined_request_can_be_tested_without_touching_hardware() {
    let build = build_native_multi_head_atomic_request(
        &[
            head(21, 41, 51, 61, scanout_size(1920, 1080)),
            head(22, 42, 52, 61, scanout_size(1920, 1080)),
        ],
        LibdrmNativeAtomicCommitRequestScope::Modeset,
    );
    let request = build
        .request
        .expect("a valid mirror group should build")
        .test_only()
        .allow_modeset();

    let flags = request.reduced_flags();
    assert!(flags.test_only);
    assert!(flags.allow_modeset);
}

fn timing(width: u32, height: u32, refresh_millihz: u32) -> LibdrmNativeOutputTiming {
    LibdrmNativeOutputTiming::new(width, height, refresh_millihz)
}

#[test]
fn a_planned_timing_resolves_to_its_position_in_the_connector_mode_list() {
    let modes = [
        timing(3840, 2160, 60_000),
        timing(2560, 1440, 144_000),
        timing(1920, 1080, 60_000),
    ];

    let resolution = resolve_native_output_mode_index(&modes, timing(2560, 1440, 144_000));

    assert_eq!(
        resolution.status,
        LibdrmNativeModeResolutionStatus::Resolved
    );
    assert_eq!(resolution.index, Some(1));
}

#[test]
fn a_timing_the_connector_does_not_offer_fails_closed() {
    let modes = [timing(1920, 1080, 60_000)];

    // Right resolution, wrong refresh: still not a mode this connector has.
    let resolution = resolve_native_output_mode_index(&modes, timing(1920, 1080, 75_000));

    assert_eq!(
        resolution.status,
        LibdrmNativeModeResolutionStatus::UnknownTiming
    );
    assert!(resolution.index.is_none());
}

#[test]
fn an_invalid_request_is_rejected_before_matching() {
    let modes = [timing(1920, 1080, 60_000)];

    for requested in [
        timing(0, 1080, 60_000),
        timing(1920, 0, 60_000),
        timing(1920, 1080, 0),
    ] {
        let resolution = resolve_native_output_mode_index(&modes, requested);
        assert_eq!(
            resolution.status,
            LibdrmNativeModeResolutionStatus::InvalidTiming
        );
    }
}

#[test]
fn resolution_picks_the_first_of_several_modes_sharing_one_reduced_timing() {
    // Reducing a DRM mode to width, height, and integer refresh is lossy, so two
    // distinct modes can collide here. The capability reader dedupes and keeps the
    // first, so resolution must make the same choice or advertisement and commit
    // would disagree about which mode a timing names.
    let modes = [
        timing(1920, 1080, 60_000),
        timing(1920, 1080, 60_000),
        timing(1280, 720, 60_000),
    ];

    let resolution = resolve_native_output_mode_index(&modes, timing(1920, 1080, 60_000));

    assert_eq!(resolution.index, Some(0));
}

#[test]
fn an_invalid_advertised_mode_is_skipped_rather_than_matched() {
    // A connector can report a degenerate mode. It must never be selected, even
    // when a caller asks for something that would compare equal after reduction.
    let modes = [timing(0, 0, 0), timing(1920, 1080, 60_000)];

    let resolution = resolve_native_output_mode_index(&modes, timing(1920, 1080, 60_000));

    assert_eq!(resolution.index, Some(1));
}

#[test]
fn an_empty_mode_list_resolves_nothing() {
    let resolution = resolve_native_output_mode_index(&[], timing(1920, 1080, 60_000));

    assert_eq!(
        resolution.status,
        LibdrmNativeModeResolutionStatus::UnknownTiming
    );
}

/// Records the flags of every submission, so a test can prove the validation pass
/// really carried TEST_ONLY and that nothing else reached the device.
#[derive(Default)]
struct RecordingCommitDevice {
    submissions: std::cell::RefCell<Vec<drm::control::AtomicCommitFlags>>,
    results: std::cell::RefCell<Vec<Result<(), std::io::ErrorKind>>>,
}

impl RecordingCommitDevice {
    fn with_results(results: Vec<Result<(), std::io::ErrorKind>>) -> Self {
        Self {
            submissions: std::cell::RefCell::new(Vec::new()),
            results: std::cell::RefCell::new(results),
        }
    }
}

impl LibdrmNativeAtomicCommitDevice for RecordingCommitDevice {
    fn submit_atomic_commit(
        &self,
        flags: drm::control::AtomicCommitFlags,
        _request: drm::control::atomic::AtomicModeReq,
    ) -> io::Result<()> {
        self.submissions.borrow_mut().push(flags);
        let mut results = self.results.borrow_mut();
        if results.is_empty() {
            return Ok(());
        }
        match results.remove(0) {
            Ok(()) => Ok(()),
            Err(kind) => Err(io::Error::new(kind, "synthetic commit failure")),
        }
    }
}

fn submitted_flags(
    committer: &NativeLibdrmAtomicScanoutCommitter<RecordingCommitDevice>,
) -> Vec<drm::control::AtomicCommitFlags> {
    committer.device().submissions.borrow().clone()
}

#[test]
fn validating_a_topology_sets_test_only_and_allows_modeset() {
    let mut committer = NativeLibdrmAtomicScanoutCommitter::new(RecordingCommitDevice::default());
    let heads = [
        head(21, 41, 51, 61, scanout_size(1920, 1080)),
        head(22, 42, 52, 61, scanout_size(1920, 1080)),
    ];

    let outcome = submit_native_multi_head_topology(
        &mut committer,
        &heads,
        NativeTopologySubmitIntent::Validate,
    );

    assert_eq!(outcome, NativeTopologySubmitOutcome::Accepted);
    let flags = submitted_flags(&committer);
    assert_eq!(flags.len(), 1);
    // The safety property: validation must not be able to change anything.
    assert!(flags[0].contains(drm::control::AtomicCommitFlags::TEST_ONLY));
    assert!(flags[0].contains(drm::control::AtomicCommitFlags::ALLOW_MODESET));
}

#[test]
fn activating_a_topology_does_not_set_test_only() {
    let mut committer = NativeLibdrmAtomicScanoutCommitter::new(RecordingCommitDevice::default());
    let heads = [head(21, 41, 51, 61, scanout_size(1920, 1080))];

    let outcome = submit_native_multi_head_topology(
        &mut committer,
        &heads,
        NativeTopologySubmitIntent::Activate,
    );

    assert_eq!(outcome, NativeTopologySubmitOutcome::Accepted);
    let flags = submitted_flags(&committer);
    assert!(!flags[0].contains(drm::control::AtomicCommitFlags::TEST_ONLY));
    assert!(flags[0].contains(drm::control::AtomicCommitFlags::ALLOW_MODESET));
}

#[test]
fn an_unbuildable_topology_never_reaches_the_device() {
    let mut committer = NativeLibdrmAtomicScanoutCommitter::new(RecordingCommitDevice::default());
    // One framebuffer, two sizes: a mirror group that cannot be expressed.
    let heads = [
        head(21, 41, 51, 61, scanout_size(1920, 1080)),
        head(22, 42, 52, 61, scanout_size(1280, 720)),
    ];

    let outcome = submit_native_multi_head_topology(
        &mut committer,
        &heads,
        NativeTopologySubmitIntent::Activate,
    );

    assert_eq!(
        outcome,
        NativeTopologySubmitOutcome::Unbuildable(
            LibdrmNativeMultiHeadRequestBuildStatus::MismatchedMirrorSize
        )
    );
    assert!(submitted_flags(&committer).is_empty());
}

#[test]
fn a_kernel_refusal_and_a_busy_device_are_reported_apart() {
    let heads = [head(21, 41, 51, 61, scanout_size(1920, 1080))];

    let mut refusing = NativeLibdrmAtomicScanoutCommitter::new(RecordingCommitDevice::with_results(
        vec![Err(io::ErrorKind::InvalidInput)],
    ));
    assert_eq!(
        submit_native_multi_head_topology(
            &mut refusing,
            &heads,
            NativeTopologySubmitIntent::Validate
        ),
        NativeTopologySubmitOutcome::Rejected
    );

    let mut busy = NativeLibdrmAtomicScanoutCommitter::new(RecordingCommitDevice::with_results(
        vec![Err(io::ErrorKind::WouldBlock)],
    ));
    assert_eq!(
        submit_native_multi_head_topology(&mut busy, &heads, NativeTopologySubmitIntent::Validate),
        NativeTopologySubmitOutcome::Busy
    );
}

#[test]
fn a_probe_that_lacked_drm_master_is_not_a_rejected_topology() {
    // The distinction matters operationally: a reader who treats MasterUnavailable
    // as a rejection would conclude the hardware refused a valid topology, when in
    // fact nothing was ever validated.
    let report = NativeTopologyProbeReport {
        status: NativeTopologyProbeStatus::MasterUnavailable,
        connected_connectors: 2,
        modes_on_selected_connector: 14,
        without_plane_state: NativeTopologyValidationOutcome::NotAttempted,
        with_current_framebuffer: NativeTopologyValidationOutcome::NotAttempted,
        reused_current_framebuffer: false,
        mode_size: (2560, 1440),
        framebuffer_size: None,
    };

    assert!(!report.answered());
    let line = report.reduced_log_line();
    assert!(line.contains("status=MasterUnavailable"));
    assert!(line.contains("without_plane_state=not_attempted"));
}

#[test]
fn a_probe_answers_only_when_it_validated_something() {
    let probed = NativeTopologyProbeReport {
        status: NativeTopologyProbeStatus::Probed,
        connected_connectors: 2,
        modes_on_selected_connector: 14,
        without_plane_state: NativeTopologyValidationOutcome::Accepted,
        with_current_framebuffer: NativeTopologyValidationOutcome::Accepted,
        reused_current_framebuffer: true,
        mode_size: (2560, 1440),
        framebuffer_size: Some((2560, 1440)),
    };
    assert!(probed.answered());
    assert!(
        probed
            .reduced_log_line()
            .contains("without_plane_state=accepted")
    );

    // Probed, but the first validation never ran: no conclusion either.
    let inconclusive = NativeTopologyProbeReport {
        without_plane_state: NativeTopologyValidationOutcome::NotAttempted,
        ..probed
    };
    assert!(!inconclusive.answered());
}

#[test]
fn a_rejected_validation_is_still_a_conclusion() {
    // "The kernel refuses a modeset with no plane state" is exactly the answer the
    // probe exists to obtain, so it must count as answered.
    let report = NativeTopologyProbeReport {
        status: NativeTopologyProbeStatus::Probed,
        connected_connectors: 1,
        modes_on_selected_connector: 3,
        without_plane_state: NativeTopologyValidationOutcome::Rejected(22),
        with_current_framebuffer: NativeTopologyValidationOutcome::Accepted,
        reused_current_framebuffer: true,
        mode_size: (2560, 1440),
        framebuffer_size: Some((2560, 1440)),
    };

    assert!(report.answered());
    let line = report.reduced_log_line();
    assert!(line.contains("without_plane_state=rejected"));
    assert!(line.contains("with_current_framebuffer=accepted"));
    assert!(line.contains("reused_framebuffer=true"));

    // The errno is what separates "this driver requires plane state" from "this
    // request was malformed", and both sizes are what separate a plane-sizing
    // refusal from an FB_ID one. A rejection reported without them is not a
    // diagnosis.
    assert!(line.contains("without_plane_state_errno=22"));
    assert!(line.contains("with_current_framebuffer_errno=0"));
    assert!(line.contains("mode_size=2560x1440"));
    assert!(line.contains("framebuffer_size=2560x1440"));
}

fn topology_head(connector: u32, crtc: u32, mode_blob: u64) -> LibdrmNativeAtomicTopologyHead {
    LibdrmNativeAtomicTopologyHead::new(
        drm::control::from_u32(connector).expect("connector handle should be nonzero"),
        drm::control::from_u32(crtc).expect("crtc handle should be nonzero"),
        mode_blob,
        primary_plane_properties(),
    )
}

#[test]
fn a_topology_validates_without_naming_a_framebuffer() {
    // The shape hardware accepted: connector CRTC_ID plus CRTC MODE_ID and ACTIVE,
    // no plane state. This is what lets an inactive output be validated before a
    // framebuffer exists for it.
    let build = build_native_multi_head_topology_request(&[
        topology_head(21, 41, 71),
        topology_head(22, 42, 72),
    ]);

    assert_eq!(build.status, LibdrmNativeMultiHeadRequestBuildStatus::Built);
    assert_eq!(build.heads, 2);
    let flags = build
        .request
        .expect("a built topology carries a request")
        .test_only()
        .allow_modeset()
        .reduced_flags();
    assert!(flags.test_only);
    assert!(flags.allow_modeset);
    assert!(!flags.page_flip_event);
}

#[test]
fn a_topology_still_gives_each_connector_and_crtc_to_one_head() {
    assert_eq!(
        build_native_multi_head_topology_request(&[
            topology_head(21, 41, 71),
            topology_head(21, 42, 72),
        ])
        .status,
        LibdrmNativeMultiHeadRequestBuildStatus::OverlappingObjects
    );
    assert_eq!(
        build_native_multi_head_topology_request(&[
            topology_head(21, 41, 71),
            topology_head(22, 41, 72),
        ])
        .status,
        LibdrmNativeMultiHeadRequestBuildStatus::OverlappingObjects
    );
}

#[test]
fn a_topology_without_a_mode_blob_is_unbuildable() {
    assert_eq!(
        build_native_multi_head_topology_request(&[topology_head(21, 41, 0)]).status,
        LibdrmNativeMultiHeadRequestBuildStatus::MissingModeBlob
    );
    assert_eq!(
        build_native_multi_head_topology_request(&[]).status,
        LibdrmNativeMultiHeadRequestBuildStatus::NoHeads
    );
}

#[test]
fn an_unbuildable_topology_validation_never_reaches_the_device() {
    let mut committer = NativeLibdrmAtomicScanoutCommitter::new(RecordingCommitDevice::default());

    let outcome = validate_native_multi_head_topology(
        &mut committer,
        &[topology_head(21, 41, 71), topology_head(21, 42, 72)],
    );

    assert_eq!(
        outcome,
        NativeTopologySubmitOutcome::Unbuildable(
            LibdrmNativeMultiHeadRequestBuildStatus::OverlappingObjects
        )
    );
    assert!(submitted_flags(&committer).is_empty());
}

#[test]
fn a_validated_topology_reaches_the_device_as_one_test_only_request() {
    let mut committer = NativeLibdrmAtomicScanoutCommitter::new(RecordingCommitDevice::default());

    let outcome = validate_native_multi_head_topology(
        &mut committer,
        &[topology_head(21, 41, 71), topology_head(22, 42, 72)],
    );

    assert_eq!(outcome, NativeTopologySubmitOutcome::Accepted);
    let flags = submitted_flags(&committer);
    assert_eq!(flags.len(), 1, "one topology is one request");
    assert!(flags[0].contains(drm::control::AtomicCommitFlags::TEST_ONLY));
    assert!(flags[0].contains(drm::control::AtomicCommitFlags::ALLOW_MODESET));
    assert!(!flags[0].contains(drm::control::AtomicCommitFlags::PAGE_FLIP_EVENT));
}

#[test]
fn a_validation_only_commit_never_asks_for_a_page_flip_event() {
    // The kernel rejects TEST_ONLY together with PAGE_FLIP_EVENT with EINVAL before
    // it inspects any property. A validation carrying both therefore reports a
    // refused topology no matter what the topology is, which is exactly the false
    // negative that cost one hardware probe run.
    let request = LibdrmNativeAtomicCommitRequest::modeset(
        drm::control::atomic::AtomicModeReq::new(),
    );
    assert!(request.reduced_flags().page_flip_event);

    let validating = LibdrmNativeAtomicCommitRequest::modeset(
        drm::control::atomic::AtomicModeReq::new(),
    )
    .test_only()
    .allow_modeset();
    let flags = validating.reduced_flags();
    assert!(flags.test_only);
    assert!(flags.allow_modeset);
    assert!(!flags.page_flip_event);
}

fn size(width: i32, height: i32) -> Size {
    Size { width, height }
}

#[test]
fn a_head_matching_the_scene_takes_the_whole_buffer() {
    // The ordinary desktop, and the case a regression here would break for
    // everyone: a head whose mode equals the scene must get the entire buffer
    // under every policy, with no bars and no offset.
    for fit in [
        NativeMirrorFit::Fit,
        NativeMirrorFit::Cover,
        NativeMirrorFit::Exact,
    ] {
        assert_eq!(
            project_mirror_rect(size(1920, 1080), size(1920, 1080), fit),
            Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080
            },
            "{fit:?} must fill a head that matches the scene"
        );
    }
}

#[test]
fn a_matching_aspect_scales_to_exact_pixels() {
    // 2560x1440 into 1920x1080 is this machine's own pair. The aspects match, so
    // a correct fit lands on the buffer exactly -- no bars, no rounding short by
    // a row, which is what the rational scaling exists to guarantee.
    assert_eq!(
        project_mirror_rect(size(2560, 1440), size(1920, 1080), NativeMirrorFit::Fit),
        Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080
        }
    );
}

#[test]
fn a_taller_scene_is_letterboxed_and_centred() {
    // The case that actually exercises the policy. A 16:10 scene on a 16:9 head
    // fits by height and leaves pillarbox bars, centred to the pixel.
    let rect = project_mirror_rect(size(1920, 1200), size(1920, 1080), NativeMirrorFit::Fit);
    assert_eq!(rect.height, 1080, "fit by the constrained axis");
    assert_eq!(rect.width, 1728);
    assert_eq!(rect.x, (1920 - 1728) / 2, "centred, so the bars match");
    assert_eq!(rect.y, 0);

    // Cover is the same geometry inverted: fill the head and crop the overflow.
    let covered = project_mirror_rect(size(1920, 1200), size(1920, 1080), NativeMirrorFit::Cover);
    assert_eq!(covered.width, 1920);
    assert_eq!(covered.height, 1200);
    assert!(covered.y < 0, "the crop hangs off both edges equally");
}

#[test]
fn exact_never_resamples_and_centres_what_it_has() {
    // A scene larger than the head is cropped rather than shrunk, and one smaller
    // sits centred rather than stretched.
    let cropped = project_mirror_rect(size(2560, 1440), size(1920, 1080), NativeMirrorFit::Exact);
    assert_eq!(cropped.width, 2560);
    assert_eq!(cropped.height, 1440);
    assert!(cropped.x < 0 && cropped.y < 0);

    let inset = project_mirror_rect(size(1280, 720), size(1920, 1080), NativeMirrorFit::Exact);
    assert_eq!(
        inset,
        Rect {
            x: 320,
            y: 180,
            width: 1280,
            height: 720
        }
    );
}

#[test]
fn a_head_with_no_area_yields_an_empty_rect_rather_than_dividing_by_zero() {
    assert_eq!(
        project_mirror_rect(size(1920, 1080), size(0, 1080), NativeMirrorFit::Fit),
        Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 0
        }
    );
}

#[test]
fn one_composed_frame_lands_on_two_heads_at_two_sizes() {
    // The claim the whole projection design rests on: a group composes once and
    // each head receives that same frame placed for its own mode. Asserted on the
    // geometry, which is what decides it -- the draw is a scaled blit that takes
    // this rect verbatim.
    let scene = size(2560, 1440);

    // The head whose mode matches the scene takes the buffer whole.
    let primary = project_mirror_rect(scene, size(2560, 1440), NativeMirrorFit::Fit);
    // The head at a different mode takes the same frame, scaled to its own buffer.
    let mirrored = project_mirror_rect(scene, size(1920, 1080), NativeMirrorFit::Fit);

    assert_eq!(primary.width, 2560);
    assert_eq!(mirrored.width, 1920);
    assert_ne!(
        (primary.width, primary.height),
        (mirrored.width, mirrored.height),
        "one frame, two differently sized destinations -- if these match, \
         nothing is being projected and both heads would need one mode"
    );
    // Neither head is cropped or offset, because the modes share an aspect.
    assert_eq!((primary.x, primary.y), (0, 0));
    assert_eq!((mirrored.x, mirrored.y), (0, 0));
}

#[test]
fn mixed_layers_and_clips_follow_the_projected_scene() {
    let source = size(2560, 1440);
    let projected = project_mirror_rect(source, size(1920, 1200), NativeMirrorFit::Fit);
    assert_eq!(
        projected,
        Rect {
            x: 0,
            y: 60,
            width: 1920,
            height: 1080,
        }
    );
    assert_eq!(
        project_mirror_child_rect(
            Rect {
                x: 128,
                y: 72,
                width: 640,
                height: 360,
            },
            source,
            projected,
        ),
        Rect {
            x: 96,
            y: 114,
            width: 480,
            height: 270,
        }
    );
    assert_eq!(
        project_mirror_coordinates(128, 72, source, size(1920, 1200), NativeMirrorFit::Fit),
        Some((96, 114))
    );
}
