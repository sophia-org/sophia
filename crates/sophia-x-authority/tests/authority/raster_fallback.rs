// Cause-classified sampled-fallback regressions.
//
// Every case here proves the negative direction of the density contract: when
// the semantic journal cannot answer a requirement, the surface must publish
// its canonical raster as sampled compatibility content with a named cause,
// and must never label that content as an authority-owned native variant.

fn fallback_window(
    runtime: &mut XAuthorityRuntime,
    namespace: NamespaceId,
    window: XResourceId,
    surface: SurfaceId,
    transaction: u64,
) {
    runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(transaction),
        namespace,
        kind: XAuthorityRequestKind::CreateWindow {
            window,
            surface,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 40,
                height: 20,
            },
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    });
}

fn fallback_requirement(
    surface: SurfaceId,
    committed_content_generation: u64,
    densities: &[u32],
) -> SurfaceRasterRequirements {
    SurfaceRasterRequirements {
        surface,
        committed_content_generation,
        requirement_generation: 1,
        logical_extent: Size {
            width: 40,
            height: 20,
        },
        classes: densities
            .iter()
            .map(|density| SurfaceRasterClass {
                density_millis: *density,
                transform: SurfaceRasterTransform::Normal,
            })
            .collect(),
    }
}

fn opaque_image(width: i32, height: i32, pixel: u32) -> Vec<u8> {
    let mut pixels = Vec::new();
    for _ in 0..width.saturating_mul(height) {
        pixels.extend_from_slice(&pixel.to_le_bytes());
    }
    pixels
}

fn z_pixmap_semantics() -> XPutImageSemantics {
    XPutImageSemantics {
        format: 2,
        depth: 24,
        left_pad: 0,
        byte_order: XByteOrder::LittleEndian,
        gc: XGraphicsContextValues::default(),
    }
}

/// Asserts a content set carries only the canonical 1x variant, so a poisoned
/// journal cannot leak a derived store that claims exact density.
fn assert_canonical_only(runtime: &mut XAuthorityRuntime, surface: SurfaceId, generation: u64) {
    let outcome = runtime
        .apply_surface_raster_requirements(
            TransactionId::from_raw(9_999),
            &fallback_requirement(surface, generation, &[750]),
        )
        .unwrap();
    assert!(
        matches!(outcome, XSurfaceRasterOutcome::SampledFallback { .. }),
        "a poisoned journal must never publish a derived variant"
    );
}

#[test]
fn unsupported_put_image_format_reports_its_cause_and_keeps_canonical_only() {
    let namespace = NamespaceId::from_raw(220);
    let window = XResourceId::new(0x220, 1);
    let surface = SurfaceId::new(220, 1);
    let mut runtime = XAuthorityRuntime::new();
    fallback_window(&mut runtime, namespace, window, surface, 200);

    runtime.begin_dispatch();
    let mut semantics = z_pixmap_semantics();
    // XYPixmap is outside the replayable subset.
    semantics.format = 1;
    runtime.apply_put_image(
        TransactionId::from_raw(201),
        namespace,
        window,
        Region::single(Rect {
            x: 0,
            y: 0,
            width: 8,
            height: 4,
        }),
        Some(&opaque_image(8, 4, 0x00ff_0000)),
        Some(&semantics),
    );

    assert_eq!(
        expect_raster_fallback(
            runtime
                .apply_surface_raster_requirements(
                    TransactionId::from_raw(202),
                    &fallback_requirement(surface, 2, &[750]),
                )
                .unwrap(),
            "an XYPixmap upload has no journal representation",
        ),
        XRasterFallbackCause::UnsupportedPutImage,
    );
    assert_canonical_only(&mut runtime, surface, 2);
}

#[test]
fn unjournaled_pixmap_present_invalidates_an_older_replayable_surface_journal() {
    let namespace = NamespaceId::from_raw(232);
    let window = XResourceId::new(0x232, 1);
    let pixmap = XResourceId::new(0x233, 1);
    let surface = SurfaceId::new(232, 1);
    let size = Size {
        width: 40,
        height: 20,
    };
    let full = Rect {
        x: 0,
        y: 0,
        width: size.width,
        height: size.height,
    };
    let mut runtime = XAuthorityRuntime::new();
    fallback_window(&mut runtime, namespace, window, surface, 320);

    runtime.begin_dispatch();
    runtime.apply_core_draw(
        TransactionId::from_raw(321),
        namespace,
        window,
        Region::single(full),
    );
    runtime.take_cpu_buffer_updates();
    runtime
        .create_pixmap(namespace, pixmap, size, 24, 1)
        .unwrap();
    runtime.begin_dispatch();
    runtime.apply_put_image(
        TransactionId::from_raw(322),
        namespace,
        pixmap,
        Region::single(full),
        Some(&opaque_image(size.width, size.height, 0x0011_2233)),
        None,
    );
    runtime.take_cpu_buffer_updates();
    runtime.begin_dispatch();
    let presented = runtime.present_standard_pixmap(
        TransactionId::from_raw(323),
        namespace,
        window,
        pixmap,
        0,
        0,
        None,
        None,
    );
    assert_eq!(presented.outcome, XAuthorityResponseOutcome::Accepted);

    assert_eq!(
        expect_raster_fallback(
            runtime
                .apply_surface_raster_requirements(
                    TransactionId::from_raw(324),
                    &fallback_requirement(surface, 3, &[750]),
                )
                .unwrap(),
            "an unjournaled presentation must not replay the older surface journal",
        ),
        XRasterFallbackCause::UnsupportedCommand,
    );
}

#[test]
fn partial_draw_after_resize_cannot_replay_over_a_blank_derived_surface() {
    let namespace = NamespaceId::from_raw(234);
    let window = XResourceId::new(0x234, 1);
    let surface = SurfaceId::new(234, 1);
    let mut runtime = XAuthorityRuntime::new();
    fallback_window(&mut runtime, namespace, window, surface, 330);

    runtime.begin_dispatch();
    runtime.apply_core_draw(
        TransactionId::from_raw(331),
        namespace,
        window,
        Region::single(Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 20,
        }),
    );
    runtime.take_cpu_buffer_updates();
    runtime
        .configure_window_from_engine(
            namespace,
            window,
            Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 40,
            },
        )
        .unwrap();
    runtime.begin_dispatch();
    runtime.apply_core_draw(
        TransactionId::from_raw(332),
        namespace,
        window,
        Region::single(Rect {
            x: 2,
            y: 2,
            width: 4,
            height: 4,
        }),
    );

    let requirements = SurfaceRasterRequirements {
        surface,
        committed_content_generation: 3,
        requirement_generation: 1,
        logical_extent: Size {
            width: 80,
            height: 40,
        },
        classes: vec![SurfaceRasterClass {
            density_millis: 750,
            transform: SurfaceRasterTransform::Normal,
        }],
    };
    assert_eq!(
        expect_raster_fallback(
            runtime
                .apply_surface_raster_requirements(
                    TransactionId::from_raw(333),
                    &requirements,
                )
                .unwrap(),
            "a partial post-resize command cannot reconstruct copied canonical pixels",
        ),
        XRasterFallbackCause::LogicalExtentMismatch,
    );
}

#[test]
fn clipped_or_non_copy_put_image_is_not_retained_as_replayable() {
    let namespace = NamespaceId::from_raw(221);
    let surface = SurfaceId::new(221, 1);
    for (index, mutate) in [
        // A non-GXcopy function does not reproduce the canonical bytes.
        (0_u64, (|gc: &mut XGraphicsContextValues| gc.function = 6) as fn(&mut _)),
        // A partial plane mask leaves some visible planes untouched.
        (1, |gc: &mut XGraphicsContextValues| gc.plane_mask = 0x0000_00ff),
        // Clipping means the upload did not write every named pixel.
        (2, |gc: &mut XGraphicsContextValues| {
            gc.clip_rectangles = Some(vec![Rect {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            }]);
        }),
        // An explicitly empty clip must not become an unrestricted upload
        // when the journal is replayed at a different density.
        (3, |gc: &mut XGraphicsContextValues| {
            gc.clip_rectangles = Some(Vec::new());
        }),
    ] {
        let window = XResourceId::new(0x230 + index, 1);
        let mut runtime = XAuthorityRuntime::new();
        fallback_window(&mut runtime, namespace, window, surface, 210 + index);

        runtime.begin_dispatch();
        let mut semantics = z_pixmap_semantics();
        mutate(&mut semantics.gc);
        runtime.apply_put_image(
            TransactionId::from_raw(211 + index),
            namespace,
            window,
            Region::single(Rect {
                x: 0,
                y: 0,
                width: 8,
                height: 4,
            }),
            Some(&opaque_image(8, 4, 0x0000_ff00)),
            Some(&semantics),
        );

        assert_eq!(
            expect_raster_fallback(
                runtime
                    .apply_surface_raster_requirements(
                        TransactionId::from_raw(212 + index),
                        &fallback_requirement(surface, 2, &[750]),
                    )
                    .unwrap(),
                "a conditional upload must not be retained as replayable",
            ),
            XRasterFallbackCause::UnsupportedPutImage,
        );
    }
}

#[test]
fn absent_put_image_semantics_reports_unsupported_rather_than_guessing() {
    let namespace = NamespaceId::from_raw(222);
    let window = XResourceId::new(0x222, 1);
    let surface = SurfaceId::new(222, 1);
    let mut runtime = XAuthorityRuntime::new();
    fallback_window(&mut runtime, namespace, window, surface, 220);

    runtime.begin_dispatch();
    runtime.apply_put_image(
        TransactionId::from_raw(221),
        namespace,
        window,
        Region::single(Rect {
            x: 0,
            y: 0,
            width: 8,
            height: 4,
        }),
        Some(&opaque_image(8, 4, 0x0012_3456)),
        None,
    );

    assert_eq!(
        expect_raster_fallback(
            runtime
                .apply_surface_raster_requirements(
                    TransactionId::from_raw(222),
                    &fallback_requirement(surface, 2, &[750]),
                )
                .unwrap(),
            "an unvouched upload must fail closed",
        ),
        XRasterFallbackCause::UnsupportedPutImage,
    );
}

#[test]
fn full_opaque_put_image_establishes_a_new_replayable_baseline() {
    let namespace = NamespaceId::from_raw(223);
    let window = XResourceId::new(0x223, 1);
    let surface = SurfaceId::new(223, 1);
    let mut runtime = XAuthorityRuntime::new();
    fallback_window(&mut runtime, namespace, window, surface, 230);

    // Poison the journal first, so recovery is what the baseline proves.
    runtime.begin_dispatch();
    let mut unsupported = z_pixmap_semantics();
    unsupported.format = 1;
    runtime.apply_put_image(
        TransactionId::from_raw(231),
        namespace,
        window,
        Region::single(Rect {
            x: 0,
            y: 0,
            width: 8,
            height: 4,
        }),
        Some(&opaque_image(8, 4, 0x0000_00ff)),
        Some(&unsupported),
    );
    assert_eq!(
        expect_raster_fallback(
            runtime
                .apply_surface_raster_requirements(
                    TransactionId::from_raw(232),
                    &fallback_requirement(surface, 2, &[750]),
                )
                .unwrap(),
            "the journal must be poisoned before recovery is meaningful",
        ),
        XRasterFallbackCause::UnsupportedPutImage,
    );

    // A partial upload covers only part of the drawable, so it cannot serve as
    // a baseline and the journal stays poisoned.
    runtime.begin_dispatch();
    runtime.apply_put_image(
        TransactionId::from_raw(233),
        namespace,
        window,
        Region::single(Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 10,
        }),
        Some(&opaque_image(40, 10, 0x0011_2233)),
        Some(&z_pixmap_semantics()),
    );
    assert_eq!(
        expect_raster_fallback(
            runtime
                .apply_surface_raster_requirements(
                    TransactionId::from_raw(234),
                    &fallback_requirement(surface, 3, &[750]),
                )
                .unwrap(),
            "a partial upload must not discard the poisoned journal",
        ),
        XRasterFallbackCause::UnsupportedPutImage,
    );

    // A full-window unconditional upload reproduces the canonical drawable on
    // its own, so it may replace the journal.
    runtime.begin_dispatch();
    runtime.apply_put_image(
        TransactionId::from_raw(235),
        namespace,
        window,
        Region::single(Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 20,
        }),
        Some(&opaque_image(40, 20, 0x0044_5566)),
        Some(&z_pixmap_semantics()),
    );
    let response = expect_satisfied_raster(
        runtime
            .apply_surface_raster_requirements(
                TransactionId::from_raw(236),
                &fallback_requirement(surface, 4, &[750]),
            )
            .unwrap(),
        "a full opaque upload must re-establish a replayable baseline",
    );
    assert_eq!(response.transaction.content.variants().len(), 2);
    assert!(
        response
            .transaction
            .content
            .variants()
            .iter()
            .all(|variant| variant.fidelity
                == sophia_protocol::SurfaceContentFidelity::AuthorityRaster)
    );
}

#[test]
fn over_budget_put_image_stream_reports_journal_capacity() {
    const EXTENT: i32 = 600;
    let namespace = NamespaceId::from_raw(224);
    let window = XResourceId::new(0x224, 1);
    let surface = SurfaceId::new(224, 1);
    let mut runtime = XAuthorityRuntime::new();
    runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(240),
        namespace,
        kind: XAuthorityRequestKind::CreateWindow {
            window,
            surface,
            geometry: Rect {
                x: 0,
                y: 0,
                width: EXTENT,
                height: EXTENT,
            },
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    });

    // Each retained upload owns its own pixels, so a few large ones cross the
    // 4 MiB payload bound. Every rectangle stops one row short of the window,
    // which keeps any single command from qualifying as a new baseline.
    let pixels = opaque_image(EXTENT, EXTENT - 1, 0x0077_8899);
    let mut generation = 1_u64;
    for index in 0..3 {
        runtime.begin_dispatch();
        runtime.apply_put_image(
            TransactionId::from_raw(241 + index),
            namespace,
            window,
            Region::single(Rect {
                x: 0,
                y: 1,
                width: EXTENT,
                height: EXTENT - 1,
            }),
            Some(&pixels),
            Some(&z_pixmap_semantics()),
        );
        generation = generation.saturating_add(1);
    }

    let requirement = SurfaceRasterRequirements {
        surface,
        committed_content_generation: generation,
        requirement_generation: 1,
        logical_extent: Size {
            width: EXTENT,
            height: EXTENT,
        },
        classes: vec![SurfaceRasterClass {
            density_millis: 750,
            transform: SurfaceRasterTransform::Normal,
        }],
    };
    assert_eq!(
        expect_raster_fallback(
            runtime
                .apply_surface_raster_requirements(TransactionId::from_raw(260), &requirement)
                .unwrap(),
            "an over-budget journal must report capacity, not exact content",
        ),
        XRasterFallbackCause::JournalCapacity,
    );
}

#[test]
fn transform_requirement_reports_transform_mismatch() {
    let namespace = NamespaceId::from_raw(225);
    let window = XResourceId::new(0x225, 1);
    let surface = SurfaceId::new(225, 1);
    let mut runtime = XAuthorityRuntime::new();
    fallback_window(&mut runtime, namespace, window, surface, 270);

    runtime.begin_dispatch();
    runtime.apply_core_draw(
        TransactionId::from_raw(271),
        namespace,
        window,
        Region::single(Rect {
            x: 1,
            y: 1,
            width: 4,
            height: 4,
        }),
    );

    let mut requirements = fallback_requirement(surface, 2, &[750]);
    requirements.classes[0].transform = SurfaceRasterTransform::Rotate90;
    assert_eq!(
        expect_raster_fallback(
            runtime
                .apply_surface_raster_requirements(TransactionId::from_raw(272), &requirements)
                .unwrap(),
            "derived stores render the normal transform only",
        ),
        XRasterFallbackCause::TransformMismatch,
    );
}

#[test]
fn a_draw_through_a_clip_pixmap_is_not_replayed_unmasked() {
    let namespace = NamespaceId::from_raw(226);
    let window = XResourceId::new(0x226, 1);
    let surface = SurfaceId::new(226, 1);
    let mut runtime = XAuthorityRuntime::new();
    fallback_window(&mut runtime, namespace, window, surface, 280);

    // The journal applies the clip list but cannot project a mask, so a
    // masked paint must refuse replay rather than replay without it.
    runtime.begin_dispatch();
    runtime.apply_core_draw_with_gc(
        TransactionId::from_raw(281),
        namespace,
        window,
        Region::single(Rect {
            x: 1,
            y: 1,
            width: 4,
            height: 4,
        }),
        &XGraphicsContextValues {
            clip_mask: Some(XResourceId::new(0x227, 1)),
            ..Default::default()
        },
    );

    assert_eq!(
        expect_raster_fallback(
            runtime
                .apply_surface_raster_requirements(
                    TransactionId::from_raw(282),
                    &fallback_requirement(surface, 2, &[750]),
                )
                .unwrap(),
            "a clip pixmap has no journal projection",
        ),
        XRasterFallbackCause::UnsupportedClipMask,
    );
}

#[test]
fn a_draw_through_a_clip_pixmap_is_not_replayed_unmasked_through_a_tile() {
    let namespace = NamespaceId::from_raw(226);
    let window = XResourceId::new(0x226, 1);
    let surface = SurfaceId::new(226, 1);
    let mut runtime = XAuthorityRuntime::new();
    fallback_window(&mut runtime, namespace, window, surface, 280);

    // The journal has no pattern pixels either, so a tiled paint must refuse
    // replay rather than replay solid.
    runtime.begin_dispatch();
    runtime.apply_core_draw_with_gc(
        TransactionId::from_raw(281),
        namespace,
        window,
        Region::single(Rect {
            x: 1,
            y: 1,
            width: 4,
            height: 4,
        }),
        &XGraphicsContextValues {
            fill_style: 1,
            tile: Some(XResourceId::new(0x227, 1)),
            ..Default::default()
        },
    );

    assert_eq!(
        expect_raster_fallback(
            runtime
                .apply_surface_raster_requirements(
                    TransactionId::from_raw(282),
                    &fallback_requirement(surface, 2, &[750]),
                )
                .unwrap(),
            "a tile has no journal projection",
        ),
        XRasterFallbackCause::UnsupportedFillPattern,
    );
}

#[test]
fn fallback_causes_have_distinct_stable_log_tokens() {
    let causes = [
        XRasterFallbackCause::UnsupportedPutImage,
        XRasterFallbackCause::UnsupportedCrossDrawableCopy,
        XRasterFallbackCause::UnsupportedClipMask,
        XRasterFallbackCause::UnsupportedFillPattern,
        XRasterFallbackCause::UnsupportedCommand,
        XRasterFallbackCause::StaleContentGeneration,
        XRasterFallbackCause::JournalCapacity,
        XRasterFallbackCause::BackingCapacity,
        XRasterFallbackCause::TransformMismatch,
    ];
    let tokens: std::collections::BTreeSet<_> =
        causes.iter().map(|cause| cause.as_str()).collect();
    assert_eq!(tokens.len(), causes.len());
    assert_eq!(
        XRasterFallbackCause::UnsupportedPutImage.as_str(),
        "unsupported_put_image"
    );
}

#[test]
fn fallback_coalescer_emits_first_and_power_of_two_cumulative_counts() {
    let mut coalescer = XRasterFallbackCoalescer::default();
    let surface = SurfaceId::new(226, 1);
    let cause = XRasterFallbackCause::UnsupportedPutImage;

    let emitted: Vec<_> = (0..10)
        .filter_map(|_| coalescer.observe(surface, cause))
        .collect();

    assert_eq!(emitted, vec![1, 2, 4, 8]);
    // Suppressed occurrences are still counted, never discarded.
    assert_eq!(coalescer.occurrences(surface, cause), 10);
}

#[test]
fn fallback_coalescer_separates_surfaces_and_causes() {
    let mut coalescer = XRasterFallbackCoalescer::default();
    let first = SurfaceId::new(227, 1);
    let second = SurfaceId::new(228, 1);

    assert_eq!(
        coalescer.observe(first, XRasterFallbackCause::UnsupportedPutImage),
        Some(1)
    );
    assert_eq!(
        coalescer.observe(first, XRasterFallbackCause::JournalCapacity),
        Some(1),
        "a second cause on the same surface is its own series"
    );
    assert_eq!(
        coalescer.observe(second, XRasterFallbackCause::UnsupportedPutImage),
        Some(1),
        "a second surface is its own series"
    );
    assert_eq!(
        coalescer.occurrences(first, XRasterFallbackCause::UnsupportedCommand),
        0
    );
}

#[test]
fn a_lagging_requirement_is_answered_from_current_state() {
    let namespace = NamespaceId::from_raw(229);
    let window = XResourceId::new(0x229, 1);
    let surface = SurfaceId::new(229, 1);
    let mut runtime = XAuthorityRuntime::new();
    fallback_window(&mut runtime, namespace, window, surface, 280);

    // Reproduces the live condition: Engine builds a requirement from the scene
    // it last committed, while the client keeps drawing. The authority advances
    // its generation per draw, so by arrival it sits well ahead of the request.
    let committed_by_engine = 2;
    for index in 0..5 {
        runtime.begin_dispatch();
        runtime.apply_core_draw(
            TransactionId::from_raw(281 + index),
            namespace,
            window,
            Region::single(Rect {
                x: 1,
                y: 1,
                width: 4,
                height: 4,
            }),
        );
    }

    let response = expect_satisfied_raster(
        runtime
            .apply_surface_raster_requirements(
                TransactionId::from_raw(290),
                &fallback_requirement(surface, committed_by_engine, &[750]),
            )
            .unwrap(),
        "a requirement is advisory demand, so a lagging one is still answerable",
    );

    // The reply describes the pixels it actually carries rather than echoing
    // the request, and anchors at that same generation so it commits when
    // Engine's ordered chain arrives there.
    assert!(
        response.identity.source_content_generation > committed_by_engine,
        "the reply must report the generation it was produced from"
    );
    assert_eq!(
        response.transaction.previous_committed_generation,
        response.identity.source_content_generation,
    );
    assert_eq!(response.transaction.content.variants().len(), 2);
    assert!(
        response
            .transaction
            .content
            .variants()
            .iter()
            .all(|variant| variant.fidelity
                == sophia_protocol::SurfaceContentFidelity::AuthorityRaster)
    );
}

#[test]
fn a_requirement_ahead_of_the_authority_still_fails_closed() {
    let namespace = NamespaceId::from_raw(231);
    let window = XResourceId::new(0x232, 1);
    let surface = SurfaceId::new(231, 1);
    let mut runtime = XAuthorityRuntime::new();
    fallback_window(&mut runtime, namespace, window, surface, 310);

    runtime.begin_dispatch();
    runtime.apply_core_draw(
        TransactionId::from_raw(311),
        namespace,
        window,
        Region::single(Rect {
            x: 1,
            y: 1,
            width: 4,
            height: 4,
        }),
    );

    // Naming content the authority has never produced is the genuine error the
    // relaxed admission still refuses.
    let (cause, observed) = expect_raster_fallback_detail(
        runtime
            .apply_surface_raster_requirements(
                TransactionId::from_raw(312),
                &fallback_requirement(surface, 500, &[750]),
            )
            .unwrap(),
        "a requirement ahead of the authority names content that does not exist",
    );
    assert_eq!(cause, XRasterFallbackCause::StaleContentGeneration);
    assert!(observed < 500);
}

#[test]
fn a_requirement_naming_the_wrong_extent_is_distinguished_from_a_stale_generation() {
    let namespace = NamespaceId::from_raw(230);
    let window = XResourceId::new(0x231, 1);
    let surface = SurfaceId::new(230, 1);
    let mut runtime = XAuthorityRuntime::new();
    fallback_window(&mut runtime, namespace, window, surface, 300);

    runtime.begin_dispatch();
    runtime.apply_core_draw(
        TransactionId::from_raw(301),
        namespace,
        window,
        Region::single(Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 20,
        }),
    );

    let mut requirements = fallback_requirement(surface, 2, &[750]);
    requirements.logical_extent = Size {
        width: 41,
        height: 20,
    };
    assert_eq!(
        expect_raster_fallback(
            runtime
                .apply_surface_raster_requirements(TransactionId::from_raw(302), &requirements)
                .unwrap(),
            "an extent disagreement must not be reported as a stale generation",
        ),
        XRasterFallbackCause::LogicalExtentMismatch,
    );
}

#[test]
fn the_first_satisfied_requirement_reports_without_a_prior_failure() {
    let mut coalescer = XRasterFallbackCoalescer::default();
    let surface = SurfaceId::new(240, 1);

    // A density path that works from the outset must still say so once.
    // Reporting only recoveries left a physical run with no evidence that
    // replies had started being produced at all.
    assert_eq!(coalescer.observe_satisfied(surface), Some(0));
    assert_eq!(
        coalescer.observe_satisfied(surface),
        None,
        "steady success stays silent after the first report"
    );
}

#[test]
fn a_recovery_still_reports_the_failures_it_ended() {
    let mut coalescer = XRasterFallbackCoalescer::default();
    let surface = SurfaceId::new(241, 1);
    let cause = XRasterFallbackCause::UnsupportedPutImage;

    for _ in 0..3 {
        coalescer.observe(surface, cause);
    }
    assert_eq!(coalescer.observe_satisfied(surface), Some(3));
}

#[test]
fn a_surface_with_no_canonical_raster_falls_back_instead_of_failing() {
    let namespace = NamespaceId::from_raw(232);
    let window = XResourceId::new(0x233, 1);
    let surface = SurfaceId::new(232, 1);
    let mut runtime = XAuthorityRuntime::new();
    // A window whose pixels only ever arrive through a renderer present has no
    // CPU drawable at all. This is the shape that killed the mixed-output gate:
    // the requirement returned an error, the error left the connection loop,
    // and one surface's demand ended the whole X server.
    fallback_window(&mut runtime, namespace, window, surface, 320);

    let outcome = runtime
        .apply_surface_raster_requirements(
            TransactionId::from_raw(321),
            &fallback_requirement(surface, 1, &[750]),
        )
        .expect("an unanswerable requirement must never be a runtime error");
    let (cause, _) = expect_raster_fallback_detail(
        outcome,
        "a surface with no canonical drawable cannot be rasterized",
    );
    assert_eq!(cause, XRasterFallbackCause::NoCanonicalRaster);
}

/// A live density variant is patched by later drawing, not resent.
///
/// A derived variant used to publish its whole buffer for every drawing
/// command, once per retained density: a one-glyph edit on a window with two
/// variants copied two whole buffers to carry a few hundred bytes of change.
/// The replay now reports where it painted and the variant publishes that
/// region.
///
/// The check is equivalence. Replaying the published updates has to leave the
/// same pixels a full replacement would, because the failure this risks is a
/// variant stale in one place, at the right generation, in a frame that is
/// otherwise presentable. `apply_command` computes the extent beside each
/// branch's own projection for exactly that reason.
#[test]
fn a_live_density_variant_publishes_patches_that_replay_like_replacements() {
    let namespace = NamespaceId::from_raw(232);
    let window = XResourceId::new(0x232, 1);
    let surface = SurfaceId::new(232, 1);
    let mut runtime = XAuthorityRuntime::new();
    fallback_window(&mut runtime, namespace, window, surface, 400);

    // Establish a replayable journal, then materialize a 0.75x variant.
    runtime.begin_dispatch();
    runtime.apply_put_image(
        TransactionId::from_raw(401),
        namespace,
        window,
        Region::single(Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 20,
        }),
        Some(&opaque_image(40, 20, 0x0011_2233)),
        Some(&z_pixmap_semantics()),
    );
    // Materializing the variant publishes a baseline replacement in the
    // requirement response; keep it as the base later updates replay against.
    let mut replayed = std::collections::BTreeMap::new();
    for update in runtime.take_cpu_buffer_updates() {
        update.apply_to(&mut replayed).unwrap();
    }
    let outcome = runtime
        .apply_surface_raster_requirements(
            TransactionId::from_raw(402),
            &fallback_requirement(surface, 2, &[750]),
        )
        .unwrap();
    let XSurfaceRasterOutcome::Satisfied(response) = outcome else {
        panic!("the 0.75 class must replay from the journal: {outcome:?}");
    };
    let [XAuthorityCpuBufferUpdate::Replace(derived)] = response.cpu_buffer_updates.as_slice()
    else {
        panic!("materializing a variant publishes one derived replacement");
    };
    let variant_handle = derived.handle;
    for update in &response.cpu_buffer_updates {
        update.apply_to(&mut replayed).unwrap();
    }

    // Draw again. The variant owes only the region the replay painted.
    runtime.begin_dispatch();
    runtime.apply_core_draw_with_gc(
        TransactionId::from_raw(403),
        namespace,
        window,
        Region::single(Rect {
            x: 4,
            y: 4,
            width: 6,
            height: 6,
        }),
        &XGraphicsContextValues {
            foreground: 0x0000_ff00,
            ..XGraphicsContextValues::default()
        },
    );
    let updates = runtime.take_cpu_buffer_updates();
    let variant_updates: Vec<_> = updates
        .iter()
        .filter(|update| update.handle() == variant_handle)
        .collect();
    assert!(
        !variant_updates.is_empty(),
        "the live variant must publish something for a drawing command"
    );
    assert!(
        variant_updates
            .iter()
            .all(|update| !update.is_replacement()),
        "a live variant must patch rather than resend its whole buffer: {variant_updates:?}"
    );
    for update in &updates {
        update.apply_to(&mut replayed).unwrap();
    }

    // The painted region arrived, and nothing outside it moved.
    let variant = replayed.get(&variant_handle).expect("replayed variant");
    let stride = usize::try_from(variant.stride).unwrap();
    let pixel_at = |x: usize, y: usize| -> u32 {
        let offset = y * stride + x * 4;
        u32::from_le_bytes(variant.bytes[offset..offset + 4].try_into().unwrap())
    };
    // The 1x rectangle (4,4,6,6) projects to (3,3,5,5) at 0.75x. Every pixel
    // of it is checked, not a sample: a cover short by one column leaves
    // exactly one edge stale, which is the shape this is guarding against.
    for y in 3..8usize {
        for x in 3..8usize {
            assert_eq!(
                pixel_at(x, y) & 0x00ff_ffff,
                0x0000_ff00,
                "({x},{y}) is inside the replayed draw and was not carried"
            );
        }
    }
    assert_eq!(
        pixel_at(25, 13) & 0x00ff_ffff,
        0x0011_2233,
        "a pixel outside the draw must still hold the uploaded image"
    );
}
