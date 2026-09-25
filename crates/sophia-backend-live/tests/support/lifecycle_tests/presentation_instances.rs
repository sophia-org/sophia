//! t244: WM surface instances, regions and the presentation tier through the
//! production runtime, the lowered owners and the mirrored native target.
use super::*;
use std::sync::Arc;

#[path = "presentation_input.rs"]
mod presentation_input;

fn commit_cpu_surface(
    runtime: &mut LiveProductionVisualRuntime,
    scene: &mut LiveProductionCpuScene,
    surface: SurfaceId,
    handle: u64,
    generation: u64,
    geometry: Rect,
) {
    let size = Size {
        width: geometry.width,
        height: geometry.height,
    };
    let byte_len = usize::try_from(size.width * size.height * 4).unwrap();
    scene
        .apply_updates([LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
            handle,
            generation,
            size,
            stride: u32::try_from(size.width * 4).unwrap(),
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            bytes: Arc::new(vec![(generation as u8).wrapping_mul(40); byte_len]),
        })])
        .unwrap();
    let transaction = SurfaceTransaction {
        transaction: TransactionId::from_raw(handle * 1_000 + generation),
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: geometry,
        presentation_extent: size,
        content: SurfaceContentSet::singleton(BufferSource::CpuBuffer { handle }, size),
        damage: Region::single(Rect {
            x: 0,
            y: 0,
            width: size.width,
            height: size.height,
        }),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: generation - 1,
        input_region: None,
    };
    runtime
        .prepare_authority_transactions(transaction.transaction, &[transaction], &[])
        .unwrap();
}

fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn presentation_output(output: OutputId, mode: PolicyPresentationMode) -> PolicyPresentationOutput {
    PolicyPresentationOutput {
        output,
        generation: 6,
        coverage: rect(0, 0, 64, 32),
        mode,
    }
}

fn region(
    output: OutputId,
    id: u64,
    z_index: u16,
    role: PolicyPresentationRegionRole,
    geometry: Rect,
    clip: Rect,
) -> PolicyPresentationRegion {
    PolicyPresentationRegion {
        id,
        generation: 2,
        output,
        geometry,
        clip,
        z_index,
        role,
        action: None,
    }
}

fn shown_instance(
    output: OutputId,
    id: u64,
    z_index: u16,
    source: SurfaceId,
    destination: Rect,
) -> PolicySurfaceInstance {
    PolicySurfaceInstance {
        id,
        generation: 3,
        output,
        source,
        destination,
        clip: destination,
        opacity_millis: 1_000,
        z_index,
        action: None,
    }
}

fn published(
    generation: u64,
    outputs: Vec<PolicyPresentationOutput>,
    instances: Vec<PolicySurfaceInstance>,
    regions: Vec<PolicyPresentationRegion>,
) -> LivePolicyPresentation {
    LivePolicyPresentation {
        owner_epoch: 41,
        presentation: PolicyPresentation {
            generation,
            keyboard_output: None,
            outputs,
            instances,
            regions,
            bindings: vec![],
        },
    }
}

fn output_list(runtime: &LiveProductionVisualRuntime, output: OutputId) -> CompositorDisplayList {
    output_composition::OutputCompositionSnapshot::capture(runtime)
        .display_list(output, runtime.committed_surfaces())
        .unwrap()
}

/// Regions and instances share one z order per output, under one stamp,
/// in Engine's palette and within each region's clipped allocation; an
/// output without an output record draws nothing of the publication.
#[test]
fn regions_and_instances_share_one_z_order_under_one_stamp() {
    let outputs = outputs();
    let output = outputs[0].id;
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let source = SurfaceId::new(5, 1);
    commit_cpu_surface(
        &mut runtime,
        &mut scene,
        source,
        55,
        1,
        rect(-100, -100, 8, 8),
    );
    let style = runtime.surface_chrome_style;
    runtime
        .set_policy_presentation(
            Some(published(
                1,
                vec![presentation_output(output, PolicyPresentationMode::Overlay)],
                vec![shown_instance(output, 9, 2, source, rect(4, 4, 8, 8))],
                vec![
                    region(
                        output,
                        3,
                        3,
                        PolicyPresentationRegionRole::Emphasis,
                        rect(2, 2, 12, 12),
                        rect(2, 2, 12, 12),
                    ),
                    region(
                        output,
                        1,
                        0,
                        PolicyPresentationRegionRole::Backdrop,
                        rect(0, 0, 64, 32),
                        rect(0, 0, 64, 32),
                    ),
                    region(
                        output,
                        2,
                        1,
                        PolicyPresentationRegionRole::Frame,
                        rect(20, 2, 40, 40),
                        rect(0, 0, 64, 20),
                    ),
                ],
            )),
            &scene,
            None,
        )
        .unwrap();
    let list = output_list(&runtime, output);
    let stamp = list.presentation_stamp().unwrap();
    assert_eq!(
        (
            stamp.owner_epoch,
            stamp.publication_generation,
            stamp.output,
            stamp.output_generation
        ),
        (41, 1, output, 6)
    );
    let tier = list
        .commands
        .iter()
        .skip_while(|command| !matches!(command, CompositorDisplayCommand::PresentationStamp(_)))
        .skip(1)
        .collect::<Vec<_>>();
    let node = |id| CompositorNodeId::PolicyRegion {
        owner_epoch: 41,
        id,
    };
    assert!(
        matches!(tier[0], CompositorDisplayCommand::Rect(rect) if rect.node == node(1)
        && rect.opacity == u8::MAX && rect.color == style.frame.unfocused_color
        && rect.geometry == Rect { x: 0, y: 0, width: 64, height: 32 })
    );
    assert!(
        matches!(tier[1], CompositorDisplayCommand::Border(border) if border.node == node(2)
        && border.color == style.frame.unfocused_color
        && border.outer == Rect { x: 20, y: 2, width: 40, height: 18 }),
        "the frame is drawn within its clipped allocation"
    );
    assert!(
        matches!(tier[2], CompositorDisplayCommand::SurfaceInstance(instance) if instance.id == 9)
    );
    assert!(
        matches!(tier[3], CompositorDisplayCommand::Border(border) if border.node == node(3)
        && border.color == style.focus_ring.color)
    );
    assert_eq!(tier.len(), 4);
    let other = output_list(&runtime, outputs[1].id);
    assert!(other.presentation_stamp().is_none());
    assert!(other.surface_instances().next().is_none());
}

/// ReplaceApplications substitutes the tier for that output's ordinary
/// application presentation and hit targets; Overlay draws the tier above
/// it; withdrawal restores it. The client's allocation is untouched.
#[test]
fn replace_applications_substitutes_the_tier_for_that_outputs_applications_only() {
    let outputs = outputs();
    let output = outputs[0].id;
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let application = SurfaceId::new(7, 1);
    commit_cpu_surface(
        &mut runtime,
        &mut scene,
        application,
        77,
        1,
        rect(0, 0, 16, 16),
    );
    runtime.presentation_order = vec![application];
    runtime.surface_outputs.insert(application, output);
    runtime.chrome_surfaces = vec![application];
    let is_application = |command: &CompositorDisplayCommand| {
        matches!(command, CompositorDisplayCommand::Surface { surface } if *surface == application)
            || matches!(command, CompositorDisplayCommand::Border(border)
                if matches!(border.node, CompositorNodeId::SurfaceChrome { surface, .. } if surface == application))
    };
    let ordinary = output_list(&runtime, output);
    assert!(ordinary.commands.iter().any(is_application));
    let backdrop = region(
        output,
        1,
        0,
        PolicyPresentationRegionRole::Backdrop,
        rect(0, 0, 64, 32),
        rect(0, 0, 64, 32),
    );
    let preview = shown_instance(output, 2, 1, application, rect(30, 4, 8, 8));

    runtime
        .set_policy_presentation(
            Some(published(
                1,
                vec![presentation_output(output, PolicyPresentationMode::Overlay)],
                vec![preview],
                vec![backdrop],
            )),
            &scene,
            None,
        )
        .unwrap();
    let overlay = output_list(&runtime, output);
    let application_at = overlay.commands.iter().position(&is_application).unwrap();
    let stamp_at = overlay
        .commands
        .iter()
        .position(|command| matches!(command, CompositorDisplayCommand::PresentationStamp(_)))
        .unwrap();
    assert!(
        application_at < stamp_at,
        "the tier is above the applications it overlays"
    );

    runtime
        .set_policy_presentation(
            Some(published(
                2,
                vec![presentation_output(
                    output,
                    PolicyPresentationMode::ReplaceApplications,
                )],
                vec![preview],
                vec![backdrop],
            )),
            &scene,
            None,
        )
        .unwrap();
    let replaced = output_list(&runtime, output);
    assert!(
        !replaced.commands.iter().any(is_application),
        "no application draw on the replaced output"
    );
    assert_eq!(
        replaced.surface_instances().count(),
        1,
        "the application is still sampled by its preview"
    );
    let damage =
        output_frame_damage_snapshot(outputs[0], replaced, runtime.committed_surfaces(), None)
            .unwrap();
    assert!(
        damage.surfaces.is_empty(),
        "and it is no application hit target there"
    );
    assert_eq!(
        runtime.committed_surfaces()[0].geometry,
        rect(0, 0, 16, 16),
        "its allocation is untouched"
    );

    let replaced_frame = output_frame_damage_snapshot(
        outputs[0],
        output_list(&runtime, output),
        runtime.committed_surfaces(),
        None,
    )
    .unwrap();
    runtime.set_policy_presentation(None, &scene, None).unwrap();
    let restored = output_list(&runtime, output);
    assert!(
        restored.commands.iter().any(is_application),
        "withdrawal restores it"
    );
    // Withdrawal repaints what the tier covered and what it restores: the
    // stamp's coverage and the application's own placement.
    let restored_frame =
        output_frame_damage_snapshot(outputs[0], restored, runtime.committed_surfaces(), None)
            .unwrap();
    assert_eq!(
        restored_frame.surfaces.len(),
        1,
        "the application is a hit target again"
    );
    let damage = output_frame_damage(Some(&replaced_frame), &restored_frame).unwrap();
    assert!(
        damage.rects.contains(&rect(0, 0, 64, 32)),
        "the coverage is repainted"
    );
    assert!(
        damage.rects.contains(&rect(0, 0, 16, 16)),
        "the restored application is repainted"
    );
}

/// The frame that retires names the publication it presents, apart from
/// the one requested since; a source repaint keeps every presented identity.
#[test]
fn a_retired_frame_names_the_publication_it_presents_and_a_repaint_keeps_its_identities() {
    let outputs = outputs();
    let output = outputs[0].id;
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = MirroredTarget::new(&outputs);
    let source = SurfaceId::new(5, 1);
    commit_cpu_surface(
        &mut runtime,
        &mut scene,
        source,
        55,
        1,
        rect(-100, -100, 8, 8),
    );
    let backdrop = region(
        output,
        1,
        0,
        PolicyPresentationRegionRole::Backdrop,
        rect(0, 0, 64, 32),
        rect(0, 0, 64, 32),
    );
    let preview = shown_instance(output, 2, 1, source, rect(4, 4, 8, 8));
    runtime
        .set_policy_presentation(
            Some(published(
                1,
                vec![presentation_output(
                    output,
                    PolicyPresentationMode::ReplaceApplications,
                )],
                vec![preview],
                vec![backdrop],
            )),
            &scene,
            None,
        )
        .unwrap();
    let retire = |runtime: &LiveProductionVisualRuntime,
                  scene: &LiveProductionCpuScene,
                  target: &mut MirroredTarget| {
        let frames = runtime
            .retained_output_head_composition_frames(scene, &*target)
            .unwrap();
        target
            .queue_retained_batch(frames, &BTreeSet::new())
            .unwrap();
        target.install(output).unwrap();
        target.prepare(output);
        target.flip(output, 1);
        target.flip(output, 0);
        LivePresentedPolicyPublication::from_presented_frame(
            target.presented_frame(output).unwrap(),
        )
    };
    let first =
        retire(&runtime, &scene, &mut target).expect("the retired frame presents the publication");
    assert_eq!(
        first,
        LivePresentedPolicyPublication {
            owner_epoch: 41,
            generation: 1,
            output,
            output_generation: 6,
            instances: vec![(2, 3)],
            regions: vec![(1, 2)],
        }
    );
    commit_cpu_surface(
        &mut runtime,
        &mut scene,
        source,
        55,
        2,
        rect(-100, -100, 8, 8),
    );
    let repainted = retire(&runtime, &scene, &mut target).unwrap();
    assert_eq!(
        repainted, first,
        "a source repaint keeps every presented identity"
    );
    runtime
        .set_policy_presentation(
            Some(published(
                2,
                vec![presentation_output(
                    output,
                    PolicyPresentationMode::ReplaceApplications,
                )],
                vec![preview],
                vec![backdrop],
            )),
            &scene,
            None,
        )
        .unwrap();
    assert_eq!(
        runtime
            .policy_presentation()
            .unwrap()
            .presentation
            .generation,
        2
    );
    assert_eq!(
        LivePresentedPolicyPublication::from_presented_frame(
            target.presented_frame(output).unwrap()
        )
        .unwrap()
        .generation,
        1,
        "requested is not presented until a frame carrying it retires"
    );
    // A binding-only publication: identical pixels, yet the changed stamp
    // damages the coverage, so a frame carrying it is presented and retires.
    let generation_one = output_list(&runtime, output);
    let mut binding_only = runtime.policy_presentation().unwrap().clone();
    binding_only.presentation.generation = 3;
    binding_only.presentation.bindings = vec![PolicyPresentationBinding {
        action: WmActionId::from_raw(12),
        keycode: 9,
        modifiers: WmModifierMask { bits: 0 },
    }];
    runtime
        .set_policy_presentation(Some(binding_only), &scene, None)
        .unwrap();
    let generation_three = output_list(&runtime, output);
    let before = output_frame_damage_snapshot(
        outputs[0],
        generation_one,
        runtime.committed_surfaces(),
        None,
    )
    .unwrap();
    let after = output_frame_damage_snapshot(
        outputs[0],
        generation_three,
        runtime.committed_surfaces(),
        None,
    )
    .unwrap();
    assert_eq!(
        output_frame_damage(Some(&before), &after).unwrap().rects,
        vec![rect(0, 0, 64, 32)]
    );
    assert_eq!(retire(&runtime, &scene, &mut target).unwrap().generation, 3);
    // Withdrawal: once the frame replacing it retires, no stamp is presented.
    runtime.set_policy_presentation(None, &scene, None).unwrap();
    let withdrawn = output_frame_damage_snapshot(
        outputs[0],
        output_list(&runtime, output),
        runtime.committed_surfaces(),
        None,
    )
    .unwrap();
    assert!(
        !output_frame_damage(Some(&after), &withdrawn)
            .unwrap()
            .is_empty()
    );
    assert_eq!(retire(&runtime, &scene, &mut target), None);
    target.teardown();
}

/// t244: a WM presentation samples a source that is not presented anywhere,
/// twice. Both instances share one source lease and keep their own geometry
/// and damage; a content-only commit repaints them without changing their
/// interaction generation; neither becomes an application input layer; the
/// source stays owned through queued native heads and copies, and the
/// copied backings through submission, display and retirement.
#[test]
fn preview_only_instances_share_a_source_until_copy_and_backings_until_retirement() {
    let outputs = outputs();
    let output = outputs[0];
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(output.size);
    let mut target = MirroredTarget::new(&outputs);
    let surface = SurfaceId::new(5, 1);
    let source_size = Size {
        width: 8,
        height: 8,
    };
    let source_geometry = Rect {
        x: -100,
        y: -100,
        width: 8,
        height: 8,
    };
    let first = Rect {
        x: 4,
        y: 4,
        width: 4,
        height: 4,
    };
    let second = Rect {
        x: 20,
        y: 4,
        width: 8,
        height: 8,
    };
    let instance = |id: u64, destination: Rect| PolicySurfaceInstance {
        id,
        generation: 3,
        output: output.id,
        source: surface,
        destination,
        clip: destination,
        opacity_millis: 1_000,
        z_index: u16::try_from(id).unwrap(),
        action: None,
    };
    let mut before = None;
    let mut source_weak = None;
    for generation in 1..=2 {
        let bytes = Arc::new(vec![generation as u8 * 40; 8 * 8 * 4]);
        source_weak = Some(Arc::downgrade(&bytes));
        scene
            .apply_updates([LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
                handle: 55,
                generation,
                size: source_size,
                stride: 32,
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                bytes,
            })])
            .unwrap();
        let transaction = SurfaceTransaction {
            transaction: TransactionId::from_raw(generation),
            authority: AuthorityKind::SophiaX,
            surface,
            namespace: None,
            target_geometry: source_geometry,
            presentation_extent: source_size,
            content: SurfaceContentSet::singleton(
                BufferSource::CpuBuffer { handle: 55 },
                source_size,
            ),
            damage: Region::single(Rect {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            }),
            readiness: SurfaceTransactionReadiness::Ready,
            timeout_msec: 250,
            previous_committed_generation: generation - 1,
            input_region: None,
        };
        runtime
            .prepare_authority_transactions(transaction.transaction, &[transaction], &[])
            .unwrap();
        // Admitted once its source has committed content.
        if generation == 1 {
            runtime
                .set_policy_presentation(
                    Some(LivePolicyPresentation {
                        owner_epoch: 41,
                        presentation: PolicyPresentation {
                            generation: 1,
                            keyboard_output: None,
                            outputs: vec![PolicyPresentationOutput {
                                output: output.id,
                                generation: 1,
                                coverage: Rect {
                                    x: 0,
                                    y: 0,
                                    width: 64,
                                    height: 32,
                                },
                                mode: PolicyPresentationMode::Overlay,
                            }],
                            instances: vec![instance(1, first), instance(2, second)],
                            regions: vec![],
                            bindings: vec![],
                        },
                    }),
                    &scene,
                    None,
                )
                .unwrap();
        }
        assert!(
            runtime.presentation_order.is_empty(),
            "the source is not presented"
        );
        let committed = runtime.committed_surfaces();
        assert_eq!(
            committed[0].geometry, source_geometry,
            "its allocation is untouched"
        );
        let captured = output_composition::OutputCompositionSnapshot::capture(&runtime);
        let list = captured.display_list(output.id, committed).unwrap();
        assert!(
            !list
                .commands
                .iter()
                .any(|command| matches!(command, CompositorDisplayCommand::Surface { .. }))
        );
        let instances = list.surface_instances().collect::<Vec<_>>();
        assert_eq!(instances.len(), 2);
        assert!(instances.iter().all(|instance| instance.generation == 3
            && instance.source_generation == generation
            && instance.owner_epoch == 41));
        let damage = output_frame_damage_snapshot(output, list.clone(), committed, None).unwrap();
        assert!(damage.surfaces.is_empty(), "no application input layer");
        assert!(
            output_frame_damage(Some(&damage), &damage)
                .unwrap()
                .is_empty()
        );
        if let Some(before) = &before {
            let changed = output_frame_damage(Some(before), &damage).unwrap();
            assert!(!changed.is_empty());
            assert!(
                changed
                    .rects
                    .iter()
                    .all(|rect| *rect == first || *rect == second)
            );
        }
        let report = scene
            .compose_display_list(output, committed, &list, None)
            .unwrap();
        let value = generation as u8 * 40;
        for (x, y) in [(4, 4), (7, 7), (20, 4), (27, 11)] {
            let pixel = (y * 64 + x) * 4;
            // XRGB: colour only; the padding byte is copied as a layer's is.
            assert_eq!(
                &report.frame.bytes[pixel..pixel + 3],
                &[value, value, value]
            );
        }
        assert_eq!(&report.frame.bytes[..4], &[0; 4]);
        before = Some(damage);
    }
    let source_weak = source_weak.unwrap();
    // Production source lookup and native lowering, including a source that
    // only instances reference: one lease, two drawn layers.
    let frames = runtime
        .retained_output_head_composition_frames(&scene, &target)
        .unwrap();
    let head = &frames[0].1[0].frame;
    assert_eq!(
        head.layers
            .iter()
            .filter(|layer| matches!(layer, LiveOwnedMixedCompositionLayer::Cpu { .. }))
            .count(),
        2
    );
    assert!(
        head.output_damage_snapshot
            .as_ref()
            .unwrap()
            .surfaces
            .is_empty(),
        "the retired head frame publishes no application input layer"
    );
    target
        .queue_retained_batch(frames, &BTreeSet::new())
        .unwrap();
    runtime.set_policy_presentation(None, &scene, None).unwrap();
    let closed = output_composition::OutputCompositionSnapshot::capture(&runtime)
        .display_list(output.id, runtime.committed_surfaces())
        .unwrap();
    assert!(closed.surface_instances().next().is_none());
    scene.reconcile_buffer_residency(&[]);
    drop(runtime);
    drop(scene);
    assert!(
        source_weak.upgrade().is_some(),
        "queued native heads own the source after close"
    );
    target.install(output.id).unwrap();
    assert!(
        source_weak.upgrade().is_some(),
        "installed copy requests still own the source"
    );
    target.prepare(output.id);
    assert!(
        source_weak.upgrade().is_none(),
        "completed copies release the source lease"
    );
    assert_eq!(
        target.owners.get(),
        2,
        "submitted scanout backings remain owned"
    );
    target.flip(output.id, 1);
    assert!(target.heads[0].custody.submitted().is_some());
    assert_eq!(target.owners.get(), 2);
    target.flip(output.id, 0);
    assert_eq!(
        target.owners.get(),
        2,
        "the displayed backing remains owned until retirement"
    );
    target.teardown();
    assert_eq!(target.owners.get(), 0);
}

/// t244: a presentation naming a source with no committed content is refused
/// whole, and the last valid presentation stays drawn; a source commit
/// changes what an instance samples, never its interaction generation;
/// removing a sampled source revokes the whole presentation once, never one
/// instance of it.
#[test]
fn a_missing_source_refuses_the_presentation_whole_and_removal_revokes_it_whole() {
    let outputs = outputs();
    let output = outputs[0];
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(output.size);
    let shown = SurfaceId::new(5, 1);
    let absent = SurfaceId::new(6, 1);
    let size = Size {
        width: 8,
        height: 8,
    };
    let mut commit = |runtime: &mut LiveProductionVisualRuntime, generation: u64| {
        scene
            .apply_updates([LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
                handle: 55,
                generation,
                size,
                stride: 32,
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                bytes: Arc::new(vec![generation as u8; 8 * 8 * 4]),
            })])
            .unwrap();
        let transaction = SurfaceTransaction {
            transaction: TransactionId::from_raw(generation),
            authority: AuthorityKind::SophiaX,
            surface: shown,
            namespace: None,
            target_geometry: Rect {
                x: -100,
                y: -100,
                width: 8,
                height: 8,
            },
            presentation_extent: size,
            content: SurfaceContentSet::singleton(BufferSource::CpuBuffer { handle: 55 }, size),
            damage: Region::single(Rect {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            }),
            readiness: SurfaceTransactionReadiness::Ready,
            timeout_msec: 250,
            previous_committed_generation: generation - 1,
            input_region: None,
        };
        runtime
            .prepare_authority_transactions(transaction.transaction, &[transaction], &[])
            .unwrap();
    };
    commit(&mut runtime, 1);
    let instance = |id: u64, source: SurfaceId| PolicySurfaceInstance {
        id,
        generation: 4,
        output: output.id,
        source,
        destination: Rect {
            x: 4 + 12 * i32::try_from(id).unwrap(),
            y: 4,
            width: 8,
            height: 8,
        },
        clip: Rect {
            x: 4 + 12 * i32::try_from(id).unwrap(),
            y: 4,
            width: 8,
            height: 8,
        },
        opacity_millis: 1_000,
        z_index: u16::try_from(id).unwrap(),
        action: None,
    };
    let presentation =
        |generation: u64, instances: Vec<PolicySurfaceInstance>| LivePolicyPresentation {
            owner_epoch: 41,
            presentation: PolicyPresentation {
                generation,
                keyboard_output: None,
                outputs: vec![PolicyPresentationOutput {
                    output: output.id,
                    generation: 1,
                    coverage: Rect {
                        x: 0,
                        y: 0,
                        width: 64,
                        height: 32,
                    },
                    mode: PolicyPresentationMode::Overlay,
                }],
                instances,
                regions: vec![],
                bindings: vec![],
            },
        };
    let valid = presentation(1, vec![instance(1, shown)]);
    assert!(
        runtime
            .set_policy_presentation(Some(valid.clone()), &scene_for(&outputs), None)
            .unwrap()
    );
    let drawn = |runtime: &LiveProductionVisualRuntime| {
        output_composition::OutputCompositionSnapshot::capture(runtime)
            .display_list(output.id, runtime.committed_surfaces())
            .unwrap()
            .surface_instances()
            .collect::<Vec<_>>()
    };
    assert_eq!(drawn(&runtime).len(), 1);

    // Refused whole: the candidate's committed source does not save it.
    let refused = runtime
        .set_policy_presentation(
            Some(presentation(
                2,
                vec![instance(1, shown), instance(2, absent)],
            )),
            &scene_for(&outputs),
            None,
        )
        .unwrap_err();
    assert_eq!(
        refused.downcast_ref::<LivePolicyPresentationRefusal>(),
        Some(&LivePolicyPresentationRefusal::MissingSource { source: absent })
    );
    // The read-only check the session runs before preparing gives the same
    // answer and changes nothing.
    let candidate = presentation(2, vec![instance(1, shown), instance(2, absent)]);
    assert_eq!(
        runtime.validate_policy_presentation(&candidate),
        Err(LivePolicyPresentationRefusal::MissingSource { source: absent })
    );
    assert_eq!(runtime.validate_policy_presentation(&valid), Ok(()));
    assert_eq!(
        runtime.policy_presentation(),
        Some(&valid),
        "the last valid presentation stays"
    );
    let before = drawn(&runtime);
    assert_eq!(before.len(), 1);

    // A content-only commit: the sample moves, the interaction does not.
    commit(&mut runtime, 2);
    let after = drawn(&runtime);
    assert_eq!(after[0].generation, before[0].generation);
    assert_eq!(after[0].node(), before[0].node());
    assert_eq!(
        (before[0].source_generation, after[0].source_generation),
        (1, 2)
    );

    // Removal revokes the whole publication, and says so once.
    runtime
        .release_removed_presentations(&[shown], None)
        .unwrap();
    assert_eq!(runtime.policy_presentation(), None);
    assert_eq!(
        runtime.take_policy_presentation_revocation(),
        Some(LivePolicyPresentationRevocation {
            owner_epoch: 41,
            generation: 1,
            source: shown,
        })
    );
    assert_eq!(runtime.take_policy_presentation_revocation(), None);
    assert!(drawn(&runtime).is_empty());
}

fn scene_for(outputs: &[HeadlessOutput]) -> LiveProductionCpuScene {
    LiveProductionCpuScene::new(outputs[0].size)
}

/// An instance on one output may sample a source another output presents:
/// the source keeps its canonical placement and output, the second output
/// draws it as ever, and the first samples it under the same single lease.
#[test]
fn an_instance_samples_a_source_that_another_output_presents() {
    let outputs = outputs();
    let (first, second) = (outputs[0].id, outputs[1].id);
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let target = MirroredTarget::new(&outputs);
    let viewport = runtime.outputs.logical_viewport(second).unwrap();
    let placement = rect(viewport.x + 4, viewport.y + 4, 8, 8);
    let source = SurfaceId::new(8, 1);
    commit_cpu_surface(&mut runtime, &mut scene, source, 88, 1, placement);
    runtime.presentation_order = vec![source];
    runtime.surface_outputs.insert(source, second);
    runtime
        .set_policy_presentation(
            Some(published(
                1,
                vec![presentation_output(first, PolicyPresentationMode::Overlay)],
                vec![shown_instance(first, 2, 1, source, rect(20, 4, 16, 16))],
                vec![],
            )),
            &scene,
            None,
        )
        .unwrap();
    let on_first = output_list(&runtime, first);
    assert_eq!(on_first.surface_instances().count(), 1);
    assert!(
        !on_first
            .commands
            .iter()
            .any(|command| matches!(command, CompositorDisplayCommand::Surface { .. }))
    );
    let on_second = output_list(&runtime, second);
    assert!(on_second
        .commands
        .iter()
        .any(|command| matches!(command, CompositorDisplayCommand::Surface { surface } if *surface == source)));
    assert!(on_second.surface_instances().next().is_none());
    assert_eq!(
        runtime.surface_outputs.get(&source),
        Some(&second),
        "ownership unchanged"
    );
    assert_eq!(
        runtime.committed_surfaces()[0].geometry,
        placement,
        "placement unchanged"
    );
    let sources = runtime
        .retained_composition_source_set(&scene, None)
        .unwrap();
    let frames = runtime
        .retained_output_head_composition_frames_from_sources(&target, &sources)
        .unwrap();
    for (output, heads) in &frames {
        let cpu_layers = heads[0]
            .frame
            .layers
            .iter()
            .filter(|layer| matches!(layer, LiveOwnedMixedCompositionLayer::Cpu { .. }))
            .count();
        assert_eq!(cpu_layers, 1, "one draw of the source on {output:?}");
    }
    assert_eq!(frames.len(), 2);
}

/// A mirrored output has presented a publication only once every head has
/// retired a frame carrying it: a lagging head, still showing the previous
/// publication or none, leaves the output without a completed publication
/// whichever head flips first.
#[test]
fn a_mirrored_output_presents_a_publication_only_once_every_head_has() {
    let outputs = outputs();
    let output = outputs[0].id;
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = MirroredTarget::new(&outputs);
    assert_eq!(
        target.presented_head_frames(output).len(),
        2,
        "a mirrored output"
    );
    let source = SurfaceId::new(5, 1);
    commit_cpu_surface(
        &mut runtime,
        &mut scene,
        source,
        55,
        1,
        rect(-100, -100, 8, 8),
    );
    let backdrop = region(
        output,
        1,
        0,
        PolicyPresentationRegionRole::Backdrop,
        rect(0, 0, 64, 32),
        rect(0, 0, 64, 32),
    );
    let preview = shown_instance(output, 2, 1, source, rect(4, 4, 8, 8));
    let publish = |runtime: &mut LiveProductionVisualRuntime,
                   scene: &LiveProductionCpuScene,
                   generation: u64| {
        runtime
            .set_policy_presentation(
                Some(published(
                    generation,
                    vec![presentation_output(
                        output,
                        PolicyPresentationMode::ReplaceApplications,
                    )],
                    vec![preview],
                    vec![backdrop],
                )),
                scene,
                None,
            )
            .unwrap();
    };
    let queue = |runtime: &LiveProductionVisualRuntime,
                 scene: &LiveProductionCpuScene,
                 target: &mut MirroredTarget| {
        let frames = runtime
            .retained_output_head_composition_frames(scene, &*target)
            .unwrap();
        target
            .queue_retained_batch(frames, &BTreeSet::new())
            .unwrap();
        target.install(output).unwrap();
        target.prepare(output);
    };
    let whole = |target: &MirroredTarget| {
        LivePresentedPolicyPublication::from_presented_heads(&target.presented_head_frames(output))
            .map(|publication| publication.generation)
    };

    publish(&mut runtime, &scene, 1);
    queue(&runtime, &scene, &mut target);
    target.flip(output, 1);
    assert_eq!(
        whole(&target),
        None,
        "the primary head has not presented it"
    );
    target.flip(output, 0);
    assert_eq!(whole(&target), Some(1));

    publish(&mut runtime, &scene, 2);
    queue(&runtime, &scene, &mut target);
    target.flip(output, 0);
    assert_eq!(
        LivePresentedPolicyPublication::from_presented_frame(
            target.presented_frame(output).unwrap()
        )
        .unwrap()
        .generation,
        2,
        "the primary head shows the new publication"
    );
    assert_eq!(
        whole(&target),
        None,
        "the mirror head still shows the previous one"
    );
    target.flip(output, 1);
    assert_eq!(whole(&target), Some(2));
    target.teardown();
}

/// Every path that commits a removal revokes a presentation sampling the
/// removed source, not only the batch path: here the prepared commit.
#[test]
fn a_prepared_removal_of_a_sampled_source_revokes_the_presentation() {
    let outputs = outputs();
    let output = outputs[0].id;
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let source = SurfaceId::new(5, 1);
    commit_cpu_surface(
        &mut runtime,
        &mut scene,
        source,
        55,
        1,
        rect(-100, -100, 8, 8),
    );
    runtime
        .set_policy_presentation(
            Some(published(
                1,
                vec![presentation_output(output, PolicyPresentationMode::Overlay)],
                vec![shown_instance(output, 2, 1, source, rect(4, 4, 8, 8))],
                vec![],
            )),
            &scene,
            None,
        )
        .unwrap();
    runtime
        .prepare_authority_transactions(TransactionId::from_raw(99), &[], &[source])
        .unwrap();
    assert!(runtime.committed_surfaces().is_empty());
    assert_eq!(runtime.policy_presentation(), None);
    assert_eq!(
        runtime
            .take_policy_presentation_revocation()
            .map(|revocation| revocation.source),
        Some(source)
    );
    assert!(output_list(&runtime, output).presentation_stamp().is_none());
}
