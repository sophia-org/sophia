use super::*;

#[path = "native_page_flip/decoding.rs"]
mod decoding;

#[test]
fn native_libdrm_read_loop_result_maps_to_reduced_poll_report() {
    assert_eq!(
        LibdrmNativeReadLoopReport::idle().into_poll_report().status,
        LibdrmPageFlipEventPollStatus::Idle
    );
    assert_eq!(
        LibdrmNativeReadLoopReport::would_block()
            .into_poll_report()
            .status,
        LibdrmPageFlipEventPollStatus::Idle
    );

    let decoded =
        LibdrmNativeReadLoopReport::callback_decoded(3).expect("decoded count must be nonzero");
    assert_eq!(decoded.status, LibdrmNativeReadLoopStatus::CallbackDecoded);
    assert_eq!(decoded.into_poll_report().callbacks.emitted, 3);
    assert_eq!(
        decoded.into_poll_report().status,
        LibdrmPageFlipEventPollStatus::Emitted
    );

    assert_eq!(LibdrmNativeReadLoopReport::callback_decoded(0), None);
    let rejected =
        LibdrmNativeReadLoopReport::callbacks_decoded(0, 2).expect("rejection count is observable");
    assert_eq!(
        rejected.status,
        LibdrmNativeReadLoopStatus::CallbackRejected
    );
    assert_eq!(rejected.decoded_callbacks, 0);
    assert_eq!(rejected.rejected_callbacks, 2);
    assert_eq!(
        rejected.into_poll_report().status,
        LibdrmPageFlipEventPollStatus::Idle
    );

    let mixed = LibdrmNativeReadLoopReport::callbacks_decoded(2, 1)
        .expect("decoded or rejected counts should produce a report");
    assert_eq!(mixed.status, LibdrmNativeReadLoopStatus::CallbackDecoded);
    assert_eq!(mixed.decoded_callbacks, 2);
    assert_eq!(mixed.rejected_callbacks, 1);
    assert_eq!(mixed.into_poll_report().callbacks.emitted, 2);

    assert_eq!(LibdrmNativeReadLoopReport::callbacks_decoded(0, 0), None);
    assert_eq!(
        LibdrmNativeReadLoopReport::read_failed()
            .into_poll_report()
            .status,
        LibdrmPageFlipEventPollStatus::Disconnected
    );
}

#[test]
fn native_libdrm_poller_skeleton_reports_idle_without_emitting_callbacks() {
    let authority =
        LibdrmBackendFdAuthority::new(14).expect("nonzero generation should mint authority token");
    let source = LibdrmNativePageFlipSource::from_authority(authority);
    let mut poller = NativeLibdrmPageFlipEventPoller::new(source);
    let (sender, receiver) = mpsc::sync_channel(1);

    assert_eq!(
        poller.source_report(),
        LibdrmNativePageFlipSourceReport {
            status: LibdrmNativePageFlipSourceStatus::ConstructedWithoutPolling,
        }
    );
    assert_eq!(
        poller.poll_page_flip_events(&sender, 4).status,
        LibdrmPageFlipEventPollStatus::Idle
    );
    assert!(receiver.try_recv().is_err());
}

#[test]
fn native_libdrm_poller_drains_injected_callback_batch_without_fd_polling() {
    let authority =
        LibdrmBackendFdAuthority::new(15).expect("nonzero generation should mint authority token");
    let slot = LibdrmNativeOutputSlot::new(2).expect("nonzero slot should be valid");
    let source = LibdrmNativePageFlipSource::from_authority(authority);
    let mut poller =
        NativeLibdrmPageFlipEventPoller::new(source).with_routes([LibdrmNativeOutputRoute {
            slot,
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
        }]);
    let (sender, receiver) = mpsc::sync_channel(4);

    poller.inject_callbacks([
        LibdrmNativePageFlipCallback::new(slot, 81),
        LibdrmNativePageFlipCallback::new(slot, 0),
        LibdrmNativePageFlipCallback::new(slot, 82),
    ]);
    assert_eq!(poller.pending_callback_count(), 3);

    let report = poller.poll_page_flip_events(&sender, 4);

    assert_eq!(report.status, LibdrmPageFlipEventPollStatus::Emitted);
    assert_eq!(report.callbacks.emitted, 2);
    assert_eq!(poller.pending_callback_count(), 0);
    assert_eq!(
        poller.last_read_loop_report().status,
        LibdrmNativeReadLoopStatus::CallbackDecoded
    );
    assert_eq!(poller.last_read_loop_report().decoded_callbacks, 2);
    assert_eq!(poller.last_read_loop_report().rejected_callbacks, 1);
    assert_eq!(
        receiver
            .try_recv()
            .expect("first callback should be queued"),
        LivePageFlipCallback {
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
            frame_serial: 81,
        }
    );
    assert_eq!(
        receiver
            .try_recv()
            .expect("second callback should be queued"),
        LivePageFlipCallback {
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
            frame_serial: 82,
        }
    );
}

#[test]
fn native_libdrm_reader_reads_bounded_callbacks_without_kms_identity() {
    let slot = LibdrmNativeOutputSlot::new(2).expect("nonzero slot should be valid");
    let mut reader = FakeLibdrmNativePageFlipReader::new([
        LibdrmNativePageFlipCallback::new(slot, 81),
        LibdrmNativePageFlipCallback::new(slot, 82),
    ]);

    let first = reader.read_ready_page_flip_callbacks(1);
    assert_eq!(
        first,
        LibdrmNativePageFlipReadResult {
            report: LibdrmNativeReadLoopReport::callback_decoded(1)
                .expect("one callback should produce a read report"),
            callbacks: vec![LibdrmNativePageFlipCallback::new(slot, 81)],
        }
    );
    assert_eq!(reader.queued_len(), 1);

    let second = reader.read_ready_page_flip_callbacks(4);
    assert_eq!(second.report.decoded_callbacks, 1);
    assert_eq!(
        second.callbacks,
        vec![LibdrmNativePageFlipCallback::new(slot, 82)]
    );
    assert_eq!(reader.queued_len(), 0);

    let empty = reader.read_ready_page_flip_callbacks(4);
    assert_eq!(empty.report, LibdrmNativeReadLoopReport::would_block());
    assert!(empty.callbacks.is_empty());
}

#[test]
fn native_libdrm_page_flip_event_reducer_uses_private_crtc_routes() {
    let crtc = drm::control::from_u32::<drm::control::crtc::Handle>(44)
        .expect("nonzero crtc handle should be constructible");
    let other_crtc = drm::control::from_u32::<drm::control::crtc::Handle>(45)
        .expect("nonzero crtc handle should be constructible");
    let slot = LibdrmNativeOutputSlot::new(3).expect("nonzero slot should be valid");
    let event = drm::control::PageFlipEvent {
        frame: 91,
        duration: std::time::Duration::from_millis(16),
        crtc,
    };
    let mut routes = [LibdrmNativeCrtcRoute::new(crtc, slot)];

    assert_eq!(
        reduce_native_page_flip_event(&event, &mut routes),
        Some(LibdrmNativePageFlipCallback::new_with_kernel_clock(
            slot,
            91,
            91,
            std::time::Duration::from_millis(16),
        ))
    );
    assert_eq!(
        reduce_native_page_flip_event(&event, &mut [LibdrmNativeCrtcRoute::new(other_crtc, slot)]),
        None
    );

    let initial_event = drm::control::PageFlipEvent { frame: 0, ..event };
    let mut initial_route = [LibdrmNativeCrtcRoute::new(crtc, slot)];
    assert_eq!(
        reduce_native_page_flip_event(&initial_event, &mut initial_route),
        Some(LibdrmNativePageFlipCallback::new_with_kernel_clock(
            slot,
            1,
            0,
            std::time::Duration::from_millis(16),
        ))
    );
    assert_eq!(
        reduce_native_page_flip_event(&initial_event, &mut initial_route),
        Some(LibdrmNativePageFlipCallback::new_with_kernel_clock(
            slot,
            2,
            0,
            std::time::Duration::from_millis(16),
        ))
    );
}

#[test]
fn native_libdrm_page_flip_event_reader_owns_device_and_private_crtc_routes() {
    let crtc = drm::control::from_u32::<drm::control::crtc::Handle>(44)
        .expect("nonzero crtc handle should be constructible");
    let slot = LibdrmNativeOutputSlot::new(3).expect("nonzero slot should be valid");
    let reader = NativeLibdrmPageFlipEventReader::new(())
        .with_crtc_routes([LibdrmNativeCrtcRoute::new(crtc, slot)]);

    assert_eq!(reader.crtc_route_count(), 1);
}

#[test]
fn native_libdrm_poller_reads_and_polls_bounded_callbacks() {
    let authority =
        LibdrmBackendFdAuthority::new(24).expect("nonzero generation should mint authority token");
    let slot = LibdrmNativeOutputSlot::new(2).expect("nonzero slot should be valid");
    let source = LibdrmNativePageFlipSource::from_authority(authority);
    let mut poller =
        NativeLibdrmPageFlipEventPoller::new(source).with_routes([LibdrmNativeOutputRoute {
            slot,
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
        }]);
    let mut reader = FakeLibdrmNativePageFlipReader::new([
        LibdrmNativePageFlipCallback::new_with_kernel_timestamp(
            slot,
            81,
            std::time::Duration::from_micros(123_456),
        ),
        LibdrmNativePageFlipCallback::new(slot, 82),
    ]);
    let (sender, receiver) = mpsc::sync_channel(4);

    let report = poller.read_and_poll_page_flip_events(&mut reader, &sender, 1, 4);

    assert_eq!(
        report,
        LibdrmNativeReadAndPollReport {
            read_loop: LibdrmNativeReadLoopReport::callback_decoded(1)
                .expect("one callback should produce a read report"),
            poll: LibdrmPageFlipEventPollReport {
                status: LibdrmPageFlipEventPollStatus::Emitted,
                callbacks: LivePageFlipCallbackSourceReport {
                    emitted: 1,
                    queued_remaining: 0,
                    backpressure: false,
                    disconnected: false,
                    max_reached: false,
                },
            },
        }
    );
    assert_eq!(reader.queued_len(), 1);
    assert_eq!(poller.pending_callback_count(), 0);
    assert_eq!(
        receiver
            .try_recv()
            .expect("callback should be reduced and queued"),
        LivePageFlipCallback {
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
            frame_serial: 81,
        }
    );
    assert_eq!(
        poller.drain_emitted_kernel_timestamps(),
        vec![LibdrmKernelPageFlipTimestamp {
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
            frame_serial: 81,
            ust_usec: 123_456,
            kernel_sequence: None,
        }]
    );
}

#[test]
fn native_libdrm_collect_preserves_zero_kernel_sequence_beside_normalized_serial() {
    let authority = LibdrmBackendFdAuthority::new(24).unwrap();
    let slot = LibdrmNativeOutputSlot::new(2).unwrap();
    let crtc = drm::control::from_u32::<drm::control::crtc::Handle>(44).unwrap();
    let mut routes = [LibdrmNativeCrtcRoute::new(crtc, slot)];
    let event = drm::control::PageFlipEvent {
        frame: 0, duration: std::time::Duration::from_micros(123_456), crtc,
    };
    let callback = reduce_native_page_flip_event(&event, &mut routes).unwrap();
    assert_eq!(callback.frame_serial, 1);
    assert_eq!(callback.kernel_sequence(), Some(0));
    let second_crtc = drm::control::from_u32::<drm::control::crtc::Handle>(45).unwrap();
    let second_slot = LibdrmNativeOutputSlot::new(3).unwrap();
    let mut routes = [
        LibdrmNativeCrtcRoute::new(crtc, slot),
        LibdrmNativeCrtcRoute::new(second_crtc, second_slot),
    ];
    let second = reduce_native_page_flip_event(
        &drm::control::PageFlipEvent { frame: 700, crtc: second_crtc, ..event },
        &mut routes,
    ).unwrap();
    let mut reader = FakeLibdrmNativePageFlipReader::new([callback, second]);
    let source = LibdrmNativePageFlipSource::from_authority(authority);
    let mut poller = NativeLibdrmPageFlipEventPoller::new(source).with_routes([
        LibdrmNativeOutputRoute {
            slot,
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
        },
        LibdrmNativeOutputRoute {
            slot: second_slot,
            output: OutputId::from_raw(7), // mirror, same logical output
            head: sophia_engine::RenderHeadId::from_raw(2),
        },
    ]);
    let mut callbacks = Vec::new();
    let mut timestamps = Vec::new();
    poller.read_and_collect_page_flip_events(&mut reader, &mut callbacks, &mut timestamps, 2, 2);
    assert_eq!(callbacks.len(), 2);
    assert_eq!(callbacks[0].frame_serial, 1);
    assert_eq!(timestamps.len(), 2);
    assert_eq!(timestamps[0].frame_serial, 1);
    assert_eq!(timestamps[0].kernel_sequence, Some(0));
    assert_eq!(timestamps[0].ust_usec, 123_456);
    assert_eq!(timestamps[0].head.raw(), 1);
    assert_eq!(timestamps[1].head.raw(), 2);
    assert_eq!(timestamps[1].kernel_sequence, Some(700));
    assert_eq!(timestamps[1].frame_serial, 700);
}

#[test]
fn native_libdrm_poller_preserves_would_block_diagnostics_after_empty_read() {
    let authority =
        LibdrmBackendFdAuthority::new(26).expect("nonzero generation should mint authority token");
    let slot = LibdrmNativeOutputSlot::new(2).expect("nonzero slot should be valid");
    let source = LibdrmNativePageFlipSource::from_authority(authority);
    let mut poller =
        NativeLibdrmPageFlipEventPoller::new(source).with_routes([LibdrmNativeOutputRoute {
            slot,
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
        }]);
    let mut reader = FakeLibdrmNativePageFlipReader::new([]);
    let (sender, receiver) = mpsc::sync_channel(1);

    let report = poller.read_and_poll_page_flip_events(&mut reader, &sender, 4, 4);

    assert_eq!(report.read_loop, LibdrmNativeReadLoopReport::would_block());
    assert_eq!(report.poll.status, LibdrmPageFlipEventPollStatus::Idle);
    assert_eq!(poller.pending_callback_count(), 0);
    assert_eq!(
        poller.last_read_loop_report(),
        LibdrmNativeReadLoopReport::would_block()
    );
    assert_eq!(
        poller.diagnostics(),
        LibdrmNativePollerDiagnostics {
            route_count: 1,
            pending_callbacks: 0,
            last_read_loop: LibdrmNativeReadLoopReport::would_block(),
        }
    );
    assert!(receiver.try_recv().is_err());
}

#[test]
fn native_libdrm_poller_drains_pending_callbacks_before_reading_more() {
    let authority =
        LibdrmBackendFdAuthority::new(27).expect("nonzero generation should mint authority token");
    let slot = LibdrmNativeOutputSlot::new(2).expect("nonzero slot should be valid");
    let source = LibdrmNativePageFlipSource::from_authority(authority);
    let mut poller =
        NativeLibdrmPageFlipEventPoller::new(source).with_routes([LibdrmNativeOutputRoute {
            slot,
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
        }]);
    let mut reader = FakeLibdrmNativePageFlipReader::new([
        LibdrmNativePageFlipCallback::new(slot, 82),
        LibdrmNativePageFlipCallback::new(slot, 83),
    ]);
    let (sender, receiver) = mpsc::sync_channel(1);

    poller.inject_callbacks([LibdrmNativePageFlipCallback::new(slot, 81)]);

    let first = poller.read_and_poll_page_flip_events(&mut reader, &sender, 4, 4);

    assert_eq!(first.read_loop.decoded_callbacks, 1);
    assert_eq!(first.poll.status, LibdrmPageFlipEventPollStatus::Emitted);
    assert_eq!(first.poll.callbacks.emitted, 1);
    assert_eq!(reader.queued_len(), 2);
    assert_eq!(poller.pending_callback_count(), 0);
    assert_eq!(
        receiver
            .try_recv()
            .expect("retained callback should emit before native read"),
        LivePageFlipCallback {
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
            frame_serial: 81,
        }
    );

    let second = poller.read_and_poll_page_flip_events(&mut reader, &sender, 4, 4);

    assert_eq!(
        second.read_loop,
        LibdrmNativeReadLoopReport::callback_decoded(2)
            .expect("reader should then consume the queued native callbacks")
    );
    assert_eq!(
        second.poll.status,
        LibdrmPageFlipEventPollStatus::Backpressure
    );
    assert_eq!(second.poll.callbacks.emitted, 1);
    assert_eq!(second.poll.callbacks.queued_remaining, 1);
    assert_eq!(reader.queued_len(), 0);
    assert_eq!(poller.pending_callback_count(), 1);
    assert_eq!(
        receiver
            .try_recv()
            .expect("first newly read callback should emit"),
        LivePageFlipCallback {
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
            frame_serial: 82,
        }
    );
}

#[test]
fn native_libdrm_poller_reports_read_failure_after_pending_backlog_drains() {
    let authority =
        LibdrmBackendFdAuthority::new(25).expect("nonzero generation should mint authority token");
    let slot = LibdrmNativeOutputSlot::new(2).expect("nonzero slot should be valid");
    let source = LibdrmNativePageFlipSource::from_authority(authority);
    let mut poller =
        NativeLibdrmPageFlipEventPoller::new(source).with_routes([LibdrmNativeOutputRoute {
            slot,
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
        }]);
    let mut reader =
        FakeLibdrmNativePageFlipReader::new([LibdrmNativePageFlipCallback::new(slot, 81)]);
    reader.fail_next_read();
    poller.inject_callbacks([LibdrmNativePageFlipCallback::new(slot, 80)]);
    let (sender, receiver) = mpsc::sync_channel(4);

    let drained = poller.read_and_poll_page_flip_events(&mut reader, &sender, 4, 4);

    assert_eq!(
        drained.read_loop,
        LibdrmNativeReadLoopReport::callback_decoded(1)
            .expect("retained pending callback should drain first")
    );
    assert_eq!(drained.poll.status, LibdrmPageFlipEventPollStatus::Emitted);
    assert_eq!(reader.queued_len(), 1);
    assert_eq!(poller.pending_callback_count(), 0);
    assert_eq!(
        receiver
            .try_recv()
            .expect("retained callback should be queued before native read"),
        LivePageFlipCallback {
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
            frame_serial: 80,
        }
    );

    let failed = poller.read_and_poll_page_flip_events(&mut reader, &sender, 4, 4);

    assert_eq!(failed.read_loop, LibdrmNativeReadLoopReport::read_failed());
    assert_eq!(
        failed.poll.status,
        LibdrmPageFlipEventPollStatus::Disconnected
    );
    assert_eq!(reader.queued_len(), 1);
    assert_eq!(poller.pending_callback_count(), 0);
    assert!(receiver.try_recv().is_err());
}

#[test]
fn native_libdrm_poller_retains_injected_callbacks_on_backpressure() {
    let authority =
        LibdrmBackendFdAuthority::new(16).expect("nonzero generation should mint authority token");
    let slot = LibdrmNativeOutputSlot::new(2).expect("nonzero slot should be valid");
    let source = LibdrmNativePageFlipSource::from_authority(authority);
    let mut poller =
        NativeLibdrmPageFlipEventPoller::new(source).with_routes([LibdrmNativeOutputRoute {
            slot,
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
        }]);
    let (sender, receiver) = mpsc::sync_channel(1);

    poller.inject_callbacks([
        LibdrmNativePageFlipCallback::new(slot, 81),
        LibdrmNativePageFlipCallback::new(slot, 82),
    ]);

    let first = poller.poll_page_flip_events(&sender, 4);
    assert_eq!(first.status, LibdrmPageFlipEventPollStatus::Backpressure);
    assert_eq!(first.callbacks.emitted, 1);
    assert_eq!(first.callbacks.queued_remaining, 1);
    assert_eq!(poller.pending_callback_count(), 1);
    assert_eq!(poller.last_read_loop_report().decoded_callbacks, 2);
    assert_eq!(
        receiver
            .try_recv()
            .expect("first callback should be queued"),
        LivePageFlipCallback {
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
            frame_serial: 81,
        }
    );

    let second = poller.poll_page_flip_events(&sender, 4);
    assert_eq!(second.status, LibdrmPageFlipEventPollStatus::Emitted);
    assert_eq!(second.callbacks.emitted, 1);
    assert_eq!(poller.pending_callback_count(), 0);
    assert_eq!(poller.last_read_loop_report().decoded_callbacks, 1);
    assert_eq!(
        receiver
            .try_recv()
            .expect("retained callback should be queued"),
        LivePageFlipCallback {
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
            frame_serial: 82,
        }
    );
}

#[test]
fn native_libdrm_poller_retains_injected_callbacks_on_disconnected_queue() {
    let authority =
        LibdrmBackendFdAuthority::new(17).expect("nonzero generation should mint authority token");
    let slot = LibdrmNativeOutputSlot::new(2).expect("nonzero slot should be valid");
    let source = LibdrmNativePageFlipSource::from_authority(authority);
    let mut poller =
        NativeLibdrmPageFlipEventPoller::new(source).with_routes([LibdrmNativeOutputRoute {
            slot,
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
        }]);
    let (sender, receiver) = mpsc::sync_channel(1);

    poller.inject_callbacks([
        LibdrmNativePageFlipCallback::new(slot, 81),
        LibdrmNativePageFlipCallback::new(slot, 82),
    ]);
    drop(receiver);

    let report = poller.poll_page_flip_events(&sender, 4);

    assert_eq!(report.status, LibdrmPageFlipEventPollStatus::Disconnected);
    assert_eq!(report.callbacks.emitted, 0);
    assert_eq!(report.callbacks.queued_remaining, 2);
    assert_eq!(poller.pending_callback_count(), 2);
    assert_eq!(poller.last_read_loop_report().decoded_callbacks, 1);
}

#[test]
fn native_libdrm_poller_replaces_routes_without_dropping_pending_callbacks() {
    let authority =
        LibdrmBackendFdAuthority::new(18).expect("nonzero generation should mint authority token");
    let slot = LibdrmNativeOutputSlot::new(2).expect("nonzero slot should be valid");
    let source = LibdrmNativePageFlipSource::from_authority(authority);
    let mut poller =
        NativeLibdrmPageFlipEventPoller::new(source).with_routes([LibdrmNativeOutputRoute {
            slot,
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
        }]);
    let (sender, receiver) = mpsc::sync_channel(2);

    poller.inject_callbacks([LibdrmNativePageFlipCallback::new(slot, 81)]);
    poller.replace_routes([LibdrmNativeOutputRoute {
        slot,
        output: OutputId::from_raw(9),
        head: sophia_engine::RenderHeadId::from_raw(1),
    }]);

    assert_eq!(poller.route_count(), 1);
    assert_eq!(poller.pending_callback_count(), 1);

    let report = poller.poll_page_flip_events(&sender, 2);

    assert_eq!(report.status, LibdrmPageFlipEventPollStatus::Emitted);
    assert_eq!(poller.pending_callback_count(), 0);
    assert_eq!(
        receiver
            .try_recv()
            .expect("callback should use replaced route"),
        LivePageFlipCallback {
            output: OutputId::from_raw(9),
            head: sophia_engine::RenderHeadId::from_raw(1),
            frame_serial: 81,
        }
    );
}

#[test]
fn native_libdrm_poller_rejects_pending_callbacks_after_route_removal() {
    let authority =
        LibdrmBackendFdAuthority::new(19).expect("nonzero generation should mint authority token");
    let slot = LibdrmNativeOutputSlot::new(2).expect("nonzero slot should be valid");
    let source = LibdrmNativePageFlipSource::from_authority(authority);
    let mut poller =
        NativeLibdrmPageFlipEventPoller::new(source).with_routes([LibdrmNativeOutputRoute {
            slot,
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
        }]);
    let (sender, receiver) = mpsc::sync_channel(2);

    poller.inject_callbacks([LibdrmNativePageFlipCallback::new(slot, 81)]);
    poller.replace_routes([]);

    let report = poller.poll_page_flip_events(&sender, 2);

    assert_eq!(report.status, LibdrmPageFlipEventPollStatus::Idle);
    assert_eq!(report.callbacks.emitted, 0);
    assert_eq!(poller.pending_callback_count(), 0);
    assert_eq!(
        poller.last_read_loop_report().status,
        LibdrmNativeReadLoopStatus::CallbackRejected
    );
    assert_eq!(poller.last_read_loop_report().rejected_callbacks, 1);
    assert!(receiver.try_recv().is_err());
}

#[test]
fn native_libdrm_poller_diagnostics_report_only_reduced_counts() {
    let authority =
        LibdrmBackendFdAuthority::new(20).expect("nonzero generation should mint authority token");
    let first_slot = LibdrmNativeOutputSlot::new(2).expect("nonzero slot should be valid");
    let second_slot = LibdrmNativeOutputSlot::new(3).expect("nonzero slot should be valid");
    let source = LibdrmNativePageFlipSource::from_authority(authority);
    let mut poller =
        NativeLibdrmPageFlipEventPoller::new(source).with_routes([LibdrmNativeOutputRoute {
            slot: first_slot,
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
        }]);
    let (sender, receiver) = mpsc::sync_channel(2);

    poller.inject_callbacks([
        LibdrmNativePageFlipCallback::new(first_slot, 81),
        LibdrmNativePageFlipCallback::new(second_slot, 82),
    ]);

    assert_eq!(
        poller.diagnostics(),
        LibdrmNativePollerDiagnostics {
            route_count: 1,
            pending_callbacks: 2,
            last_read_loop: LibdrmNativeReadLoopReport::idle(),
        }
    );

    poller.replace_routes([
        LibdrmNativeOutputRoute {
            slot: first_slot,
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
        },
        LibdrmNativeOutputRoute {
            slot: second_slot,
            output: OutputId::from_raw(9),
            head: sophia_engine::RenderHeadId::from_raw(1),
        },
    ]);
    let report = poller.poll_page_flip_events(&sender, 4);

    assert_eq!(report.status, LibdrmPageFlipEventPollStatus::Emitted);
    assert_eq!(
        poller.diagnostics(),
        LibdrmNativePollerDiagnostics {
            route_count: 2,
            pending_callbacks: 0,
            last_read_loop: LibdrmNativeReadLoopReport::callback_decoded(2)
                .expect("decoded count should build a report"),
        }
    );
    assert_eq!(
        receiver
            .try_recv()
            .expect("first callback should be queued"),
        LivePageFlipCallback {
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
            frame_serial: 81,
        }
    );
    assert_eq!(
        receiver
            .try_recv()
            .expect("second callback should be queued"),
        LivePageFlipCallback {
            output: OutputId::from_raw(9),
            head: sophia_engine::RenderHeadId::from_raw(1),
            frame_serial: 82,
        }
    );
}

#[test]
fn live_runtime_assembly_reports_reduced_native_libdrm_poller_diagnostics() {
    let root = ready_drm_sysfs_fixture("native-libdrm-runtime-diagnostics");
    let report = discover_live_backend(&LiveBackendConfig::new(&root));
    let authority =
        LibdrmBackendFdAuthority::new(21).expect("nonzero generation should mint authority token");
    let slot = LibdrmNativeOutputSlot::new(2).expect("nonzero slot should be valid");
    let source = LibdrmNativePageFlipSource::from_authority(authority);
    let mut poller =
        NativeLibdrmPageFlipEventPoller::new(source).with_routes([LibdrmNativeOutputRoute {
            slot,
            output: OutputId::from_raw(1),
            head: sophia_engine::RenderHeadId::from_raw(1),
        }]);
    let (sender, receiver) = mpsc::sync_channel(2);

    poller.inject_callbacks([
        LibdrmNativePageFlipCallback::new(slot, 81),
        LibdrmNativePageFlipCallback::new(slot, 0),
    ]);
    let poll_report = poller.poll_page_flip_events(&sender, 4);
    assert_eq!(poll_report.status, LibdrmPageFlipEventPollStatus::Emitted);

    let mut assembly = report
        .into_live_runtime_assembly(QueuedInputPoller::default())
        .expect("ready startup should seed live assembly")
        .with_native_libdrm_poller_diagnostics(poller.diagnostics())
        .with_page_flip_callback_queue(LivePageFlipCallbackQueue::new(receiver, 4));

    assert_eq!(
        assembly.libdrm_poller_diagnostics(),
        LiveLibdrmPollerDiagnostics {
            status: LiveLibdrmPollerDiagnosticsStatus::CallbackDecoded,
            route_count: 1,
            pending_callbacks: 0,
            decoded_callbacks: 1,
            rejected_callbacks: 1,
        }
    );

    let tick = assembly
        .run_tick(CompositorBackendTickInput::default())
        .expect("runtime tick should drain callback and report diagnostics");

    assert_eq!(
        tick.libdrm_poller,
        LiveLibdrmPollerDiagnostics {
            status: LiveLibdrmPollerDiagnosticsStatus::CallbackDecoded,
            route_count: 1,
            pending_callbacks: 0,
            decoded_callbacks: 1,
            rejected_callbacks: 1,
        }
    );
    assert_eq!(
        tick.page_flip,
        LivePageFlipEvent {
            status: LivePageFlipEventStatus::Presented,
            frame_serial: Some(81),
        }
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_card_completion_collector_reuses_direct_buffers_and_defers_without_loss() {
    let authority =
        LibdrmBackendFdAuthority::new(25).expect("nonzero generation should mint authority token");
    let slot = LibdrmNativeOutputSlot::new(2).expect("nonzero slot should be valid");
    let source = LibdrmNativePageFlipSource::from_authority(authority);
    let mut poller =
        NativeLibdrmPageFlipEventPoller::new(source).with_routes([LibdrmNativeOutputRoute {
            slot,
            output: OutputId::from_raw(7),
            head: sophia_engine::RenderHeadId::from_raw(1),
        }]);
    let mut reader = FakeLibdrmNativePageFlipReader::new([
        LibdrmNativePageFlipCallback::new(slot, 81),
        LibdrmNativePageFlipCallback::new(slot, 82),
        LibdrmNativePageFlipCallback::new(slot, 83),
    ]);
    let mut callbacks = Vec::with_capacity(2);
    let mut timestamps = Vec::with_capacity(2);

    let first = poller.read_and_collect_page_flip_events(
        &mut reader,
        &mut callbacks,
        &mut timestamps,
        2,
        1,
    );
    assert_eq!(first.poll.callbacks.emitted, 1);
    assert_eq!(first.poll.callbacks.queued_remaining, 1);
    assert!(first.poll.callbacks.max_reached);
    assert_eq!(callbacks[0].frame_serial, 81);
    assert_eq!(reader.queued_len(), 1);

    callbacks.clear();
    let second = poller.read_and_collect_page_flip_events(
        &mut reader,
        &mut callbacks,
        &mut timestamps,
        2,
        2,
    );
    assert_eq!(second.poll.callbacks.emitted, 1);
    assert_eq!(callbacks[0].frame_serial, 82);
    assert_eq!(reader.queued_len(), 1);

    callbacks.clear();
    let third = poller.read_and_collect_page_flip_events(
        &mut reader,
        &mut callbacks,
        &mut timestamps,
        2,
        2,
    );
    assert_eq!(third.poll.callbacks.emitted, 1);
    assert_eq!(callbacks[0].frame_serial, 83);
    assert_eq!(reader.queued_len(), 0);
    assert_eq!(poller.pending_callback_count(), 0);

    let cumulative = poller.cumulative_diagnostics();
    assert_eq!(cumulative.read_calls, 2);
    assert_eq!(cumulative.decoded_callbacks, 3);
    assert_eq!(cumulative.rejected_callbacks, 0);
    assert_eq!(cumulative.emitted_callbacks, 3);
}
