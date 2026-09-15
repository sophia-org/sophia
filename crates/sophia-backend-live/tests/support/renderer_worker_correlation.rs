// Included by the worker-private test harness; all jobs use channels without a GPU.

fn correlated_facade() -> (
    super::NativeGbmRendererWorker,
    Receiver<WorkerCommand>,
    SyncSender<WorkerResult>,
) {
    let (core, commands, resume) = inventory_test_core(32);
    resume.send(()).unwrap();
    let facade = core.attach(LiveRendererWorkerOutputKey::from_raw(701));
    let WorkerCommand::Register { reply, .. } = commands.recv().unwrap() else {
        panic!("facade did not register its reply channel");
    };
    (facade, commands, reply)
}

fn mixed_job(generation: u64) -> super::PendingRenderedFrame {
    super::PendingRenderedFrame::Mixed(sophia_renderer_live::LiveOwnedMixedCompositionFrame {
        trace: Some(sophia_renderer_live::LiveCompositionTrace {
            output: sophia_protocol::OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(17),
            scene_generation: generation,
        }),
        direct_scanout: sophia_engine::DirectScanoutVerdict::Eligible,
        ..Default::default()
    }, None)
}

fn submit_mixed_job(
    facade: &mut super::NativeGbmRendererWorker,
    commands: &Receiver<WorkerCommand>,
    generation: u64,
) -> (
    super::LiveRendererFrameCorrelation,
    super::PendingRenderedFrame,
) {
    submit_owned_job(facade, commands, mixed_job(generation))
}

fn submit_owned_job(
    facade: &mut super::NativeGbmRendererWorker,
    commands: &Receiver<WorkerCommand>,
    frame: super::PendingRenderedFrame,
) -> (super::LiveRendererFrameCorrelation, super::PendingRenderedFrame) {
    facade
        .submit(
            LiveGbmEglFrameTargetRecord::new(Size {
                width: 16,
                height: 16,
            }),
            frame,
            Vec::new(),
            None,
        )
        .unwrap();
    let WorkerCommand::Render {
        request_id, frame, ..
    } = commands.recv().unwrap()
    else {
        panic!("accepted submission did not send a render");
    };
    let correlation = super::frame_correlation(&frame, Some(request_id));
    assert_eq!(correlation.request, Some(request_id));
    assert_eq!(facade.in_flight_correlation(), Some(correlation));
    (correlation, frame)
}

fn correlated_result(
    correlation: super::LiveRendererFrameCorrelation,
    outcome: WorkerOutcome,
) -> WorkerResult {
    WorkerResult {
        output: LiveRendererWorkerOutputKey::from_raw(701),
        request_id: correlation.request.unwrap(),
        correlation,
        context_status: super::NativeGbmRenderedScanoutContextStatus::Ready,
        persistent_render_stats: super::LiveNativePersistentRenderStats::default(),
        composition_nonzero_rgb_pixels: 0,
        outcome,
    }
}

fn exported_outcome() -> WorkerOutcome {
    let mut slots = super::LiveRendererFrameSlotPool::new();
    let crate::LiveRendererFrameSlotAcquire::Acquired(slot_token) = slots.try_acquire() else {
        panic!("empty slot pool refused a lease");
    };
    WorkerOutcome::Exported {
        descriptor: super::LiveRendererScanoutBufferDescriptor::new(
            Size {
                width: 16,
                height: 16,
            },
            64,
            sophia_renderer_live::LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            1,
        ),
        lease_id: super::LiveRendererWorkerLeaseId(1),
        slot_token,
    }
}

#[test]
fn a_delayed_lease_keeps_its_submission_when_a_newer_frame_is_pending() {
    let (mut facade, commands, results) = correlated_facade();
    let (first, _) = submit_mixed_job(&mut facade, &commands, 41);
    let newer = mixed_job(42);
    assert_ne!(first.trace, super::frame_correlation(&newer, None).trace);
    assert!(matches!(facade.poll(), super::WorkerPoll::Pending { .. }));
    results
        .send(correlated_result(first, exported_outcome()))
        .unwrap();
    let super::WorkerPoll::Exported(lease) = facade.poll() else {
        panic!("matching completion was not leased");
    };
    assert_eq!(lease.correlation(), first);
    assert_eq!(facade.in_flight_correlation(), None);
    let lease_id = lease.lease_id();
    let slot_token = lease.slot_token();
    drop(lease);
    assert!(
        matches!(commands.recv().unwrap(), WorkerCommand::Release { output, lease_id: released, slot_token: slot }
        if output == facade.output && released == lease_id && slot == slot_token)
    );
    let (second, _) = submit_mixed_job(&mut facade, &commands, 42);
    assert_ne!(second.request, first.request);
    assert_ne!(second.trace, first.trace);
    assert_eq!(second.trace, super::frame_correlation(&newer, None).trace);
}

#[test]
fn a_deferred_frame_reoffered_with_the_same_trace_gets_a_new_request_identity() {
    let (mut facade, commands, results) = correlated_facade();
    let (first, frame) = submit_mixed_job(&mut facade, &commands, 43);
    results
        .send(correlated_result(first, WorkerOutcome::Deferred(frame)))
        .unwrap();
    let super::WorkerPoll::Deferred(frame) = facade.poll() else {
        panic!("slot deferral lost its frame");
    };
    assert_eq!(super::frame_correlation(&frame, first.request), first);
    assert_eq!(facade.in_flight_correlation(), None);
    let (second, _) = submit_mixed_job(&mut facade, &commands, 43);
    assert_eq!(second.trace, first.trace);
    assert_eq!(second.direct_scanout, first.direct_scanout);
    assert_ne!(second.request, first.request);
    results
        .send(correlated_result(first, exported_outcome()))
        .unwrap();
    assert!(matches!(
        facade.poll(),
        super::WorkerPoll::Failed(super::LiveRendererScanoutBufferExportDetail::WorkerDisconnected)
    ));
    assert!(
        facade.quarantined,
        "an earlier attempt cannot settle its replacement"
    );
}

#[test]
fn failed_and_disconnected_jobs_name_the_accepted_frame() {
    let (mut facade, commands, results) = correlated_facade();
    let (first, _) = submit_mixed_job(&mut facade, &commands, 44);
    let detail = super::LiveRendererScanoutBufferExportDetail::BackendDeviceUnavailable;
    results
        .send(correlated_result(first, WorkerOutcome::Failed(detail)))
        .unwrap();
    assert!(matches!(facade.poll(), super::WorkerPoll::Failed(observed) if observed == detail));
    let (second, _) = submit_mixed_job(&mut facade, &commands, 45);
    assert_eq!(facade.in_flight_correlation(), Some(second));
    drop(results);
    assert!(matches!(
        facade.poll(),
        super::WorkerPoll::Failed(super::LiveRendererScanoutBufferExportDetail::WorkerDisconnected)
    ));
    assert_eq!(facade.in_flight_correlation(), None);
}

#[test]
fn a_hard_stalled_job_cannot_assign_its_late_result_to_another_frame() {
    let (mut facade, commands, results) = correlated_facade();
    let (first, _) = submit_mixed_job(&mut facade, &commands, 46);
    facade.in_flight.as_mut().unwrap().submitted_at =
        std::time::Instant::now() - super::LIVE_RENDERER_WORKER_HARD_STALL;
    assert!(matches!(facade.poll(), super::WorkerPoll::HardStalled(_)));
    assert_eq!(facade.in_flight_correlation(), None);
    results
        .send(correlated_result(first, exported_outcome()))
        .unwrap();
    assert_eq!(
        facade.submit(
            LiveGbmEglFrameTargetRecord::new(Size {
                width: 16,
                height: 16
            }),
            mixed_job(47),
            Vec::new(),
            None,
        ),
        Err(super::LiveRendererScanoutBufferExportDetail::WorkerPending)
    );
    assert!(matches!(facade.poll(), super::WorkerPoll::Idle));
    assert!(
        matches!(commands.recv().unwrap(), WorkerCommand::Release { output, lease_id, .. } if output == facade.output && lease_id == super::LiveRendererWorkerLeaseId(1))
    );
    assert!(
        commands.try_recv().is_err(),
        "quarantined facade accepted another render"
    );
}

#[test]
fn the_result_must_preserve_output_trace_and_scanout_verdict() {
    for change in 0..7 {
        let (mut facade, commands, results) = correlated_facade();
        let (expected, _) = submit_mixed_job(&mut facade, &commands, 48);
        let mut result = correlated_result(expected, exported_outcome());
        match change {
            0 => result.output = LiveRendererWorkerOutputKey::from_raw(702),
            1 => result.correlation.trace.as_mut().unwrap().scene_generation += 1,
            2 => {
                result.correlation.direct_scanout =
                    Some(sophia_engine::DirectScanoutVerdict::LayerNotActive)
            }
            3 => result.correlation.request = Some(LiveRendererWorkerRequestId(900)),
            4 => {
                result.correlation.trace.as_mut().unwrap().head =
                    sophia_engine::RenderHeadId::from_raw(18)
            }
            5 => {
                result.correlation.trace.as_mut().unwrap().output =
                    sophia_protocol::OutputId::from_raw(8)
            }
            6 => result.correlation.trace = None,
            _ => unreachable!(),
        }
        let WorkerOutcome::Exported {
            lease_id,
            slot_token,
            ..
        } = result.outcome
        else {
            unreachable!()
        };
        let released_output = result.output;
        results.send(result).unwrap();
        assert!(matches!(
            facade.poll(),
            super::WorkerPoll::Failed(
                super::LiveRendererScanoutBufferExportDetail::WorkerDisconnected
            )
        ));
        assert!(facade.quarantined);
        assert!(
            matches!(commands.recv().unwrap(), WorkerCommand::Release { output, lease_id: released, slot_token: slot }
            if output == released_output && released == lease_id && slot == slot_token)
        );
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn a_deferred_result_cannot_substitute_a_different_frame() {
    let (mut facade, commands, results) = correlated_facade();
    let (expected, _) = submit_mixed_job(&mut facade, &commands, 49);
    results
        .send(correlated_result(
            expected,
            WorkerOutcome::Deferred(mixed_job(50)),
        ))
        .unwrap();
    assert!(matches!(
        facade.poll(),
        super::WorkerPoll::Failed(super::LiveRendererScanoutBufferExportDetail::WorkerDisconnected)
    ));
}

#[test]
fn the_service_derives_result_identity_from_the_owned_frame() {
    let (commands, thread) = worker_channel();
    let output = LiveRendererWorkerOutputKey::from_raw(703);
    let results = register(&commands, output);
    let request_id = LiveRendererWorkerRequestId(51);
    let correlation = super::frame_correlation(&mixed_job(51), Some(request_id));
    commands
        .send(WorkerCommand::Render {
            output,
            request_id,
            target: LiveGbmEglFrameTargetRecord::new(Size {
                width: 16,
                height: 16,
            }),
            frame: mixed_job(51),
            preferred_modifiers: Vec::new(),
            output_format: None,
        })
        .unwrap();
    let result = results.recv_timeout(SETTLE).unwrap();
    assert_eq!(result.correlation, correlation);
    assert!(matches!(
        result.outcome,
        WorkerOutcome::Failed(
            super::LiveRendererScanoutBufferExportDetail::BackendDeviceUnavailable
        )
    ));
    drop(commands);
    thread.join().unwrap();
}

#[test]
fn exhausted_request_identity_refuses_before_queueing_another_job() {
    let (mut facade, commands, _results) = correlated_facade();
    facade.next_request_id = u64::MAX;
    assert_eq!(
        facade.submit(
            LiveGbmEglFrameTargetRecord::new(Size {
                width: 16,
                height: 16
            }),
            mixed_job(53),
            Vec::new(),
            None,
        ),
        Err(super::LiveRendererScanoutBufferExportDetail::WorkerDisconnected)
    );
    assert_eq!(facade.in_flight_correlation(), None);
    assert!(commands.try_recv().is_err());
}

#[test]
fn a_full_queue_retains_discarded_lease_cleanup_until_it_can_be_sent() {
    let (mut facade, commands, results) = correlated_facade();
    let (accepted, _) = submit_mixed_job(&mut facade, &commands, 54);
    let mut stale = correlated_result(accepted, exported_outcome());
    stale.request_id = LiveRendererWorkerRequestId(900);
    let WorkerOutcome::Exported {
        lease_id,
        slot_token,
        ..
    } = stale.outcome
    else {
        unreachable!()
    };
    results.send(stale).unwrap();
    for _ in 0..32 {
        facade
            .core
            .command_sender
            .try_send(WorkerCommand::Shutdown)
            .unwrap();
    }
    assert!(matches!(facade.poll(), super::WorkerPoll::Failed(_)));
    assert!(facade.discarded_release.is_some());
    assert!(matches!(facade.poll(), super::WorkerPoll::Idle));
    assert!(
        facade.discarded_release.is_some(),
        "another full queue discarded cleanup debt"
    );
    for _ in 0..32 {
        assert!(matches!(commands.recv().unwrap(), WorkerCommand::Shutdown));
    }
    assert!(matches!(facade.poll(), super::WorkerPoll::Idle));
    assert!(facade.discarded_release.is_none());
    assert!(
        matches!(commands.recv().unwrap(), WorkerCommand::Release { output, lease_id: released, slot_token: slot }
        if output == facade.output && released == lease_id && slot == slot_token)
    );
    assert!(matches!(facade.poll(), super::WorkerPoll::Idle));
    assert!(
        commands.try_recv().is_err(),
        "cleanup was sent more than once"
    );
}

#[test]
fn exporter_latest_wins_does_not_relabel_an_earlier_completed_lease() {
    let (core, commands, resume) = inventory_test_core(32);
    resume.send(()).unwrap();
    let mut exporter =
        crate::NativeGbmRenderedScanoutBufferDiscoveryExporter::new(MissingWorkerDevice);
    exporter.attach_shared_worker(&core);
    let WorkerCommand::Register { output, reply, .. } = commands.recv().unwrap() else {
        panic!("missing worker registration")
    };
    let target = LiveGbmEglFrameTargetRecord::new(Size {
        width: 16,
        height: 16,
    });
    let super::PendingRenderedFrame::Mixed(first, _) = mixed_job(60) else {
        unreachable!()
    };
    exporter.set_pending_mixed_frame(first);
    assert!(
        exporter
            .export_rendered_scanout_buffer(target)
            .correlation
            .is_none()
    );
    let WorkerCommand::Render {
        request_id, frame, ..
    } = commands.recv().unwrap()
    else {
        panic!("missing first job")
    };
    let accepted = super::frame_correlation(&frame, Some(request_id));
    assert_eq!(exporter.rendering_frame_correlation(), Some(accepted));
    let super::PendingRenderedFrame::Mixed(newer, _) = mixed_job(61) else {
        unreachable!()
    };
    exporter.set_pending_mixed_frame(newer);
    assert_eq!(exporter.rendering_frame_correlation(), Some(accepted));
    let mut result = correlated_result(accepted, exported_outcome());
    result.output = output;
    reply.send(result).unwrap();
    let completed = exporter.export_rendered_scanout_buffer(target);
    assert_eq!(
        completed.status,
        super::LiveRendererScanoutBufferExportStatus::Exported
    );
    assert_eq!(completed.correlation, Some(accepted));
    assert!(completed.owner.is_some());
    drop(completed);
    assert!(
        matches!(commands.recv().unwrap(), WorkerCommand::Release { output: released, .. } if released == output)
    );
    assert!(
        exporter
            .export_rendered_scanout_buffer(target)
            .correlation
            .is_none()
    );
    let WorkerCommand::Render {
        request_id, frame, ..
    } = commands.recv().unwrap()
    else {
        panic!("newer job was lost")
    };
    let accepted_newer = super::frame_correlation(&frame, Some(request_id));
    assert_eq!(accepted_newer.trace.unwrap().scene_generation, 61);
    assert_ne!(accepted_newer.request, accepted.request);
    let mut failure = correlated_result(
        accepted_newer,
        WorkerOutcome::Failed(
            super::LiveRendererScanoutBufferExportDetail::BackendDeviceUnavailable,
        ),
    );
    failure.output = output;
    reply.send(failure).unwrap();
    let failed = exporter.export_rendered_scanout_buffer(target);
    assert_eq!(
        failed.status,
        super::LiveRendererScanoutBufferExportStatus::Degraded
    );
    assert!(failed.owner.is_none());
    assert!(failed.correlation.is_none());
    assert_eq!(exporter.rendering_frame_correlation(), None);
}

#[test]
fn a_correlation_cannot_survive_loss_of_an_export_owner_or_descriptor() {
    let correlation = super::frame_correlation(&mixed_job(62), None);
    let WorkerOutcome::Exported { descriptor, .. } = exported_outcome() else {
        unreachable!()
    };
    for (descriptor, owner) in [(Some(descriptor), None), (None, Some(()))] {
        let export = crate::LiveRenderedScanoutBufferExport::new(
            super::LiveRendererScanoutBufferExportStatus::Exported,
            super::LiveRendererScanoutBufferExportDetail::Exported,
            descriptor,
            owner,
        )
        .with_correlation(Some(correlation));
        assert_eq!(
            export.status,
            super::LiveRendererScanoutBufferExportStatus::Degraded
        );
        assert!(export.correlation.is_none());
        assert!(export.normalized().correlation.is_none());
    }
}

#[test]
fn output_format_requests_cross_the_worker_boundary_without_relabeling() {
    use sophia_protocol::{DRM_FORMAT_ARGB8888, DRM_FORMAT_XRGB8888};
    use sophia_renderer_live::LiveCompositionFormatRequest::{Preferred, Required};

    for (request, actual, admitted) in [
        (Required(DRM_FORMAT_ARGB8888), DRM_FORMAT_ARGB8888, true),
        (Required(DRM_FORMAT_ARGB8888), DRM_FORMAT_XRGB8888, false),
        (Preferred(DRM_FORMAT_ARGB8888), DRM_FORMAT_ARGB8888, true),
        (Preferred(DRM_FORMAT_ARGB8888), DRM_FORMAT_XRGB8888, true),
    ] {
        let (mut facade, commands, results) = correlated_facade();
        facade
            .submit(
                LiveGbmEglFrameTargetRecord::new(Size {
                    width: 16,
                    height: 16,
                }),
                mixed_job(71),
                Vec::new(),
                Some(request),
            )
            .unwrap();
        let WorkerCommand::Render {
            request_id,
            frame,
            output_format,
            ..
        } = commands.recv().unwrap()
        else {
            panic!("missing accepted render command");
        };
        assert_eq!(output_format, Some(request));
        let correlation = super::frame_correlation(&frame, Some(request_id));
        let mut outcome = exported_outcome();
        let WorkerOutcome::Exported {
            descriptor,
            lease_id,
            slot_token,
        } = &mut outcome
        else {
            unreachable!()
        };
        descriptor.format = actual;
        let (expected_lease, expected_slot) = (*lease_id, *slot_token);
        results
            .send(correlated_result(correlation, outcome))
            .unwrap();
        match facade.poll() {
            super::WorkerPoll::Exported(lease) => {
                assert!(admitted, "required format accepted a different allocation");
                assert_eq!(lease.descriptor().format, actual);
                assert_eq!(lease.correlation(), correlation);
                drop(lease);
            }
            super::WorkerPoll::Failed(
                super::LiveRendererScanoutBufferExportDetail::WorkerDisconnected,
            ) => {
                assert!(
                    !admitted,
                    "an optional format preference blocked ordinary rendering"
                );
            }
            _ => panic!("completion neither admitted nor refused"),
        }
        assert!(
            matches!(commands.recv().unwrap(), WorkerCommand::Release { lease_id, slot_token, .. }
            if lease_id == expected_lease && slot_token == expected_slot)
        );
        assert_eq!(facade.in_flight_correlation(), None);
    }
}


fn native_job(identity: crate::LiveNativeFrameIdentity) -> super::PendingRenderedFrame {
    let super::PendingRenderedFrame::Mixed(frame, _) = mixed_job(41) else { unreachable!() };
    super::PendingRenderedFrame::Mixed(frame, Some(identity))
}

#[test]
fn native_identity_survives_identical_pixels_pending_and_same_frame_retry() {
    let owner = crate::NativeFrameOwner::new();
    let output = sophia_protocol::OutputId::from_raw(7);
    let head = sophia_engine::RenderHeadId::from_raw(17);
    let a = owner.frame(output, head, 1, 1);
    let b = owner.frame(output, head, 1, 2);
    let (mut facade, commands, results) = correlated_facade();
    let (first, frame) = submit_owned_job(&mut facade, &commands, native_job(a));
    let pending = native_job(b);
    assert_eq!(first.trace, super::frame_correlation(&pending, None).trace);
    assert_ne!(first.native, super::frame_correlation(&pending, None).native);
    results.send(correlated_result(first, WorkerOutcome::Deferred(frame))).unwrap();
    let super::WorkerPoll::Deferred(frame) = facade.poll() else { panic!("deferred owner missing") };
    let (retry, _) = submit_owned_job(&mut facade, &commands, frame);
    assert_eq!(retry.native, Some(a));
    assert_ne!(retry.request, first.request);
    results.send(correlated_result(retry, exported_outcome())).unwrap();
    let super::WorkerPoll::Exported(lease) = facade.poll() else { panic!("exact retry missing") };
    assert_eq!(lease.correlation().native, Some(a));
    drop(lease);
    assert!(matches!(commands.recv().unwrap(), WorkerCommand::Release { .. }));
    let (next, _) = submit_owned_job(&mut facade, &commands, pending);
    assert_eq!(next.native, Some(b));
    assert_eq!(next.trace, retry.trace);
}

#[test]
fn wrong_head_or_reconstructed_native_owner_cannot_complete_the_current_job() {
    let owner = crate::NativeFrameOwner::new();
    let output = sophia_protocol::OutputId::from_raw(7);
    let head = sophia_engine::RenderHeadId::from_raw(17);
    let current = owner.frame(output, head, 1, 1);
    for wrong in [
        owner.frame(output, sophia_engine::RenderHeadId::from_raw(18), 1, 1),
        crate::NativeFrameOwner::new().frame(output, head, 1, 1),
        owner.frame(output, head, 2, 1),
    ] {
        let (mut facade, commands, results) = correlated_facade();
        let (mut result, _) = submit_owned_job(&mut facade, &commands, native_job(current));
        result.native = Some(wrong);
        results.send(correlated_result(result, exported_outcome())).unwrap();
        assert!(matches!(facade.poll(), super::WorkerPoll::Failed(
            super::LiveRendererScanoutBufferExportDetail::WorkerDisconnected
        )));
        assert!(facade.quarantined);
    }
}
