mod frame_correlation {
    use super::*;
    use sophia_backend_live::LiveRendererFrameCorrelation;
    use sophia_renderer_live::LiveCompositionTrace;
    use std::{cell::Cell, rc::Rc};

    fn metadata(generation: u64) -> LiveRendererFrameCorrelation {
        LiveRendererFrameCorrelation {
            native: None,
            request: None,
            trace: Some(LiveCompositionTrace {
                output: OutputId::from_raw(7),
                head: RenderHeadId::from_raw(9),
                scene_generation: generation,
            }),
            direct_scanout: Some(sophia_engine::DirectScanoutVerdict::CompositionRequired(
                "refused",
            )),
        }
    }

    #[derive(Debug)]
    struct Owner(Rc<Cell<usize>>);

    impl Drop for Owner {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    impl LiveRenderedScanoutBufferPrimeSource for Owner {
        fn shares_kms_drm_file(&self) -> bool {
            true
        }
        fn export_scanout_dma_buf_fds(&self) -> io::Result<Option<LiveRenderedScanoutDmaBufFds>> {
            Ok(None)
        }
    }

    struct Exporter {
        pending: Option<LiveRenderedScanoutBufferExport<Owner>>,
    }

    impl LiveRenderedScanoutBufferExporter for Exporter {
        type Owner = Owner;
        fn export_rendered_scanout_buffer(
            &mut self,
            _: LiveGbmEglFrameTargetRecord,
        ) -> LiveRenderedScanoutBufferExport<Owner> {
            self.pending
                .take()
                .expect("one offered export per preparation")
        }
    }

    fn owned_export(
        size: Size,
        generation: u64,
        drops: &Rc<Cell<usize>>,
    ) -> LiveRenderedScanoutBufferExport<Owner> {
        LiveRenderedScanoutBufferExport::new(
            LiveRendererScanoutBufferExportStatus::Exported,
            LiveRendererScanoutBufferExportDetail::from_status(
                LiveRendererScanoutBufferExportStatus::Exported,
            ),
            Some(scanout_descriptor(size)),
            Some(Owner(drops.clone())),
        )
        .with_correlation(Some(metadata(generation)))
    }

    #[test]
    fn normalization_clears_frame_metadata_without_an_owned_successful_export() {
        let size = Size {
            width: 1280,
            height: 720,
        };
        for (status, has_descriptor, has_owner) in [
            (
                LiveRendererScanoutBufferExportStatus::Unavailable,
                true,
                true,
            ),
            (LiveRendererScanoutBufferExportStatus::Degraded, true, true),
            (LiveRendererScanoutBufferExportStatus::Pending, true, true),
            (
                LiveRendererScanoutBufferExportStatus::InvalidTarget,
                true,
                true,
            ),
            (LiveRendererScanoutBufferExportStatus::Exported, true, false),
            (LiveRendererScanoutBufferExportStatus::Exported, false, true),
            (
                LiveRendererScanoutBufferExportStatus::Exported,
                false,
                false,
            ),
        ] {
            for normalize in [false, true] {
                let drops = Rc::new(Cell::new(0));
                // Public fields can carry stale metadata; normalization must revalidate ownership.
                let export = LiveRenderedScanoutBufferExport {
                    status,
                    detail: LiveRendererScanoutBufferExportDetail::from_status(status),
                    descriptor: has_descriptor.then(|| scanout_descriptor(size)),
                    owner: has_owner.then(|| Owner(drops.clone())),
                    correlation: Some(metadata(41)),
                };
                let checked = if normalize {
                    export.normalized()
                } else {
                    export.with_correlation(Some(metadata(41)))
                };
                assert_eq!(
                    checked.correlation, None,
                    "{status:?}, descriptor={has_descriptor}, owner={has_owner}, normalized={normalize}"
                );
                if normalize {
                    assert!(checked.owner.is_none());
                    assert!(checked.descriptor.is_none());
                    assert_eq!(drops.get(), usize::from(has_owner));
                }
                drop(checked);
                assert_eq!(drops.get(), usize::from(has_owner));
            }
        }
        let drops = Rc::new(Cell::new(0));
        let valid = owned_export(size, 41, &drops).normalized();
        assert_eq!(valid.correlation, Some(metadata(41)));
        assert!(valid.owner.is_some());
        assert!(valid.descriptor.is_some());
        assert_eq!(drops.get(), 0);
        drop(valid);
        assert_eq!(drops.get(), 1);
    }

    #[test]
    fn prepared_frame_keeps_export_metadata_when_a_newer_frame_is_offered() {
        let device = full_primary_plane_scanout_device();
        let size = Size {
            width: 1280,
            height: 720,
        };
        let first_drops = Rc::new(Cell::new(0));
        let second_drops = Rc::new(Cell::new(0));
        let mut exporter = Exporter {
            pending: Some(owned_export(size, 41, &first_drops)),
        };
        let mut prepare = prepare_rendered_primary_plane_scanout_from_target_and_selection_with(
            LiveKmsScanoutTargetStatus::Ready,
            Some(LiveGbmEglFrameTargetRecord::new(size)),
            select_native_primary_plane_target(&device),
            None,
            &device,
            &mut exporter,
        );
        assert_eq!(
            prepare.status,
            LiveRenderedPrimaryPlaneScanoutPrepareStatus::Prepared
        );
        let first = prepare.prepared.take().expect("first framebuffer prepared");
        assert_eq!(first.correlation(), Some(metadata(41)));
        assert_eq!(first_drops.get(), 0);

        exporter.pending = Some(owned_export(size, 42, &second_drops));
        assert_eq!(first.correlation(), Some(metadata(41)));
        let second = prepare_rendered_primary_plane_scanout_from_target_and_selection_with(
            LiveKmsScanoutTargetStatus::Ready,
            Some(LiveGbmEglFrameTargetRecord::new(size)),
            select_native_primary_plane_target(&device),
            None,
            &device,
            &mut exporter,
        )
        .prepared
        .expect("newly offered framebuffer prepared separately");
        assert_eq!(second.correlation(), Some(metadata(42)));
        assert_eq!(first.correlation(), Some(metadata(41)));
        assert_eq!(device.commits.get(), 0);

        let cancelled = cancel_prepared_rendered_primary_plane_scanout(&device, first);
        assert_eq!(
            cancelled.destroy,
            LibdrmNativePrimaryPlaneResourceDestroyStatus::Destroyed
        );
        assert!(cancelled.cleanup.is_none());
        assert_eq!(device.resources.destroyed_framebuffers.get(), 1);
        assert_eq!(first_drops.get(), 1);
        assert_eq!(second_drops.get(), 0);
        assert_eq!(second.correlation(), Some(metadata(42)));
        let cancelled = cancel_prepared_rendered_primary_plane_scanout(&device, second);
        assert_eq!(
            cancelled.destroy,
            LibdrmNativePrimaryPlaneResourceDestroyStatus::Destroyed
        );
        assert!(cancelled.cleanup.is_none());
        assert_eq!(device.resources.destroyed_framebuffers.get(), 2);
        assert_eq!(first_drops.get(), 1);
        assert_eq!(second_drops.get(), 1);
        assert_eq!(device.resources.imported_buffers.get(), 0);
        assert_eq!(device.resources.closed_buffers.get(), 0);
        assert_eq!(device.commits.get(), 0);
    }

    fn prepare_owned(
        device: &FakeNativePrimaryPlaneScanoutDevice,
        drops: &Rc<Cell<usize>>,
        topology: bool,
    ) -> sophia_backend_live::LivePreparedRenderedPrimaryPlaneScanout<Owner> {
        let size = Size { width: 1280, height: 720 };
        let mut exporter = Exporter { pending: Some(owned_export(size, 41, drops)) };
        if topology {
            sophia_backend_live::prepare_rendered_primary_plane_topology_head_from_target_and_selection_with(
                LiveKmsScanoutTargetStatus::Ready, Some(LiveGbmEglFrameTargetRecord::new(size)),
                select_native_primary_plane_target(device), None, device, &mut exporter,
            ).prepared.expect("prepared topology owner")
        } else {
            prepare_rendered_primary_plane_scanout_from_target_and_selection_with(
                LiveKmsScanoutTargetStatus::Ready, Some(LiveGbmEglFrameTargetRecord::new(size)),
                select_native_primary_plane_target(device), None, device, &mut exporter,
            ).prepared.expect("prepared page-flip owner")
        }
    }

    fn failing_cleanup_device() -> FakeNativePrimaryPlaneScanoutDevice {
        FakeNativePrimaryPlaneScanoutDevice {
            resources: FakeNativePrimaryPlaneResourceDevice {
                destroy_framebuffer: Err(io::Error::other("held framebuffer")),
                ..full_primary_plane_resource_device()
            },
            ..full_primary_plane_scanout_device()
        }
    }

    fn prove_cleanup_retry(
        cleanup: sophia_backend_live::LiveRenderedPrimaryPlaneScanoutCleanup<Owner>,
        drops: &Rc<Cell<usize>>,
    ) {
        assert_eq!(cleanup.correlation(), Some(metadata(41)));
        let cleanup = cleanup.map_scanout_buffer(Box::new);
        assert_eq!(cleanup.correlation(), Some(metadata(41)));
        let retry = sophia_backend_live::retry_rendered_primary_plane_scanout_cleanup(
            &failing_cleanup_device(), cleanup,
        ).cleanup.expect("failed retry retains exact owner");
        assert_eq!(retry.correlation(), Some(metadata(41)));
        assert_eq!(drops.get(), 0);
        let finished = sophia_backend_live::retry_rendered_primary_plane_scanout_cleanup(
            &full_primary_plane_scanout_device(), retry,
        );
        assert!(finished.cleanup.is_none());
        assert_eq!(drops.get(), 1);
    }

    #[test]
    fn submitted_correlation_survives_boxing_retirement_and_failed_cleanup_retry() {
        let device = full_primary_plane_scanout_device();
        let drops = Rc::new(Cell::new(0));
        let prepared = prepare_owned(&device, &drops, false);
        let submission = sophia_backend_live::submit_prepared_rendered_primary_plane_scanout(
            &device, prepared,
        ).submission.expect("submitted owner");
        assert_eq!(submission.correlation(), Some(metadata(41)));
        let retired = retire_rendered_primary_plane_scanout_after_page_flip(
            &failing_cleanup_device(), submission,
            &LivePageFlipCallbackReport {
                decision: LivePageFlipCallbackDecision::Accepted,
                event: LivePageFlipEvent { status: LivePageFlipEventStatus::Presented, frame_serial: Some(1) },
            },
        );
        prove_cleanup_retry(retired.cleanup.expect("retirement retains failed cleanup"), &drops);
    }

    #[test]
    fn cancelled_page_flip_and_topology_keep_correlation_until_cleanup_finishes() {
        for topology in [false, true] {
            let device = full_primary_plane_scanout_device();
            let drops = Rc::new(Cell::new(0));
            let prepared = prepare_owned(&device, &drops, topology);
            let cancelled = if topology {
                let prepared = sophia_backend_live::prepare_rendered_topology_head_from_prepared_scanout(
                    prepared, None,
                ).expect("topology conversion");
                assert_eq!(prepared.correlation(), Some(metadata(41)));
                sophia_backend_live::cancel_prepared_rendered_topology_head(&failing_cleanup_device(), prepared)
            } else {
                cancel_prepared_rendered_primary_plane_scanout(&failing_cleanup_device(), prepared)
            };
            prove_cleanup_retry(cancelled.cleanup.expect("cancel retains failed cleanup"), &drops);
        }
    }

    #[test]
    fn adopted_topology_and_failed_submit_preserve_correlation() {
        let device = full_primary_plane_scanout_device();
        let drops = Rc::new(Cell::new(0));
        let prepared = prepare_owned(&device, &drops, true);
        let topology = sophia_backend_live::prepare_rendered_topology_head_from_prepared_scanout(
            prepared, None,
        ).expect("topology conversion");
        let submission = sophia_backend_live::adopt_prepared_rendered_topology_head_after_commit(topology);
        assert_eq!(submission.correlation(), Some(metadata(41)));
        let submission = submission.map_scanout_buffer(Box::new);
        assert_eq!(submission.correlation(), Some(metadata(41)));
        // This control exercises adoption custody, not a hardware topology commit.
        let retired = retire_rendered_primary_plane_scanout_after_page_flip(
            &device, submission,
            &LivePageFlipCallbackReport {
                decision: LivePageFlipCallbackDecision::Accepted,
                event: LivePageFlipEvent { status: LivePageFlipEventStatus::Presented, frame_serial: Some(1) },
            },
        );
        assert!(retired.submission.is_none());
        assert!(retired.cleanup.is_none());
        assert_eq!(drops.get(), 1);

        let drops = Rc::new(Cell::new(0));
        let prepared = prepare_owned(&device, &drops, false);
        let failing = failing_cleanup_device().accepting_commits(0);
        let result = sophia_backend_live::submit_prepared_rendered_primary_plane_scanout(&failing, prepared);
        assert!(result.submission.is_none());
        prove_cleanup_retry(result.cleanup.expect("failed commit retains cleanup"), &drops);
    }

    #[test]
    fn persistent_successor_is_presented_even_when_old_cleanup_and_teardown_fail() {
        let root = ready_drm_sysfs_fixture("persistent-custody-cleanup-overlap");
        let mut runtime = discover_live_backend(&LiveBackendConfig::new(&root))
            .into_live_runtime_assembly(QueuedInputPoller::default()).unwrap()
            .with_persistent_rendered_primary_plane_scanout();
        let size = Size { width: 1280, height: 720 };
        let first = Rc::new(Cell::new(0));
        let second = Rc::new(Cell::new(0));
        let device = full_primary_plane_scanout_device();
        let failing = failing_cleanup_device();
        for (generation, drops) in [(41, &first), (42, &second)] {
            let mut exporter = Exporter { pending: Some(owned_export(size, generation, drops)) };
            let submitted = runtime.submit_and_track_rendered_primary_plane_scanout_with(&device, &mut exporter);
            assert_eq!(submitted.status, LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip);
            let presented = runtime.retire_tracked_rendered_primary_plane_scanout_after_page_flip(
                &failing,
                &LivePageFlipCallbackReport {
                    decision: LivePageFlipCallbackDecision::Accepted,
                    event: LivePageFlipEvent { status: LivePageFlipEventStatus::Presented, frame_serial: Some(generation) },
                },
            );
            assert_eq!(presented.status, LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::RetiredAfterPageFlip);
            assert_eq!(presented.runtime_scanout_state, Some(RuntimeScanoutState::Retired));
            assert_eq!(presented.cleanup_pending, generation == 42);
            assert!(runtime.rendered_primary_plane_scanout_displayed());
        }
        assert_eq!((first.get(), second.get()), (0, 0));
        let teardown = runtime.retire_displayed_rendered_primary_plane_scanout(&failing);
        assert!(teardown.cleanup_pending);
        assert!(!runtime.rendered_primary_plane_scanout_displayed());
        assert_eq!((first.get(), second.get()), (0, 0), "teardown must retain both failed owners");
        for _ in 0..3 {
            assert!(runtime.retry_tracked_rendered_primary_plane_scanout_cleanup(&failing).cleanup_pending);
            assert_eq!((first.get(), second.get()), (0, 0));
        }
        assert!(runtime.retry_tracked_rendered_primary_plane_scanout_cleanup(&device).cleanup_pending);
        assert_eq!(first.get() + second.get(), 1);
        assert!(!runtime.retry_tracked_rendered_primary_plane_scanout_cleanup(&device).cleanup_pending);
        assert_eq!((first.get(), second.get()), (1, 1));
        let repeated = runtime.retry_tracked_rendered_primary_plane_scanout_cleanup(&device);
        assert_eq!(repeated.status, LiveTrackedRenderedPrimaryPlaneScanoutCleanupStatus::NoCleanupPending);
        assert_eq!((first.get(), second.get()), (1, 1));
        std::fs::remove_dir_all(root).unwrap();
    }
}
