use super::*;

#[test]
fn foreign_previews_do_not_join_the_ordinary_present_retirement_cohort() {
    let mut f = present_scene();
    let remote = outputs()[1].id;
    let bounds = rect(0, 0, 16, 16);
    let publication = |mode| {
        published(
            10,
            vec![
                presentation_output(f.output, mode),
                presentation_output(remote, PolicyPresentationMode::Overlay),
            ],
            vec![shown_instance(
                remote,
                1,
                0,
                f.application,
                rect(8, 8, 8, 8),
            )],
            vec![],
        )
    };
    f.runtime
        .set_policy_presentation(
            Some(publication(PolicyPresentationMode::Overlay)),
            &f.scene,
            None,
        )
        .unwrap();
    assert_eq!(
        f.runtime
            .present_retirement_outputs(f.application, Some(bounds), bounds)
            .unwrap(),
        vec![f.output],
        "a remote 60 Hz preview must not join a 120 Hz source's completion"
    );
    f.runtime
        .presentation_order
        .retain(|id| *id != f.application);
    assert_eq!(
        f.runtime
            .present_retirement_outputs(f.application, Some(bounds), bounds)
            .unwrap(),
        vec![remote],
        "a hidden-workspace source has exactly one visible-preview retirement owner"
    );
    f.runtime.presentation_order.push(f.application);
    f.runtime
        .set_policy_presentation(
            Some(publication(PolicyPresentationMode::ReplaceApplications)),
            &f.scene,
            None,
        )
        .unwrap();
    assert_eq!(
        f.runtime
            .present_retirement_outputs(f.application, Some(bounds), bounds)
            .unwrap(),
        vec![remote],
        "a replacement hiding the application's own draw delegates to its visible preview"
    );
    f.runtime
        .set_policy_presentation(None, &f.scene, None)
        .unwrap();
    f.runtime
        .presentation_order
        .retain(|id| *id != f.application);
    assert!(
        f.runtime
            .present_retirement_outputs(f.application, Some(bounds), bounds)
            .unwrap()
            .is_empty(),
        "a source with neither application nor preview stays on the existing Skipped path"
    );
}

#[test]
fn preview_admission_failure_revokes_once_while_a_busy_donor_only_defers() {
    let mut f = present_scene();
    f.replace_with(vec![
        shown_instance(f.output, 2, 1, f.previewed, rect(9, 0, 8, 8)),
        shown_instance(f.output, 1, 0, f.application, rect(0, 0, 8, 8)),
    ]);
    let image = sophia_renderer_live::LiveRendererImageId::from_raw(PRESENT_IMAGE);
    f.runtime.displayed_surfaces.insert(
        f.application,
        LiveDisplayedSurface {
            layer: crate::LiveRetainedRendererImageLayer {
                image_id: image,
                size: Size {
                    width: 16,
                    height: 16,
                },
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                placement: sophia_renderer_live::LiveCompositionPlacement {
                    target: rect(0, 0, 16, 16),
                    clip: None,
                    transform: sophia_protocol::Transform::IDENTITY,
                    alpha: 1.0,
                    sampling: sophia_engine::HeadSamplingClass::Exact,
                },
            },
        },
    );
    for detail in [
        crate::LiveRendererScanoutBufferExportDetail::WorkerStalled,
        crate::LiveRendererScanoutBufferExportDetail::WorkerDisconnected,
        crate::LiveRendererScanoutBufferExportDetail::EglContextUnavailable,
    ] {
        assert!(
            !f.runtime
                .handle_preview_refusal(&crate::LivePreviewImageRefusal::Renderer {
                    image,
                    detail
                })
        );
        assert!(f.runtime.policy_presentation().is_some());
    }
    for detail in [
        crate::LiveRendererScanoutBufferExportDetail::WorkerPending,
        crate::LiveRendererScanoutBufferExportDetail::WorkerQueueFull,
    ] {
        assert!(
            f.runtime
                .handle_preview_refusal(&crate::LivePreviewImageRefusal::Renderer {
                    image,
                    detail
                })
        );
        assert!(f.runtime.policy_presentation().is_some());
        assert!(f.runtime.take_policy_presentation_revocation().is_none());
    }
    assert!(
        f.runtime
            .handle_preview_refusal(&crate::LivePreviewImageRefusal::Renderer {
                image,
                detail: crate::LiveRendererScanoutBufferExportDetail::RendererImageStoreFull,
            })
    );
    assert!(f.runtime.policy_presentation().is_none());
    assert!(f.runtime.retained_projection_pending);
    let revoked = f.runtime.take_policy_presentation_revocation().unwrap();
    assert_eq!(
        (revoked.owner_epoch, revoked.generation, revoked.source),
        (41, 1, f.application)
    );
    assert!(f.runtime.take_policy_presentation_revocation().is_none());
    let unrelated: Box<dyn std::error::Error> = "unrelated runtime invariant".into();
    assert!(!f.runtime.handle_preview_refusal(unrelated.as_ref()));
}

#[test]
fn preview_recovery_rebuilds_the_original_present_candidate_and_client_planes() {
    let mut fixture = present_scene();
    commit_dma_surface(
        &mut fixture.runtime,
        fixture.previewed,
        2,
        rect(62, 0, 16, 16),
    );
    fixture.runtime.surface_outputs.remove(&fixture.previewed);
    fixture
        .runtime
        .geometry_routed_surfaces
        .insert(fixture.previewed);
    let neighbour_image = sophia_renderer_live::LiveRendererImageId::from_raw(777);
    let (transaction, _) = fixture.queue_present(950, Instant::now());
    let mut queued = fixture.runtime.present_scheduler.pop_front().unwrap();
    queued.candidate.previous_committed_generation = 1;
    queued.candidate.target_geometry = rect(60, 0, 16, 16);
    fixture.runtime.surface_outputs.remove(&fixture.application);
    fixture
        .runtime
        .geometry_routed_surfaces
        .insert(fixture.application);
    let prepared = fixture
        .runtime
        .production
        .prepare_present_transaction(&queued.candidate);
    assert!(prepared.is_ready());
    let source = prepared
        .candidate()
        .iter()
        .find(|state| state.surface == fixture.application)
        .unwrap()
        .clone();
    let a = fixture.output;
    let b = outputs()[1].id;
    let image = crate::presentation::renderer_image_for_present(transaction);
    let mut submitted = crate::LiveProductionSubmittedPresent::new(
        BTreeMap::from([
            (a, crate::LiveProductionNativeFrameId::from_raw(10)),
            (b, crate::LiveProductionNativeFrameId::from_raw(11)),
        ]),
        a,
        queued.candidate.key(),
        transaction,
        fixture.application,
        prepared,
        crate::LiveRetainedRendererImageLayer {
            image_id: image,
            size: Size {
                width: 16,
                height: 16,
            },
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            placement: sophia_renderer_live::LiveCompositionPlacement {
                target: rect(60, 0, 16, 16),
                clip: Some(rect(60, 0, 16, 16)),
                transform: sophia_protocol::Transform::IDENTITY,
                alpha: 1.0,
                sampling: sophia_engine::HeadSamplingClass::Exact,
            },
        },
    )
    .unwrap();
    let mut current = fixture
        .runtime
        .presentation_feedback
        .resources()
        .build_mixed_frame(
            transaction,
            None,
            rect(60, 0, 16, 16),
            Some(rect(60, 0, 16, 16)),
            1.0,
        )
        .unwrap();
    let sophia_renderer_live::LiveOwnedMixedCompositionLayer::DmaBuf {
        image_id, frame, ..
    } = current.layers.pop().unwrap()
    else {
        panic!("client buffer");
    };
    submitted.recovery_sources = vec![sophia_renderer_live::LiveOwnedHeadCompositionSource {
        surface: fixture.application,
        source: source.buffer(),
        kind: sophia_renderer_live::LiveOwnedHeadCompositionSourceKind::DmaBuf { image_id, frame },
    }];
    submitted
        .recovery_sources
        .push(sophia_renderer_live::LiveOwnedHeadCompositionSource {
            surface: fixture.previewed,
            source: BufferSource::DmaBuf { handle: 77 },
            kind: sophia_renderer_live::LiveOwnedHeadCompositionSourceKind::RendererImage {
                image_id: neighbour_image,
                size: Size {
                    width: 16,
                    height: 16,
                },
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            },
        });
    submitted.image_reads = fixture
        .runtime
        .image_reads_for_sources(&submitted.recovery_sources);
    submitted.recovery_order = fixture.runtime.presentation_order.clone();
    fixture.runtime.present_scheduler.mark_rendering(submitted);
    fixture
        .runtime
        .release_removed_presentations(&[fixture.previewed], None)
        .unwrap();
    fixture
        .runtime
        .prepare_authority_transactions(TransactionId::from_raw(990), &[], &[fixture.previewed])
        .unwrap();
    assert!(
        !fixture
            .runtime
            .image_reads
            .request_eviction(neighbour_image)
    );
    fixture
        .runtime
        .present_scheduler
        .mark_output_submitted(a)
        .unwrap();
    // A later scene is not the candidate whose client completion is owed.
    commit_dma_surface(
        &mut fixture.runtime,
        fixture.application,
        2,
        rect(4, 8, 16, 16),
    );
    assert_ne!(
        fixture
            .runtime
            .committed_surfaces()
            .iter()
            .find(|s| s.surface == fixture.application)
            .unwrap(),
        &source
    );
    // Withdrawal has cleared native pending work, but the scheduler still
    // owns its original output set. Both repaint routes must remain blocked.
    let old_b = crate::LiveProductionNativeFrameId::from_raw(11);
    fixture.runtime.retained_projection_pending = true;
    fixture.runtime.ordinary_repaints_pending.insert(b);
    assert!(
        !fixture
            .runtime
            .queue_retained_projection(&fixture.scene, &mut fixture.target)
            .unwrap()
    );
    fixture
        .runtime
        .service_ordinary_repaints(&fixture.scene, &mut fixture.target)
        .unwrap();
    assert!(fixture.runtime.ordinary_repaints_pending.contains(&b));
    assert_eq!(
        fixture.runtime.present_scheduler.unsubmitted_frame(b),
        Some(old_b)
    );
    fixture.runtime.software_present_frames_waiting.push_back(
        software_present::LiveProductionSoftwarePresentFrame {
            source_set: compositor_graphics::LiveProductionRetainedCompositionSourceSet {
                _image_reads: Default::default(),
                committed: Vec::new(),
                presentation_order: Vec::new(),
                scene_generation: 1,
                sources: Vec::new(),
            },
            submissions: vec![crate::LiveProductionSoftwarePresentSubmission {
                candidate: queued.candidate.key(),
                transaction,
                surface: fixture.application,
                source_size: Size {
                    width: 16,
                    height: 16,
                },
                acquire_fence: None,
                idle_fence: None,
            }],
        },
    );
    assert!(
        !fixture
            .runtime
            .stage_software_present_frame(&mut fixture.target, b)
            .unwrap()
    );
    assert_eq!(fixture.runtime.software_present_frames_waiting.len(), 1);
    assert!(fixture.runtime.software_present_frames_bound.is_empty());
    fixture.runtime.software_present_frames_waiting.clear();
    // A late failure of N must neither revoke nor draw N+1 using N's frozen
    // source set. Even a valid newer tier must wait for a retained repaint.
    fixture
        .runtime
        .set_policy_presentation(
            Some(published(
                21,
                vec![presentation_output(b, PolicyPresentationMode::Overlay)],
                vec![shown_instance(
                    b,
                    71,
                    0,
                    fixture.application,
                    rect(8, 8, 8, 8),
                )],
                vec![region(
                    b,
                    72,
                    1,
                    PolicyPresentationRegionRole::Backdrop,
                    rect(0, 0, 64, 32),
                    rect(0, 0, 64, 32),
                )],
            )),
            &fixture.scene,
            None,
        )
        .unwrap();
    let (rebuilt_transaction, rebuilt_image, batches) = fixture
        .runtime
        .present_preview_recovery_frames(&fixture.scene, &fixture.target, b)
        .unwrap();
    assert_eq!((rebuilt_transaction, rebuilt_image), (transaction, image));
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].0, b);
    assert!(crate::live_present_head_frames_capture_image(
        &batches, image
    ));
    assert!(batches[0].1.iter().flat_map(|head| &head.frame.layers).any(|layer| matches!(layer,
        sophia_renderer_live::LiveOwnedMixedCompositionLayer::RendererImage { image_id, .. } if *image_id == neighbour_image)),
        "the retry must still sample its removed neighbour from frozen sources");
    for head in &batches[0].1 {
        assert!(head.frame.layers.iter().any(|layer| matches!(layer,
            sophia_renderer_live::LiveOwnedMixedCompositionLayer::DmaBuf { image_id, .. } if *image_id == image)));
        let snapshot = head.frame.output_damage_snapshot.as_ref().unwrap();
        assert_eq!(
            snapshot
                .surfaces
                .iter()
                .find(|state| state.surface == fixture.application)
                .unwrap()
                .buffer,
            source.buffer()
        );
        assert!(
            snapshot
                .compositor_display_list
                .presentation_stamp()
                .is_none()
        );
    }
    assert_eq!(
        fixture.runtime.present_scheduler.submitted_frame(a),
        Some(crate::LiveProductionNativeFrameId::from_raw(10))
    );
    let replacement = crate::LiveProductionNativeFrameId::from_raw(12);
    let failure =
        crate::LivePreviewFrameFailure::test_failure(b, old_b, 41, 20, fixture.application);
    fixture.target.preview_failures.insert(b, failure);
    fixture.target.recovering.insert(b);
    fixture.target.preview_withdraw_ready = false;
    fixture.target.next = replacement.raw();
    fixture
        .runtime
        .recover_policy_preview_frames(&fixture.scene, &mut fixture.target)
        .unwrap();
    assert_eq!(
        fixture.runtime.present_scheduler.unsubmitted_frame(b),
        Some(old_b)
    );
    fixture.target.preview_withdraw_ready = true;
    fixture
        .runtime
        .recover_policy_preview_frames(&fixture.scene, &mut fixture.target)
        .unwrap();
    assert!(fixture.target.preview_failures.is_empty());
    assert!(
        fixture
            .runtime
            .take_policy_presentation_revocation()
            .is_none()
    );
    assert!(
        !fixture
            .runtime
            .queue_retained_projection(&fixture.scene, &mut fixture.target)
            .unwrap()
    );
    assert_eq!(
        fixture.runtime.present_scheduler.unsubmitted_frame(b),
        Some(replacement)
    );
    fixture
        .runtime
        .present_scheduler
        .mark_output_submitted(b)
        .unwrap();
    let clock = crate::LiveProductionPageFlipRetirement {
        output: a,
        ust: 200,
        msc: 120,
    };
    assert!(
        fixture
            .runtime
            .present_scheduler
            .mark_output_retired(clock)
            .unwrap()
            .is_none()
    );
    assert!(
        fixture
            .runtime
            .present_scheduler
            .mark_output_retired(crate::LiveProductionPageFlipRetirement {
                output: b,
                ust: 250,
                msc: 60
            })
            .unwrap()
            .is_some()
    );
    assert_eq!(
        fixture
            .runtime
            .present_scheduler
            .take_submitted()
            .unwrap()
            .presentation_clock(),
        Some(clock)
    );
    assert!(fixture.runtime.present_scheduler.take_submitted().is_none());
    let following = fixture
        .runtime
        .display_list_for_output(
            b,
            rect(0, 0, 64, 32),
            fixture.runtime.committed_surfaces(),
            &fixture.runtime.presentation_order,
        )
        .unwrap();
    assert_eq!(
        following
            .presentation_stamp()
            .unwrap()
            .publication_generation,
        21
    );
    assert!(following.surface_instances().next().is_some());
    assert_eq!(
        fixture.runtime.image_reads.ready_evictions(),
        vec![neighbour_image]
    );
}

fn software_retirement(
    output: OutputId,
    frame: crate::LiveProductionNativeFrameId,
) -> crate::LiveProductionNativeFrameRetirement {
    crate::LiveProductionNativeFrameRetirement {
        clocks: crate::LiveNativeRetirementClocks::from_samples([
            crate::LiveNativePresentClockSample {
                source: crate::LiveNativePresentClockSource {
                    owner: 9,
                    incarnation: output.raw(),
                },
                ust_usec: 1000 + output.raw(),
                msc: 500 + output.raw(),
            },
        ]),
        output,
        frame,
        submission: frame.raw(),
        direct: false,
        layout_witness: None,
        content: crate::LiveProductionScanoutContent::RetainedMixed {
            frame,
            logical_content_checksum: None,
            nonzero_rgb_pixels: 1,
            requires_retirement: true,
        },
        ust: 1000 + output.raw(),
        msc: 500 + output.raw(),
    }
}

#[test]
fn software_preview_recovery_keeps_frozen_pixels_root_binding_and_clock() {
    for fail_clock_output in [true, false] {
        let mut f = present_scene();
        f.runtime.presentation_order = vec![f.previewed];
        let (transaction, candidate) = f.queue_present(952, Instant::now());
        f.runtime.present_scheduler.pop_front().unwrap();
        f.runtime.surface_content_stream.begin(candidate).unwrap();
        f.runtime
            .queue_software_present_frame(
                &mut f.scene,
                &outputs(),
                vec![crate::LiveProductionSoftwarePresentSubmission {
                    candidate,
                    transaction,
                    surface: f.application,
                    source_size: Size {
                        width: 16,
                        height: 16,
                    },
                    acquire_fence: None,
                    idle_fence: None,
                }],
            )
            .unwrap();
        let a = f.output;
        let b = outputs()[1].id;
        assert!(
            f.runtime
                .stage_software_present_frame(&mut f.target, a)
                .unwrap()
        );
        let root = *f
            .runtime
            .software_present_frames_bound
            .keys()
            .next()
            .unwrap();
        let frames = f.runtime.software_present_frames_bound[&root]
            .frames
            .clone();
        let (failed, other) = if fail_clock_output { (a, b) } else { (b, a) };
        let old = frames[&failed];
        f.runtime
            .observe_software_present_frame_submitted(frames[&other])
            .unwrap();
        assert!(matches!(
            f.runtime
                .settle_software_present_frame(software_retirement(other, frames[&other]))
                .unwrap(),
            software_present::LiveProductionSoftwarePresentSettlement::Waiting
        ));
        commit_cpu_surface(
            &mut f.runtime,
            &mut f.scene,
            f.previewed,
            88,
            2,
            rect(22, 1, 16, 16),
        );
        f.runtime
            .set_policy_presentation(
                Some(published(
                    22,
                    vec![presentation_output(failed, PolicyPresentationMode::Overlay)],
                    vec![shown_instance(failed, 81, 0, f.previewed, rect(8, 8, 8, 8))],
                    vec![],
                )),
                &f.scene,
                None,
            )
            .unwrap();
        let rebuilt = f
            .runtime
            .software_preview_recovery_frames(&f.target, failed, old)
            .unwrap();
        for head in &rebuilt {
            let list = &head
                .frame
                .output_damage_snapshot
                .as_ref()
                .unwrap()
                .compositor_display_list;
            assert!(list.presentation_stamp().is_none());
            assert!(list.surface_instances().next().is_none());
        }
        assert_eq!(
            f.runtime
                .policy_presentation
                .as_ref()
                .unwrap()
                .presentation
                .generation,
            22
        );
        for layer in rebuilt.iter().flat_map(|head| &head.frame.layers) {
            if let sophia_renderer_live::LiveOwnedMixedCompositionLayer::Cpu { buffer, .. } = layer
                && buffer.handle == 88
            {
                assert_eq!(
                    buffer.generation, 1,
                    "recovery must retain the original software pixels"
                );
            }
        }
        let replacement = crate::LiveProductionNativeFrameId::from_raw(100);
        assert!(
            f.runtime
                .replace_software_preview_frame(other, frames[&other], replacement)
                .is_err()
        );
        let failure =
            crate::LivePreviewFrameFailure::test_failure(failed, old, 41, 21, f.previewed);
        f.target.preview_failures.insert(failed, failure);
        f.target.recovering.insert(failed);
        f.target.preview_withdraw_ready = false;
        f.target.next = replacement.raw();
        f.runtime
            .recover_policy_preview_frames(&f.scene, &mut f.target)
            .unwrap();
        assert_eq!(
            f.runtime.software_present_frames_bound[&root].frames[&failed],
            old
        );
        f.target.preview_withdraw_ready = true;
        f.runtime
            .recover_policy_preview_frames(&f.scene, &mut f.target)
            .unwrap();
        assert!(f.target.preview_failures.is_empty());
        assert!(f.runtime.take_policy_presentation_revocation().is_none());
        assert!(!f.runtime.software_present_frame_owners.contains_key(&old));
        assert_eq!(f.runtime.software_present_frame_owners[&replacement], root);
        assert_eq!(
            f.runtime.software_present_frames_bound[&root].clock_output,
            a
        );
        assert!(
            !f.runtime
                .queue_retained_projection(&f.scene, &mut f.target)
                .unwrap()
        );
        f.runtime
            .observe_software_present_frame_submitted(replacement)
            .unwrap();
        let retirement = software_retirement(failed, replacement);
        assert!(matches!(
            f.runtime
                .settle_software_present_frame(retirement.clone())
                .unwrap(),
            software_present::LiveProductionSoftwarePresentSettlement::Settled
        ));
        assert!(matches!(
            f.runtime.settle_software_present_frame(retirement).unwrap(),
            software_present::LiveProductionSoftwarePresentSettlement::NotOwned
        ));
        assert!(f.runtime.software_present_frames_bound.is_empty());
        assert!(f.runtime.software_present_frame_owners.is_empty());
        assert!(!f.runtime.native_publication_blocked());
        let mut receipts = Vec::new();
        f.runtime
            .drain_retired_software_presents_into(&mut receipts)
            .unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].msc, 500 + a.raw());
        assert_eq!(receipts[0].ust_usec, 1000 + a.raw());
        let mut feedback = Vec::new();
        f.runtime
            .drain_present_feedback_into(&mut feedback)
            .unwrap();
        let completion = feedback.iter().find(|outcome| outcome.feedback.iter().any(|item|
            matches!(item, crate::LivePresentProtocolFeedback::Complete { transaction: found, .. } if *found == transaction))).unwrap();
        for output in [a, b] {
            let sample = completion
                .clocks
                .sample(crate::LiveNativePresentClockSource {
                    owner: 9,
                    incarnation: output.raw(),
                })
                .unwrap();
            assert_eq!(sample.msc, 500 + output.raw());
        }
    }
}

#[test]
fn every_output_report_preserves_faults_even_during_preview_recovery() {
    use crate::LiveRendererScanoutBufferExportDetail as D;
    use crate::LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus as S;
    for recovering in [false, true] {
        for detail in [
            D::InvalidRendererImageId,
            D::RendererImageStoreFull,
            D::RetainedBufferMissing,
            D::DmaBufImportCacheFull,
            D::WorkerStalled,
            D::EglMakeCurrentFailed,
        ] {
            let result = super::super::super::native::check_frame_service_submission(
                S::ScanoutExportFailed,
                Some(detail),
                recovering,
            );
            if recovering
                && matches!(
                    detail,
                    D::InvalidRendererImageId | D::RendererImageStoreFull
                )
            {
                assert!(result.is_ok());
            } else {
                let error = result.unwrap_err();
                assert_eq!(error.downcast_ref::<D>(), Some(&detail));
            }
        }
        for status in [
            S::ScanoutExportFailed,
            S::ScanoutTargetNotReady,
            S::FrameTargetUnavailable,
            S::PrimaryPlaneSubmitFailed,
        ] {
            assert!(
                super::super::super::native::check_frame_service_submission(
                    status, None, recovering
                )
                .is_err()
            );
        }
    }
}

#[test]
fn detached_recovery_revokes_only_its_publication_and_rearms_exact_claims() {
    for later in [false, true] {
        let mut f = present_scene();
        f.replace_with(vec![shown_instance(
            f.output,
            1,
            0,
            f.application,
            rect(0, 0, 8, 8),
        )]);
        let publication = f.runtime.policy_presentation().unwrap();
        let failure = crate::LivePreviewFrameFailure::test_failure(
            f.output,
            crate::LiveProductionNativeFrameId::from_raw(80),
            publication.owner_epoch,
            publication.presentation.generation,
            f.application,
        );
        let claim = (f.output, LiveShellContentLayer::Shell);
        let owner = sophia_protocol::ContentGrant {
            connection_epoch: 7,
            content_grant_epoch: 8,
        };
        f.runtime
            .queued_shell_retirements
            .insert((f.output, failure.frame), BTreeMap::from([(claim, owner)]));
        if later {
            f.runtime
                .policy_presentation
                .as_mut()
                .unwrap()
                .presentation
                .generation += 1;
        }
        f.runtime
            .settle_detached_preview_failures([failure])
            .unwrap();
        assert!(f.runtime.queued_shell_retirements.is_empty());
        assert_eq!(f.runtime.retained_projection_retirements[&claim], owner);
        assert_eq!(f.runtime.policy_presentation().is_some(), later);
        assert_eq!(
            f.runtime.take_policy_presentation_revocation().is_some(),
            !later
        );
        // Detach now settles all existing Present/software obligations through
        // its established path. Resume cannot see an abandoned recovery owner.
        f.runtime.settle_detached_preview_failures([]).unwrap();
        assert_eq!(f.runtime.retained_projection_retirements.len(), 1);
    }
}

#[test]
fn detached_settlement_revokes_later_records_even_after_an_earlier_claim_conflict() {
    let mut f = present_scene();
    f.replace_with(vec![shown_instance(
        f.output,
        1,
        0,
        f.application,
        rect(0, 0, 8, 8),
    )]);
    let publication = f.runtime.policy_presentation().unwrap();
    let current = crate::LivePreviewFrameFailure::test_failure(
        f.output,
        crate::LiveProductionNativeFrameId::from_raw(80),
        publication.owner_epoch,
        publication.presentation.generation,
        f.application,
    );
    let first = crate::LivePreviewFrameFailure::test_failure(
        f.output,
        crate::LiveProductionNativeFrameId::from_raw(79),
        publication.owner_epoch,
        publication.presentation.generation + 1,
        f.application,
    );
    let key = (f.output, LiveShellContentLayer::Shell);
    let old = sophia_protocol::ContentGrant {
        connection_epoch: 7,
        content_grant_epoch: 8,
    };
    let new = sophia_protocol::ContentGrant {
        connection_epoch: 9,
        content_grant_epoch: 10,
    };
    f.runtime
        .queued_shell_retirements
        .insert((f.output, first.frame), BTreeMap::from([(key, old)]));
    f.runtime.retained_projection_retirements.insert(key, new);
    assert!(
        f.runtime
            .settle_detached_preview_failures([first, current])
            .is_err()
    );
    assert!(f.runtime.policy_presentation().is_none());
    assert!(f.runtime.take_policy_presentation_revocation().is_some());
}

#[test]
fn forced_detach_revokes_previews_whose_native_owner_is_already_unavailable() {
    let mut f = present_scene();
    f.replace_with(vec![shown_instance(
        f.output,
        1,
        0,
        f.application,
        rect(0, 0, 8, 8),
    )]);
    f.runtime
        .suspend_revoked_native_scanout(&outputs())
        .unwrap();
    assert!(f.runtime.policy_presentation().is_none());
    assert!(f.runtime.take_policy_presentation_revocation().is_some());
    assert!(f.runtime.retained_projection_pending);
}
