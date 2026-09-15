//! Production mirror completion, real boxed copied buffers and fake cleanup only.
//! These controls do not claim that the worker/KMS mirror adapter has run.
use super::*;
use crate::{MirrorCompletionWitness, complete_mirror_head};

struct Mirror {
    owner: crate::NativeFrameOwner,
    group: crate::LiveProductionMirrorGroupLifecycle,
    custody: [crate::PersistentScanoutCustody; 2],
    device: Device,
    owners: Rc<Cell<usize>>,
    last: [Option<u64>; 2],
    cohorts: BTreeMap<u64, sophia_engine::OutputPresentationCohort>,
}

impl Mirror {
    fn new() -> Self {
        Self {
            owner: crate::NativeFrameOwner::new(),
            group: crate::LiveProductionMirrorGroupLifecycle::new(
                OutputId::from_raw(1),
                [head(0), head(1)],
            )
            .unwrap(),
            custody: Default::default(),
            device: Device {
                destroyed: Cell::new(0),
                failing: Cell::new(None),
            },
            owners: Rc::new(Cell::new(0)),
            last: [None; 2],
            cohorts: BTreeMap::new(),
        }
    }

    fn begin(&mut self, frame: u64) {
        assert_eq!(
            self.group
                .begin(crate::LiveProductionNativeFrameId::from_raw(frame)),
            crate::LiveProductionMirrorGroupBegin::Started
        );
        let mut cohort = sophia_engine::OutputPresentationCohort::new(
            OutputId::from_raw(1),
            frame,
            head(0),
            [head(0), head(1)],
        )
        .unwrap();
        // Simulated renderer preparation; real cohort enforces the all-head barrier.
        for index in 0..2 {
            assert!(matches!(
                cohort.mark_prepared(sophia_engine::HeadFrameCandidate {
                    candidate: sophia_engine::HeadFrameCandidateId::from_raw(
                        frame * 10 + index as u64
                    ),
                    output: OutputId::from_raw(1),
                    scene_generation: frame,
                    head: head(index),
                    target_generation: 1,
                    logical_content_checksum: 99,
                }),
                sophia_engine::OutputPresentationTransition::Accepted
                    | sophia_engine::OutputPresentationTransition::PhaseReady
            ));
        }
        self.cohorts.insert(frame, cohort);
    }

    fn submit(&mut self, index: usize, frame: u64) {
        self.owners.set(self.owners.get() + 1);
        self.custody[index]
            .accept_submission(crate::LiveRenderedPrimaryPlaneScanoutSubmission {
                scanout_buffer: Box::new(CopiedBacking {
                    bytes: vec![0x7f; 32],
                    live: self.owners.clone(),
                }),
                correlation: Some(crate::LiveRendererFrameCorrelation {
                    native: Some(self.identity(index, frame)),
                    request: None,
                    trace: None,
                    direct_scanout: None,
                }),
                primary_plane: crate::LibdrmNativePrimaryPlaneScanoutSubmission {
                    resources: crate::LibdrmNativePrimaryPlaneResourceBundle::new(
                        NonZeroU32::new((frame * 10 + index as u64) as u32)
                            .unwrap()
                            .into(),
                        None,
                        Size {
                            width: 4,
                            height: 2,
                        },
                    ),
                    completion_fence: None,
                },
                submitted_after_page_flip_serial: self.last[index],
                layout_witness: None,
            })
            .unwrap();
        assert!(matches!(
            self.cohorts
                .get_mut(&frame)
                .unwrap()
                .mark_submitted(head(index)),
            sophia_engine::OutputPresentationTransition::Accepted
                | sophia_engine::OutputPresentationTransition::PhaseReady
        ));
        assert!(matches!(
            self.group.mark_submitted(
                head(index),
                crate::LiveProductionNativeFrameId::from_raw(frame)
            ),
            crate::LiveProductionMirrorHeadTransition::Accepted
                | crate::LiveProductionMirrorHeadTransition::GroupReady
        ));
    }

    fn identity(&self, index: usize, frame: u64) -> crate::LiveNativeFrameIdentity {
        self.owner
            .frame(OutputId::from_raw(1), head(index), 1, frame)
    }

    fn flip(&mut self, index: usize, frame: u64, serial: u64) -> crate::PersistentFlipOutcome {
        let result = complete_mirror_head(
            &self.device,
            &mut self.custody[index],
            &mut self.group,
            self.cohorts.get_mut(&frame),
            MirrorCompletionWitness {
                expected: self
                    .owner
                    .frame(OutputId::from_raw(1), head(index), 1, frame),
                callback: crate::LivePageFlipCallback {
                    output: OutputId::from_raw(1),
                    head: head(index),
                    frame_serial: serial,
                },
                last_callback_serial: self.last[index],
                ust_usec: serial * 1000,
            },
        );
        if matches!(
            result.physical,
            crate::PersistentFlipOutcome::Presented { .. }
        ) {
            self.last[index] = Some(serial);
            assert!(result.timing_valid);
            assert!(matches!(
                result.cohort,
                Some(
                    sophia_engine::OutputPresentationTransition::Accepted
                        | sophia_engine::OutputPresentationTransition::PhaseReady
                )
            ));
            assert_eq!(
                result.logical,
                Some(if index == 0 {
                    crate::LiveProductionMirrorHeadTransition::GroupReady
                } else {
                    crate::LiveProductionMirrorHeadTransition::Accepted
                })
            );
        } else {
            assert!(result.logical.is_none());
        }
        result.physical
    }
}

fn head(index: usize) -> RenderHeadId {
    RenderHeadId::from_raw(index as u64 + 1)
}

#[test]
fn primary_progress_and_old_cleanup_failure_do_not_release_a_delayed_sibling() {
    let mut mirror = Mirror::new();
    mirror.begin(1);
    mirror.submit(0, 1);
    mirror.submit(1, 1);
    assert!(matches!(
        mirror.flip(0, 1, 1),
        crate::PersistentFlipOutcome::Presented { .. }
    ));
    assert_eq!(mirror.group.completed_frame().unwrap().raw(), 1);
    assert!(mirror.custody[1].submitted().is_some());
    mirror.begin(2);
    mirror.submit(0, 2);
    mirror.device.failing.set(Some(10));
    assert!(matches!(
        mirror.flip(0, 2, 2),
        crate::PersistentFlipOutcome::Presented {
            previous_cleanup: Some(crate::ScanoutCleanupOutcome {
                released: false,
                ..
            }),
            ..
        }
    ));
    assert_eq!(mirror.group.completed_frame().unwrap().raw(), 2);
    assert_eq!(mirror.owners.get(), 3); // new primary, failed predecessor, delayed sibling
    assert!(mirror.custody[1].submitted().is_some());
    assert!(matches!(
        mirror.flip(1, 1, 1),
        crate::PersistentFlipOutcome::Presented { .. }
    ));
    assert_eq!(mirror.group.completed_frame().unwrap().raw(), 2);
    assert_eq!(mirror.group.flip_timing(), Some((2, 2000)));
    mirror.device.failing.set(None);
    assert!(
        mirror.custody[0]
            .retry_cleanup(&mirror.device)
            .unwrap()
            .released
    );
    assert!(mirror.custody[0].retry_cleanup(&mirror.device).is_none());
    for custody in &mut mirror.custody {
        assert!(
            custody
                .retire_displayed(&mirror.device)
                .unwrap()
                .unwrap()
                .released
        );
    }
    assert_eq!(mirror.owners.get(), 0);
    assert_eq!(mirror.device.destroyed.get(), 3);
}

#[test]
fn wrong_native_witness_and_stale_callback_do_not_advance_physical_or_logical_custody() {
    let mut mirror = Mirror::new();
    mirror.begin(1);
    mirror.submit(0, 1);
    let foreign = crate::NativeFrameOwner::new();
    for expected in [
        foreign.frame(OutputId::from_raw(1), head(0), 1, 1),
        mirror.owner.frame(OutputId::from_raw(1), head(0), 2, 1),
        mirror.owner.frame(OutputId::from_raw(1), head(1), 1, 1),
        mirror.owner.frame(OutputId::from_raw(2), head(0), 1, 1),
        mirror.owner.frame(OutputId::from_raw(1), head(0), 1, 2),
    ] {
        let result = complete_mirror_head(
            &mirror.device,
            &mut mirror.custody[0],
            &mut mirror.group,
            mirror.cohorts.get_mut(&1),
            MirrorCompletionWitness {
                expected,
                callback: crate::LivePageFlipCallback {
                    output: OutputId::from_raw(1),
                    head: head(0),
                    frame_serial: 1,
                },
                last_callback_serial: None,
                ust_usec: 1000,
            },
        );
        assert!(matches!(
            result.physical,
            crate::PersistentFlipOutcome::IdentityMismatch
        ));
        assert!(result.logical.is_none());
        assert!(result.cohort.is_none());
        assert!(mirror.custody[0].submitted().is_some());
        assert!(mirror.custody[0].displayed().is_none());
        assert!(mirror.group.completed_frame().is_none());
        assert_eq!(mirror.device.destroyed.get(), 0);
    }
    mirror.last[0] = Some(1);
    assert!(matches!(
        mirror.flip(0, 1, 1),
        crate::PersistentFlipOutcome::Waiting
    ));
    assert!(matches!(
        mirror.flip(0, 1, 2),
        crate::PersistentFlipOutcome::Presented { .. }
    ));
    assert!(matches!(
        mirror.flip(0, 1, 3),
        crate::PersistentFlipOutcome::IdentityMismatch
    ));
    assert_eq!(mirror.owners.get(), 1);
    assert!(
        mirror.custody[0]
            .retire_displayed(&mirror.device)
            .unwrap()
            .unwrap()
            .released
    );
    assert_eq!(mirror.owners.get(), 0);
}

#[test]
fn secondary_flip_never_publishes_the_primary_logical_frame() {
    let mut mirror = Mirror::new();
    mirror.begin(1);
    mirror.submit(0, 1);
    mirror.submit(1, 1);
    assert!(matches!(
        mirror.flip(1, 1, 1),
        crate::PersistentFlipOutcome::Presented { .. }
    ));
    assert!(mirror.group.completed_frame().is_none());
    assert!(mirror.group.flip_timing().is_none());
    assert!(mirror.custody[0].submitted().is_some());
    assert_eq!(mirror.owners.get(), 2);
    assert!(matches!(
        mirror.flip(0, 1, 2),
        crate::PersistentFlipOutcome::Presented { .. }
    ));
    assert_eq!(mirror.group.flip_timing(), Some((1, 2000)));
    for custody in &mut mirror.custody {
        assert!(
            custody
                .retire_displayed(&mirror.device)
                .unwrap()
                .unwrap()
                .released
        );
    }
    assert_eq!(mirror.owners.get(), 0);
}

#[test]
fn poisoned_mirror_still_retires_its_physical_owner_without_logical_success() {
    let mut mirror = Mirror::new();
    mirror.begin(1);
    mirror.submit(0, 1);
    assert!(
        mirror
            .group
            .abort(crate::LiveProductionNativeFrameId::from_raw(1))
    );
    let expected = mirror.identity(0, 1);
    let result = complete_mirror_head(
        &mirror.device,
        &mut mirror.custody[0],
        &mut mirror.group,
        mirror.cohorts.get_mut(&1),
        MirrorCompletionWitness {
            expected,
            callback: crate::LivePageFlipCallback {
                output: OutputId::from_raw(1),
                head: head(0),
                frame_serial: 1,
            },
            last_callback_serial: None,
            ust_usec: 1000,
        },
    );
    assert!(matches!(
        result.physical,
        crate::PersistentFlipOutcome::Presented { .. }
    ));
    assert!(result.logical.is_none());
    assert!(matches!(
        mirror.cohorts[&1].terminal(),
        Some(sophia_engine::OutputPresentationTerminal::Failed(_))
    ));
    assert!(mirror.group.completed_frame().is_none());
    assert!(mirror.custody[0].submitted().is_none());
    assert!(mirror.custody[0].displayed().is_some());
    assert!(
        mirror.custody[0]
            .retire_displayed(&mirror.device)
            .unwrap()
            .unwrap()
            .released
    );
    assert_eq!(mirror.owners.get(), 0);
}

#[test]
fn newer_abort_cannot_publish_an_older_inflight_cohort_or_retract_an_old_terminal() {
    for primary_already_presented in [false, true] {
        let mut mirror = Mirror::new();
        mirror.begin(1);
        mirror.submit(0, 1);
        mirror.submit(1, 1);
        if primary_already_presented {
            assert!(matches!(
                mirror.flip(0, 1, 1),
                crate::PersistentFlipOutcome::Presented { .. }
            ));
        }
        let previous_terminal = mirror.cohorts[&1].terminal();
        mirror.begin(2);
        assert!(
            mirror
                .group
                .abort(crate::LiveProductionNativeFrameId::from_raw(2))
        );
        let index = usize::from(primary_already_presented);
        let expected = mirror.identity(index, 1);
        let result = complete_mirror_head(
            &mirror.device,
            &mut mirror.custody[index],
            &mut mirror.group,
            mirror.cohorts.get_mut(&1),
            MirrorCompletionWitness {
                expected,
                callback: crate::LivePageFlipCallback {
                    output: OutputId::from_raw(1),
                    head: head(index),
                    frame_serial: 2,
                },
                last_callback_serial: None,
                ust_usec: 2000,
            },
        );
        assert!(matches!(
            result.physical,
            crate::PersistentFlipOutcome::Presented { .. }
        ));
        assert!(result.logical.is_none());
        if primary_already_presented {
            assert_eq!(mirror.cohorts[&1].terminal(), previous_terminal);
            assert_eq!(mirror.group.flip_timing(), Some((1, 1000)));
        } else {
            assert!(matches!(
                mirror.cohorts[&1].terminal(),
                Some(sophia_engine::OutputPresentationTerminal::Failed(_))
            ));
            assert!(mirror.group.completed_frame().is_none());
        }
        for custody in &mut mirror.custody {
            if custody.submitted().is_some() {
                assert!(custody.discard_submitted(&mirror.device).unwrap().released);
            }
            if custody.displayed().is_some() {
                assert!(
                    custody
                        .retire_displayed(&mirror.device)
                        .unwrap()
                        .unwrap()
                        .released
                );
            }
        }
        assert_eq!(mirror.owners.get(), 0);
    }
}

fn supplied_cohort(
    output: u64,
    frame: u64,
    primary: usize,
    target: u64,
    head_count: usize,
) -> sophia_engine::OutputPresentationCohort {
    let output = OutputId::from_raw(output);
    let mut cohort = sophia_engine::OutputPresentationCohort::new(
        output,
        frame,
        head(primary),
        (0..head_count).map(head),
    )
    .unwrap();
    for index in 0..head_count {
        cohort.mark_prepared(sophia_engine::HeadFrameCandidate {
            candidate: sophia_engine::HeadFrameCandidateId::from_raw(index as u64 + 1),
            output,
            scene_generation: frame,
            head: head(index),
            target_generation: target,
            logical_content_checksum: 99,
        });
    }
    for index in 0..head_count {
        cohort.mark_submitted(head(index));
    }
    cohort
}

#[test]
fn mismatched_supplied_cohort_refuses_before_any_owner_or_cohort_mutation() {
    let mut mirror = Mirror::new();
    mirror.begin(1);
    mirror.submit(0, 1);
    let expected = mirror.identity(0, 1);
    let mut not_submitted = sophia_engine::OutputPresentationCohort::new(
        OutputId::from_raw(1),
        1,
        head(0),
        [head(0), head(1)],
    )
    .unwrap();
    not_submitted.mark_prepared(sophia_engine::HeadFrameCandidate {
        candidate: sophia_engine::HeadFrameCandidateId::from_raw(1),
        output: OutputId::from_raw(1),
        scene_generation: 1,
        head: head(0),
        target_generation: 1,
        logical_content_checksum: 99,
    });
    let mut already_flipped = supplied_cohort(1, 1, 0, 1, 2);
    already_flipped.mark_flipped(head(0), 1000);
    for mut cohort in [
        supplied_cohort(2, 1, 0, 1, 2),
        supplied_cohort(1, 2, 0, 1, 2),
        supplied_cohort(1, 1, 1, 1, 2),
        supplied_cohort(1, 1, 0, 2, 2),
        supplied_cohort(1, 1, 0, 1, 1),
        not_submitted,
        already_flipped,
    ] {
        let before_cohort = cohort.clone();
        let before_group = mirror.group.clone();
        let result = complete_mirror_head(
            &mirror.device,
            &mut mirror.custody[0],
            &mut mirror.group,
            Some(&mut cohort),
            MirrorCompletionWitness {
                expected,
                callback: crate::LivePageFlipCallback {
                    output: OutputId::from_raw(1),
                    head: head(0),
                    frame_serial: 1,
                },
                last_callback_serial: None,
                ust_usec: 1000,
            },
        );
        assert!(matches!(
            result.physical,
            crate::PersistentFlipOutcome::IdentityMismatch
        ));
        assert!(result.logical.is_none());
        assert!(result.cohort.is_none());
        assert_eq!(cohort, before_cohort);
        assert_eq!(mirror.group, before_group);
        assert!(mirror.custody[0].submitted().is_some());
        assert!(mirror.custody[0].displayed().is_none());
        assert_eq!(mirror.owners.get(), 1);
        assert_eq!(mirror.device.destroyed.get(), 0);
    }
    assert!(matches!(
        mirror.flip(0, 1, 1),
        crate::PersistentFlipOutcome::Presented { .. }
    ));
    assert!(
        mirror.custody[0]
            .retire_displayed(&mirror.device)
            .unwrap()
            .unwrap()
            .released
    );
    assert_eq!(mirror.owners.get(), 0);
}
