//! The session lock cover through the production head planner: a locked
//! output samples no client, draws only the cover, never scans a client out
//! directly, and proves which lock it drew from what each head retired.

use sophia_engine::*;
use sophia_protocol::*;

const OUTPUT: OutputId = OutputId::from_raw(1);
const FILL: CompositorRgb8 = CompositorRgb8 {
    red: 0x10,
    green: 0x20,
    blue: 0x30,
};

fn viewport() -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: 2560,
        height: 1440,
    }
}

fn cover(epoch: u64) -> SessionLockCover {
    SessionLockCover::fill(SessionLockEpoch::from_raw(epoch).unwrap(), FILL)
}

/// A fullscreen client that would scan out directly if it were drawn.
fn fullscreen_client() -> CommittedSurfaceState {
    let full = viewport();
    CommittedSurfaceState {
        surface: SurfaceId::new(4, 1),
        committed_generation: 8,
        geometry: full,
        content: SurfaceContentSet::new(
            Size {
                width: full.width,
                height: full.height,
            },
            vec![SurfaceContentVariant {
                variant: 1,
                source: BufferSource::DmaBuf { handle: 77 },
                pixel_size: Size {
                    width: full.width,
                    height: full.height,
                },
                density_millis: 1_000,
                transform: SurfaceRasterTransform::Normal,
                fidelity: SurfaceContentFidelity::AuthorityRaster,
                damage: Region::single(full),
            }],
        )
        .unwrap(),
        damage: Region::single(full),
    }
}

fn target(head: u64, width: i32, height: i32, mapping: OutputHeadMapping) -> HeadRenderTarget {
    HeadRenderTarget {
        head: RenderHeadId::from_raw(head),
        output: OUTPUT,
        target_generation: 7,
        native_size: Size { width, height },
        scale: 1,
        refresh_millihz: 60_000,
        transform: OutputTransform::Normal,
        mapping,
    }
}

/// Two mirror heads of unequal size, so one of them is letterboxed.
fn heads() -> [HeadRenderTarget; 2] {
    [
        target(1, 2560, 1440, OutputHeadMapping::Fit),
        target(2, 1920, 1200, OutputHeadMapping::Fit),
    ]
}

fn locked_plans(epoch: u64) -> Vec<HeadCompositionPlan> {
    let snapshot = output_scene_snapshot_from_committed_in_view(
        OUTPUT,
        12,
        viewport(),
        &[fullscreen_client()],
        cover(epoch).display_list(OUTPUT, viewport()),
        None,
    )
    .unwrap();
    build_output_head_plans(&snapshot, &heads()).unwrap()
}

fn retired(plans: &[HeadCompositionPlan]) -> Vec<OutputFrameDamageSnapshot> {
    plans.iter().map(head_output_damage_snapshot).collect()
}

fn proof(frames: &[OutputFrameDamageSnapshot]) -> Option<SessionLockEpoch> {
    presented_session_lock(OUTPUT, &frames.iter().map(Some).collect::<Vec<_>>())
}

#[test]
fn epochs_are_never_zero_and_refuse_at_exhaustion() {
    assert_eq!(SessionLockEpoch::from_raw(0), None);
    assert_eq!(SessionLockEpoch::FIRST.raw(), 1);
    assert_eq!(
        SessionLockEpoch::FIRST.next(),
        SessionLockEpoch::from_raw(2)
    );
    assert_eq!(SessionLockEpoch::from_raw(u64::MAX).unwrap().next(), None);
}

#[test]
fn a_locked_output_samples_no_client_and_draws_only_the_cover() {
    for plan in locked_plans(3) {
        assert!(
            plan.layers.is_empty(),
            "head {:?} bound a client",
            plan.head
        );
        let drawn = plan
            .compositor
            .iter()
            .filter(|command| !matches!(command, HeadCompositorCommand::Background(_)))
            .collect::<Vec<_>>();
        assert_eq!(drawn.len(), 1, "head {:?} drew {drawn:?}", plan.head);
        let HeadCompositorCommand::Rect(rect) = drawn[0] else {
            panic!("head {:?} drew {:?}, not the cover", plan.head, drawn[0]);
        };
        assert_eq!(
            rect.node,
            CompositorNodeId::SessionLock {
                output: OUTPUT,
                epoch: 3
            }
        );
        assert_eq!(rect.color, FILL);
        assert_eq!(rect.opacity, u8::MAX);
    }
}

#[test]
fn a_locked_fullscreen_client_is_never_scanned_out_directly() {
    // The same client and head, unlocked, is the direct-scanout case.
    let unlocked = output_scene_snapshot_from_committed_in_view(
        OUTPUT,
        12,
        viewport(),
        &[fullscreen_client()],
        CompositorDisplayList {
            output: OUTPUT,
            commands: vec![CompositorDisplayCommand::Surface {
                surface: SurfaceId::new(4, 1),
            }],
        },
        None,
    )
    .unwrap();
    let head = target(1, 2560, 1440, OutputHeadMapping::Fit);
    assert!(
        build_head_composition_plan(&unlocked, head)
            .unwrap()
            .direct_scanout
            .is_eligible()
    );

    for plan in locked_plans(3) {
        assert!(!plan.direct_scanout.is_eligible());
    }
}

#[test]
fn every_retired_head_proves_the_lock_it_drew() {
    assert_eq!(
        proof(&retired(&locked_plans(5))),
        SessionLockEpoch::from_raw(5)
    );
}

#[test]
fn a_head_still_showing_the_desktop_leaves_the_output_unproven() {
    let mut frames = retired(&locked_plans(5));
    let desktop = output_scene_snapshot_from_committed_in_view(
        OUTPUT,
        11,
        viewport(),
        &[fullscreen_client()],
        CompositorDisplayList {
            output: OUTPUT,
            commands: vec![CompositorDisplayCommand::Surface {
                surface: SurfaceId::new(4, 1),
            }],
        },
        None,
    )
    .unwrap();
    frames[1] =
        head_output_damage_snapshot(&build_head_composition_plan(&desktop, heads()[1]).unwrap());
    assert_eq!(proof(&frames), None);
}

#[test]
fn a_head_that_has_retired_nothing_leaves_the_output_unproven() {
    let frames = retired(&locked_plans(5));
    assert_eq!(
        presented_session_lock(OUTPUT, &[Some(&frames[0]), None]),
        None
    );
    assert_eq!(presented_session_lock(OUTPUT, &[]), None);
}

#[test]
fn heads_showing_different_locks_prove_neither() {
    let mut frames = retired(&locked_plans(5));
    frames[1] = retired(&locked_plans(6)).remove(1);
    assert_eq!(proof(&frames), None);
}

#[test]
fn anything_drawn_beside_the_cover_voids_the_proof() {
    let mut frames = retired(&locked_plans(5));
    frames[0]
        .compositor_display_list
        .commands
        .push(CompositorDisplayCommand::Rect(CompositorRect {
            opacity: u8::MAX,
            node: CompositorNodeId::IndicatorStrip { output: OUTPUT },
            generation: 1,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            color: FILL,
        }));
    assert_eq!(proof(&frames), None);
}

#[test]
fn a_cover_drawn_for_another_output_does_not_prove_this_one() {
    let frames = retired(&locked_plans(5));
    assert_eq!(
        presented_session_lock(
            OutputId::from_raw(2),
            &frames.iter().map(Some).collect::<Vec<_>>()
        ),
        None
    );
}

#[test]
fn a_client_drawn_beside_the_cover_voids_the_proof() {
    // A head whose frame carries the cover and also a client: the cover is
    // there, but so is something it should have hidden.
    let desktop = output_scene_snapshot_from_committed_in_view(
        OUTPUT,
        11,
        viewport(),
        &[fullscreen_client()],
        CompositorDisplayList {
            output: OUTPUT,
            commands: vec![CompositorDisplayCommand::Surface {
                surface: SurfaceId::new(4, 1),
            }],
        },
        None,
    )
    .unwrap();
    let client =
        head_output_damage_snapshot(&build_head_composition_plan(&desktop, heads()[0]).unwrap());
    assert!(!client.surfaces.is_empty());

    let mut frames = retired(&locked_plans(5));
    frames[0].surfaces = client.surfaces.clone();
    assert_eq!(proof(&frames), None, "a client surface beside the cover");

    let mut frames = retired(&locked_plans(5));
    frames[0]
        .compositor_display_list
        .commands
        .extend(client.compositor_display_list.commands.iter().cloned());
    assert_eq!(proof(&frames), None, "a client draw beside the cover");
}

fn image(output: OutputId, generation: u64) -> SessionLockImagePlacement {
    SessionLockImagePlacement {
        image: SessionLockImage {
            identity: SessionLockImageIdentity {
                output,
                connection_epoch: 9,
                resource_id: 4,
                resource_generation: generation,
            },
            width_px: 2560,
            height_px: 1440,
            pixels: std::sync::Arc::from(vec![0u8; 2560 * 1440 * 4]),
        },
        generation,
    }
}

fn cover_with(epoch: u64, images: &[(OutputId, SessionLockImagePlacement)]) -> SessionLockCover {
    SessionLockCover {
        images: std::sync::Arc::new(images.iter().cloned().collect()),
        ..cover(epoch)
    }
}

fn plans_with(cover: &SessionLockCover) -> Vec<HeadCompositionPlan> {
    let snapshot = output_scene_snapshot_from_committed_in_view(
        OUTPUT,
        12,
        viewport(),
        &[fullscreen_client()],
        cover.display_list(OUTPUT, viewport()),
        None,
    )
    .unwrap();
    build_output_head_plans(&snapshot, &heads()).unwrap()
}

#[test]
fn every_mirror_head_draws_the_provider_image_over_the_fill() {
    let cover = cover_with(3, &[(OUTPUT, image(OUTPUT, 1))]);
    for plan in plans_with(&cover) {
        assert!(
            plan.layers.is_empty(),
            "head {:?} bound a client",
            plan.head
        );
        let drawn = plan
            .compositor
            .iter()
            .filter(|command| !matches!(command, HeadCompositorCommand::Background(_)))
            .collect::<Vec<_>>();
        assert_eq!(drawn.len(), 2, "head {:?} drew {drawn:?}", plan.head);
        assert!(
            matches!(drawn[0], HeadCompositorCommand::Rect(_)),
            "the fill first"
        );
        let HeadCompositorCommand::ContentImage(content) = drawn[1] else {
            panic!("head {:?} drew {:?}", plan.head, drawn[1]);
        };
        assert!(matches!(
            content.image.resource,
            CompositorImageSource::Lock(_)
        ));
        assert!(
            !content.geometry.is_empty(),
            "scaled onto head {:?}",
            plan.head
        );
    }
}

#[test]
fn the_proof_holds_with_the_image_and_reads_what_every_head_shows() {
    let cover = cover_with(3, &[(OUTPUT, image(OUTPUT, 6))]);
    let frames = retired(&plans_with(&cover));
    assert_eq!(proof(&frames), SessionLockEpoch::from_raw(3));
    let shown = presented_session_lock_image(OUTPUT, &frames.iter().map(Some).collect::<Vec<_>>());
    assert_eq!(
        shown.map(|(identity, generation)| (identity.resource_generation, generation)),
        Some((6, 6))
    );
    // Heads showing different images agree on the lock, not on the image.
    let mut mixed = retired(&plans_with(&cover));
    mixed[1] = retired(&plans_with(&cover_with(3, &[(OUTPUT, image(OUTPUT, 7))])))[1].clone();
    assert_eq!(proof(&mixed), SessionLockEpoch::from_raw(3));
    assert_eq!(
        presented_session_lock_image(OUTPUT, &mixed.iter().map(Some).collect::<Vec<_>>()),
        None
    );
    // A fill-only lock shows no image.
    let plain = retired(&locked_plans(3));
    assert_eq!(
        presented_session_lock_image(OUTPUT, &plain.iter().map(Some).collect::<Vec<_>>()),
        None
    );
}

#[test]
fn an_image_for_another_output_is_never_drawn_here() {
    let other = OutputId::from_raw(2);
    // Keyed to this output but naming another, or keyed to another.
    for images in [
        vec![(OUTPUT, image(other, 1))],
        vec![(other, image(other, 1))],
    ] {
        let cover = cover_with(3, &images);
        let list = cover.display_list(OUTPUT, viewport());
        assert_eq!(list.commands.len(), 1, "the fill alone");
    }
}

#[test]
fn a_shell_image_beside_the_cover_voids_the_proof() {
    let cover = cover_with(3, &[(OUTPUT, image(OUTPUT, 1))]);
    let mut frames = retired(&plans_with(&cover));
    let CompositorDisplayCommand::ContentImage(lock_image) =
        frames[0].compositor_display_list.commands[1].clone()
    else {
        panic!("the image");
    };
    let mut shell = lock_image;
    shell.resource = CompositorImageSourceIdentity::Shell(ContentResourceBegin {
        grant: ContentGrant {
            connection_epoch: 1,
            content_grant_epoch: 1,
        },
        resource: ContentResourceId {
            id: 1,
            generation: 1,
        },
        width_px: 1,
        height_px: 1,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: 1,
        total_bytes: 4,
    });
    frames[0]
        .compositor_display_list
        .commands
        .push(CompositorDisplayCommand::ContentImage(shell));
    assert_eq!(proof(&frames), None);
}
