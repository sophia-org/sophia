// Drive the real exporter and worker facade with a controlled reply channel.
// The render device is unavailable; no graphics work is performed.

#[test]
fn exporter_retains_a_late_frame_with_and_without_a_newer_workspace_frame() {
    for newer_pending in [false, true] {
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
        let super::PendingRenderedFrame::Mixed(first, _) = mixed_job(80) else {
            unreachable!()
        };
        let native_owner = crate::NativeFrameOwner::new();
        let first_native = native_owner.frame(
            sophia_protocol::OutputId::from_raw(7),
            sophia_engine::RenderHeadId::from_raw(17),
            1,
            80,
        );
        exporter.set_pending_identified_mixed_frame(first, Some(first_native));
        assert_eq!(
            exporter.export_rendered_scanout_buffer(target).status,
            super::LiveRendererScanoutBufferExportStatus::Pending
        );
        let WorkerCommand::Render {
            request_id, frame, ..
        } = commands.recv().unwrap()
        else {
            panic!("missing render request")
        };
        let accepted = super::frame_correlation(&frame, Some(request_id));
        // The exporter deliberately exposes no test-only clock or worker
        // mutation. Hold its reply beyond the real hard-stall threshold.
        std::thread::sleep(super::LIVE_RENDERER_WORKER_HARD_STALL + Duration::from_millis(20));
        assert_eq!(
            exporter.export_rendered_scanout_buffer(target).status,
            super::LiveRendererScanoutBufferExportStatus::Pending
        );
        assert!(exporter.worker_in_flight());
        if newer_pending {
            let super::PendingRenderedFrame::Mixed(newer, _) = mixed_job(81) else {
                unreachable!()
            };
            let newer_native =
                native_owner.frame(first_native.output(), first_native.head(), 1, 81);
            exporter.set_pending_identified_mixed_frame(newer, Some(newer_native));
        }
        let mut result = correlated_result(accepted, exported_outcome());
        result.output = output;
        reply.send(result).unwrap();
        let completed = exporter.export_rendered_scanout_buffer(target);
        assert_eq!(
            completed.status,
            super::LiveRendererScanoutBufferExportStatus::Exported,
            "a completed accepted frame must survive the stall; newer_pending={newer_pending}, detail={:?}",
            completed.detail
        );
        assert_eq!(completed.correlation, Some(accepted));
        assert_eq!(completed.correlation.unwrap().native, Some(first_native));
        assert!(completed.owner.is_some());
        assert_eq!(exporter.mixed_frame_exports(), 1);
        assert_eq!(exporter.pending_frame(), newer_pending);
        drop(completed);
        assert!(
            matches!(commands.recv().unwrap(), WorkerCommand::Release { output: released, .. } if released == output)
        );
        assert!(
            commands.try_recv().is_err(),
            "the old completion must not submit a newer frame in its place"
        );
        if newer_pending {
            assert_eq!(
                exporter.export_rendered_scanout_buffer(target).status,
                super::LiveRendererScanoutBufferExportStatus::Pending
            );
            let WorkerCommand::Render {
                request_id, frame, ..
            } = commands.recv().unwrap()
            else {
                panic!("new workspace frame was lost")
            };
            let next = super::frame_correlation(&frame, Some(request_id));
            assert_ne!(next.request, accepted.request);
            assert_eq!(next.trace.unwrap().scene_generation, 81);
            assert_eq!(next.native.unwrap().frame(), 81);
        }
    }
}

#[test]
fn stalled_completions_still_validate_output_request_trace_and_required_format() {
    for change in 0..4 {
        let (mut facade, commands, results) = correlated_facade();
        let (expected, _) = submit_mixed_job(&mut facade, &commands, 82);
        let accepted = facade.in_flight.as_mut().unwrap();
        accepted.output_format = Some(
            sophia_renderer_live::LiveCompositionFormatRequest::Required(
                sophia_renderer_live::LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            ),
        );
        accepted.submitted_at = std::time::Instant::now() - super::LIVE_RENDERER_WORKER_HARD_STALL;
        assert!(matches!(facade.poll(), super::WorkerPoll::HardStalled(_)));
        let mut result = correlated_result(expected, exported_outcome());
        match change {
            0 => result.output = LiveRendererWorkerOutputKey::from_raw(702),
            1 => result.request_id = LiveRendererWorkerRequestId(999),
            2 => result.correlation.trace.as_mut().unwrap().scene_generation += 1,
            3 => {
                let WorkerOutcome::Exported { descriptor, .. } = &mut result.outcome else {
                    unreachable!()
                };
                descriptor.format = 0;
            }
            _ => unreachable!(),
        }
        let output = result.output;
        results.send(result).unwrap();
        assert!(matches!(
            facade.poll(),
            super::WorkerPoll::Failed(
                super::LiveRendererScanoutBufferExportDetail::WorkerDisconnected
            )
        ));
        assert!(facade.quarantined);
        assert!(!facade.in_flight());
        assert_eq!(facade.metrics().stall_recoveries, 0);
        assert!(matches!(
            commands.recv_timeout(Duration::from_secs(1)).unwrap(),
            WorkerCommand::Release { output: released, .. } if released == output
        ));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn a_stalled_deferred_frame_keeps_its_identity_and_can_be_submitted_again() {
    let (mut facade, commands, results) = correlated_facade();
    let (expected, frame) = submit_mixed_job(&mut facade, &commands, 83);
    facade.in_flight.as_mut().unwrap().submitted_at =
        std::time::Instant::now() - super::LIVE_RENDERER_WORKER_HARD_STALL;
    assert!(matches!(facade.poll(), super::WorkerPoll::HardStalled(_)));
    results
        .send(correlated_result(expected, WorkerOutcome::Deferred(frame)))
        .unwrap();
    let super::WorkerPoll::Deferred(frame) = facade.poll() else {
        panic!("the accepted deferred frame was lost")
    };
    assert_eq!(super::frame_correlation(&frame, expected.request), expected);
    assert!(!facade.in_flight());
    let (resubmitted, _) = submit_owned_job(&mut facade, &commands, frame);
    assert_eq!(resubmitted.trace, expected.trace);
    assert_ne!(resubmitted.request, expected.request);
}

#[test]
fn an_abandoned_completion_is_released_and_cannot_revive_the_worker() {
    let (mut facade, commands, results) = correlated_facade();
    let (expected, _) = submit_mixed_job(&mut facade, &commands, 84);
    facade.in_flight.as_mut().unwrap().submitted_at =
        std::time::Instant::now() - super::LIVE_RENDERER_WORKER_HARD_STALL;
    assert!(matches!(facade.poll(), super::WorkerPoll::HardStalled(_)));
    facade.stalled.as_mut().unwrap().since =
        std::time::Instant::now() - super::LIVE_RENDERER_WORKER_STALL_ABANDON;
    assert!(matches!(
        facade.poll(),
        super::WorkerPoll::Failed(super::LiveRendererScanoutBufferExportDetail::WorkerStalled)
    ));
    results
        .send(correlated_result(expected, exported_outcome()))
        .unwrap();
    assert!(matches!(facade.poll(), super::WorkerPoll::Idle));
    assert!(facade.quarantined);
    assert!(!facade.in_flight());
    assert_eq!(facade.metrics().stall_recoveries, 0);
    assert!(matches!(
        commands.recv_timeout(Duration::from_secs(1)).unwrap(),
        WorkerCommand::Release { output, .. } if output == facade.output
    ));
}
