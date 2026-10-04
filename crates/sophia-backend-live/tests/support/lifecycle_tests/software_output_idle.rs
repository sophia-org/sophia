//! Real software Present staging and native custody, with simulated device flips.
use super::presentation_instances::{
    commit_cpu_surface, presentation_output, published, rect, shown_instance,
};
use super::*;

struct Fixture {
    runtime: LiveProductionVisualRuntime,
    scene: LiveProductionCpuScene,
    target: Target,
    surface: SurfaceId,
}

impl Fixture {
    fn new() -> Self {
        let outputs = outputs();
        let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
        let mut scene = LiveProductionCpuScene::new(outputs[0].size);
        let surface = SurfaceId::new(41, 1);
        commit_cpu_surface(&mut runtime, &mut scene, surface, 41, 1, rect(0, 0, 16, 16));
        runtime.presentation_order = vec![surface];
        runtime.surface_outputs.insert(surface, outputs[0].id);
        Self {
            runtime,
            scene,
            target: Target::new(&outputs),
            surface,
        }
    }

    fn settle_initial_scene(&mut self) {
        assert!(
            self.runtime
                .queue_retained_projection(&self.scene, &mut self.target)
                .unwrap()
        );
        self.target.drain();
    }

    fn secondary_frames(&self) -> Vec<crate::LiveProductionHeadCompositionFrame> {
        self.runtime
            .retained_output_head_composition_frames(&self.scene, &self.target)
            .unwrap()
            .pop()
            .unwrap()
            .1
    }

    fn secondary_is_idle(&self, frames: &[crate::LiveProductionHeadCompositionFrame]) -> bool {
        self.runtime
            .software_present_output_is_idle(&self.target, outputs()[1].id, frames, &[])
    }

    fn expect_both(&mut self) {
        let frames = self.stage(100);
        assert_eq!(
            frames.len(),
            2,
            "output with unfinished work must join the cohort"
        );
        self.retire(&frames);
        self.target.teardown();
    }

    fn stage(&mut self, id: u64) -> BTreeMap<OutputId, crate::LiveProductionNativeFrameId> {
        let transaction = TransactionId::from_raw(id);
        let candidate = SurfaceTransactionKey {
            transaction,
            surface: self.surface,
            target_buffer: BufferSource::CpuBuffer { handle: 41 },
        };
        self.runtime
            .surface_content_stream
            .begin(candidate)
            .unwrap();
        let resources = self.runtime.presentation_feedback.resources_mut();
        resources.begin_software(transaction, None, None).unwrap();
        assert!(resources.poll_acquire_fence(transaction).unwrap());
        self.runtime
            .queue_software_present_frame(
                &mut self.scene,
                &outputs(),
                vec![LiveProductionSoftwarePresentSubmission {
                    candidate,
                    source_size: Size {
                        width: 16,
                        height: 16,
                    },
                    transaction,
                    surface: self.surface,
                    acquire_fence: None,
                    idle_fence: None,
                }],
            )
            .unwrap();
        assert!(
            self.runtime
                .stage_software_present_frame(&mut self.target, outputs()[0].id)
                .unwrap()
        );
        self.runtime
            .software_present_frames_bound
            .values()
            .next()
            .unwrap()
            .frames
            .clone()
    }

    fn retire(&mut self, frames: &BTreeMap<OutputId, crate::LiveProductionNativeFrameId>) {
        for (output, frame) in frames {
            self.target.complete(*output);
            self.runtime
                .observe_software_present_frame_submitted(*frame)
                .unwrap();
            self.runtime
                .settle_software_present_frame(LiveProductionNativeFrameRetirement {
                    output: *output,
                    frame: *frame,
                    submission: frame.raw(),
                    direct: false,
                    layout_witness: None,
                    content: crate::LiveProductionScanoutContent::RetainedMixed {
                        frame: *frame,
                        logical_content_checksum: None,
                        nonzero_rgb_pixels: 1,
                        requires_retirement: true,
                    },
                    ust: 1234,
                    msc: 567,
                    clocks: crate::LiveNativeRetirementClocks::default(),
                })
                .unwrap();
        }
    }
}

#[test]
fn software_present_singleton_observation_uses_runtime_custody() {
    fn observe(f: &mut Fixture, output: OutputId) -> Option<crate::LiveNativeFrameIdentity> {
        let index = f.runtime.outputs.output_index(output).unwrap();
        let custody = f.target.custody.entry(output).or_default();
        f.runtime
            .outputs
            .run_output(index, &[], |runtime| {
                std::mem::swap(
                    &mut runtime.primary_output_state_mut().scanout_custody,
                    custody,
                );
                Ok(())
            })
            .unwrap();
        let observed = f.runtime.outputs.idle_presented_native_frame(output);
        f.runtime
            .outputs
            .run_output(index, &[], |runtime| {
                std::mem::swap(
                    &mut runtime.primary_output_state_mut().scanout_custody,
                    custody,
                );
                Ok(())
            })
            .unwrap();
        observed
    }
    let mut f = Fixture::new();
    let output = outputs()[1].id;
    assert!(observe(&mut f, output).is_none());
    assert!(
        f.runtime
            .outputs
            .idle_presented_native_frame(OutputId::from_raw(999))
            .is_none()
    );
    f.settle_initial_scene();
    assert!(
        f.runtime
            .outputs
            .idle_presented_native_frame(output)
            .is_none()
    );
    let displayed = observe(&mut f, output).expect("runtime owns a retired singleton");
    assert_eq!(
        displayed.frame(),
        f.target.presented_frame_id(output).unwrap().raw()
    );

    let frames = f.secondary_frames();
    f.target
        .queue_ordinary_batch(vec![(output, frames)])
        .unwrap();
    f.target.begin_render(output);
    f.target.finish_render(output);
    assert!(
        observe(&mut f, output).is_none(),
        "newer submission prevents reuse"
    );
    f.target
        .fail_cleanup(Some(u32::try_from(displayed.frame()).unwrap()));
    assert!(f.target.flip(output, None));
    assert!(
        observe(&mut f, output).is_none(),
        "failed predecessor cleanup prevents reuse"
    );
    f.target.fail_cleanup(None);
    assert_eq!(f.target.retry_cleanup(output), Some(true));
    let next = observe(&mut f, output).expect("retired and cleaned singleton");
    assert_ne!(next.frame(), displayed.frame());
    f.target.teardown();
}

#[test]
fn software_present_keeps_legacy_cpu_with_an_older_snapshot() {
    let mut f = Fixture::new();
    f.settle_initial_scene();
    let secondary = outputs()[1].id;
    assert!(f.secondary_is_idle(&f.secondary_frames()));
    // Model the legacy CPU retirement's observation: native identity matches
    // presented_content, but no snapshot replaced the old presented snapshot.
    let frame = f.target.presented_frame_id(secondary).unwrap();
    f.target.newest.insert(
        secondary,
        crate::LiveProductionScanoutContent::Cpu {
            frame,
            checksum: 99,
        },
    );
    f.expect_both();
}

#[test]
fn software_present_keeps_mismatched_native_content_identity() {
    let mut f = Fixture::new();
    f.settle_initial_scene();
    f.target.newest.insert(
        outputs()[1].id,
        crate::LiveProductionScanoutContent::HeadComposition {
            frame: crate::LiveProductionNativeFrameId::from_raw(999),
            logical_content_checksum: 0,
            nonzero_rgb_pixels: 0,
        },
    );
    f.expect_both();
}

#[test]
fn software_present_keeps_outputs_without_a_retired_baseline() {
    Fixture::new().expect_both();
}

#[test]
fn software_present_keeps_clock_retirement_when_no_output_samples_it() {
    let mut f = Fixture::new();
    f.runtime.presentation_order.clear();
    f.settle_initial_scene();
    let frames = f.stage(100);
    assert_eq!(
        frames.keys().copied().collect::<Vec<_>>(),
        vec![outputs()[0].id]
    );
    assert!(f.runtime.retired_software_presents.is_empty());
    f.retire(&frames);
    assert_eq!(f.runtime.retired_software_presents.len(), 1);
    f.target.teardown();
}

#[test]
fn software_present_keeps_an_unchanged_output_sampling_the_present() {
    let mut f = Fixture::new();
    commit_cpu_surface(
        &mut f.runtime,
        &mut f.scene,
        f.surface,
        41,
        2,
        rect(64, 0, 16, 16),
    );
    f.runtime.surface_outputs.insert(f.surface, outputs()[1].id);
    f.settle_initial_scene();
    f.expect_both();
}

#[test]
fn software_present_keeps_an_unchanged_preview_of_the_present() {
    let mut f = Fixture::new();
    let secondary = outputs()[1].id;
    let mut record = presentation_output(secondary, PolicyPresentationMode::Overlay);
    record.coverage = rect(64, 0, 64, 32);
    f.runtime
        .set_policy_presentation(
            Some(published(
                1,
                vec![record],
                vec![shown_instance(
                    secondary,
                    1,
                    0,
                    f.surface,
                    rect(66, 2, 8, 8),
                )],
                vec![],
            )),
            &f.scene,
            None,
        )
        .unwrap();
    f.settle_initial_scene();
    assert!(
        f.target
            .presented_frame(secondary)
            .unwrap()
            .compositor_display_list
            .surface_instances()
            .any(|instance| instance.source == f.surface)
    );
    f.expect_both();
}

#[test]
fn software_present_keeps_the_output_a_surface_moved_off() {
    let mut f = Fixture::new();
    commit_cpu_surface(
        &mut f.runtime,
        &mut f.scene,
        f.surface,
        41,
        2,
        rect(64, 0, 16, 16),
    );
    f.runtime.surface_outputs.insert(f.surface, outputs()[1].id);
    f.settle_initial_scene();
    commit_cpu_surface(
        &mut f.runtime,
        &mut f.scene,
        f.surface,
        41,
        3,
        rect(0, 0, 16, 16),
    );
    f.runtime.surface_outputs.insert(f.surface, outputs()[0].id);
    f.expect_both();
}

#[test]
fn software_present_keeps_other_content_changes_but_can_leave_static_content_idle() {
    for changed in [false, true] {
        let mut f = Fixture::new();
        let other = SurfaceId::new(42, 1);
        commit_cpu_surface(
            &mut f.runtime,
            &mut f.scene,
            other,
            42,
            1,
            rect(64, 0, 16, 16),
        );
        f.runtime.surface_outputs.insert(other, outputs()[1].id);
        f.runtime.presentation_order.push(other);
        f.settle_initial_scene();
        if changed {
            commit_cpu_surface(
                &mut f.runtime,
                &mut f.scene,
                other,
                42,
                2,
                rect(64, 0, 16, 16),
            );
        }
        // The global history advances, while only this output's sampled
        // identities determine whether it still shows the required scene.
        commit_cpu_surface(
            &mut f.runtime,
            &mut f.scene,
            f.surface,
            41,
            2,
            rect(0, 0, 16, 16),
        );
        let frames = f.stage(100);
        assert_eq!(frames.len(), if changed { 2 } else { 1 });
        f.retire(&frames);
        f.target.teardown();
    }
}

#[test]
fn software_present_keeps_lock_and_output_retirement_obligations() {
    for obligation in 0..4 {
        let mut f = Fixture::new();
        let secondary = outputs()[1].id;
        if obligation == 0 {
            f.runtime.session_lock = Some(SessionLockCover::fill(
                SessionLockEpoch::FIRST,
                CompositorRgb8 {
                    red: 0,
                    green: 0,
                    blue: 0,
                },
            ));
        }
        f.settle_initial_scene();
        match obligation {
            0 => {}
            1 => {
                f.runtime.ordinary_repaints_pending.insert(secondary);
            }
            2 => {
                f.runtime
                    .retained_projection_retirements
                    .insert((secondary, LiveShellContentLayer::Shell), grant());
            }
            3 => {
                f.runtime.queued_shell_retirements.insert(
                    (secondary, f.target.presented_frame_id(secondary).unwrap()),
                    BTreeMap::from([((secondary, LiveShellContentLayer::Shell), grant())]),
                );
            }
            _ => unreachable!(),
        }
        f.expect_both();
    }
}

#[test]
fn software_present_idle_proof_refuses_changed_cursor_scene_and_target() {
    let mut f = Fixture::new();
    f.settle_initial_scene();
    assert!(f.secondary_is_idle(&f.secondary_frames()));
    for change in 0..7 {
        let mut frames = f.secondary_frames();
        match change {
            0 => {
                frames[0].target_generation += 1;
            }
            1 => {
                frames[0].head = RenderHeadId::from_raw(99);
            }
            2 => {
                frames[0].frame.output_damage_snapshot = None;
            }
            3 => {
                frames[0]
                    .frame
                    .output_damage_snapshot
                    .as_mut()
                    .unwrap()
                    .software_cursor = Some(rect(1, 1, 2, 2));
            }
            4 => {
                frames[0]
                    .frame
                    .output_damage_snapshot
                    .as_mut()
                    .unwrap()
                    .output
                    .scale += 1;
            }
            5 => {
                frames[0]
                    .frame
                    .output_damage_snapshot
                    .as_mut()
                    .unwrap()
                    .compositor_display_list
                    .commands
                    .push(CompositorDisplayCommand::PresentationStamp(
                        CompositorPresentationStamp {
                            owner_epoch: 1,
                            publication_generation: 1,
                            output: outputs()[1].id,
                            output_generation: 1,
                            coverage: rect(0, 0, 64, 32),
                            keyboard: PresentedKeyboardScope::Held,
                        },
                    ));
            }
            6 => {
                frames.clear();
            }
            _ => unreachable!(),
        }
        assert!(!f.secondary_is_idle(&frames), "change {change} discarded");
    }
    f.target.teardown();
}

#[test]
fn software_present_idle_proof_refuses_pending_rendering_and_submitted_work() {
    let mut f = Fixture::new();
    f.settle_initial_scene();
    let frames = f.secondary_frames();
    let secondary = outputs()[1].id;
    f.target
        .queue_ordinary_batch(vec![(secondary, f.secondary_frames())])
        .unwrap();
    assert!(
        !f.secondary_is_idle(&frames),
        "pending successor cannot be forgotten"
    );
    f.target.begin_render(secondary);
    assert!(
        !f.secondary_is_idle(&frames),
        "rendering successor cannot be forgotten"
    );
    f.target.finish_render(secondary);
    assert!(
        !f.secondary_is_idle(&frames),
        "submitted successor cannot be forgotten"
    );
    assert!(f.target.flip(secondary, None));
    assert!(f.secondary_is_idle(&frames));
    f.target.teardown();
}

#[test]
fn software_present_leaves_an_unchanged_secondary_output_idle() {
    let mut f = Fixture::new();
    f.settle_initial_scene();
    let secondary = outputs()[1].id;
    let displayed = f.target.presented_frame_id(secondary);
    let mut previous_primary = f.target.presented_frame_id(outputs()[0].id);
    // Even identical Presents must each own a fresh primary retirement.
    for id in 100..103 {
        let frames = f.stage(id);
        assert_eq!(
            frames.len(),
            1,
            "unchanged secondary must not join the Present cohort"
        );
        assert!(!f.target.queue.pending(secondary));
        assert_eq!(f.target.presented_frame_id(secondary), displayed);
        let primary = frames[&outputs()[0].id];
        assert_ne!(Some(primary), previous_primary);
        assert!(f.runtime.retired_software_presents.is_empty());
        f.retire(&frames);
        assert!(f.runtime.software_present_frames_bound.is_empty());
        let mut receipts = Vec::new();
        f.runtime
            .drain_retired_software_presents_into(&mut receipts)
            .unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!((receipts[0].ust_usec, receipts[0].msc), (1234, 567));
        previous_primary = Some(primary);
    }
    f.target.teardown();
}
