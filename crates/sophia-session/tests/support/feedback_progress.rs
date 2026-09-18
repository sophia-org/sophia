use crate::live_session::{
    XPresentSessionObserver, authority_batch_has_engine_work,
    native_frame_service_should_preempt_authority, wm_update_coordinator_batch,
};
use sophia_backend_live::{
    LivePresentBufferDisposition, LivePresentFeedbackOutcome, LivePresentProtocolFeedback,
    LiveProductionVisualRuntime,
};
use sophia_engine::{
    HeadlessOutput, OutputFrameServiceObservation, OutputFrameServiceRequest,
    OutputNativeFramePhase,
};
use sophia_protocol::{NamespaceId, OutputId, Size, TransactionId};
use sophia_x_authority::{
    X_PRESENT_MAJOR_OPCODE, XServerFrontendConfig, XServerFrontendRouteBroker,
    XServerFrontendServiceCommand, run_x_server_frontend_routed_until_stopped,
};
use std::io::{Read, Write};
use std::num::NonZeroUsize;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct FeedbackFrontend {
    socket: std::path::PathBuf,
    stop: mpsc::SyncSender<XServerFrontendServiceCommand>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[test]
fn skipped_first_present_reopens_admission_without_claiming_presentation() {
    skipped_admission_control(false);
}

#[test]
fn skipped_first_present_preserves_already_quarantined_successor() {
    skipped_admission_control(true);
}

fn skipped_admission_control(early_successor: bool) {
    use crate::live_session::PersistentLiveLayout;
    use sophia_engine::SurfacePresentationAdmissionState;
    use sophia_protocol::{
        LayoutNodeKind, SurfaceConstraints, SurfacePlacementPreference, SurfacePresentationIntent,
        SurfacePresentationIntentKind, SurfacePresentationRole,
    };

    let mut client = FeedbackClient::new();
    let batch = client.present_batch(81, 0);
    let pixels = batch
        .transactions
        .first()
        .expect("real Present transaction");
    let candidate = pixels.key();
    let surface = candidate.surface;
    let geometry = pixels.target_geometry;
    let admission = TransactionId::from_raw(8000);
    let mut layout = PersistentLiveLayout::default();
    layout.admissions.observe_intent(SurfacePresentationIntent {
        surface,
        kind: SurfacePresentationIntentKind::Request,
        role: SurfacePresentationRole::PolicyManaged,
        surface_kind: LayoutNodeKind::Toplevel,
        placement_preference: SurfacePlacementPreference::Default,
        presentation_owner: None,
        stack_rank: 0,
        geometry,
        constraints: SurfaceConstraints {
            min_size: None,
            max_size: None,
        },
        generation: 1,
    });
    assert!(
        layout
            .admissions
            .begin_control(surface, admission, geometry)
    );
    assert!(layout.admissions.acknowledge_control(surface, admission));
    assert!(layout.admissions.begin_retirement(surface, candidate));
    layout
        .presentation_roles
        .insert(surface, SurfacePresentationRole::PolicyManaged);
    let extent = Size {
        width: geometry.width,
        height: geometry.height,
    };
    layout.layout_epochs.record_safe_observation(
        candidate,
        extent,
        sophia_engine::SurfaceVisualEvidence::PresentedBuffer,
    );
    layout
        .awaiting_visual_commits
        .arm(crate::resize_transaction::ResizeVisualCommit {
            candidate,
            size: extent,
            layout_size: extent,
        })
        .unwrap();
    // The same pixmap is not modified; two Presents can name it. This tests
    // distinct transaction ownership rather than pretending to draw while busy.
    let queued = early_successor.then(|| {
        let batch = client.present_batch(82, 0);
        layout.observe_authority_batch(&batch);
        batch
    });
    let mut runtime = LiveProductionVisualRuntime::new(
        &[HeadlessOutput {
            id: OutputId::from_raw(1),
            size: Size {
                width: 2,
                height: 2,
            },
            scale: 1,
        }],
        None,
    )
    .unwrap();
    // Supply the backend's terminal Skip, not a native presentation. Exercise
    // its real drain and socket delivery; no GPU or visibility timer runs here.
    runtime.route_present_feedback(LivePresentFeedbackOutcome {
        feedback: vec![
            LivePresentProtocolFeedback::Complete {
                transaction: candidate.transaction,
                ust: 0,
                msc: 0,
                disposition: LivePresentBufferDisposition::Skipped,
            },
            LivePresentProtocolFeedback::Idle {
                transaction: candidate.transaction,
            },
        ],
        idle_fence_triggered: false,
        layout_comparison: None,
    });
    client
        .observer
        .drain_pending_feedback_with_layout(
            &mut runtime,
            &mut Vec::new(),
            &mut layout,
            |_, _, _| None,
        )
        .unwrap();
    let complete = packet(&mut client.stream);
    let idle = packet(&mut client.stream);
    assert_eq!(complete[11], 2, "wire completion is Skip, never Copy/Flip");
    assert_eq!(u16::from_le_bytes([idle[8], idle[9]]), 2);
    client.barrier();
    assert_eq!(
        layout.admissions.state(surface),
        SurfacePresentationAdmissionState::AwaitingPixels {
            transaction: admission,
            geometry,
        },
        "terminal Skip must not leave admission waiting for impossible native retirement"
    );
    assert_eq!(layout.focus_to_apply, None);
    assert!(!layout.awaiting_visual_commits.surface_awaiting(surface));
    if let Some(queued) = &queued {
        assert_eq!(
            layout
                .layout_epochs
                .safe_observation(surface)
                .unwrap()
                .candidate,
            Some(queued.transactions[0].key())
        );
    } else {
        assert!(layout.layout_epochs.safe_observation(surface).is_none());
    }
    assert!(!layout.complete_admission_retirement(candidate));

    // Reuse only after the exact Idle. An old repeated terminal must not undo
    // the successor's independent retirement or its retained observation.
    let next = queued.unwrap_or_else(|| client.present_batch(82, 0));
    let successor = next.transactions.first().unwrap().key();
    assert_ne!(candidate.transaction, successor.transaction);
    if !early_successor {
        layout.observe_authority_batch(&next);
    }
    let recovery = TransactionId::from_raw(8001);
    let proposal = crate::live_session::LiveWmProposal {
        transaction: recovery,
        layers: vec![sophia_protocol::LayerSnapshot {
            input_region: None,
            translation: None,
            output: None,
            surface,
            authority_local_id: None,
            namespace: None,
            stack_rank: 0,
            geometry,
            source_size: extent,
            source: sophia_protocol::BufferSource::None,
            damage: sophia_protocol::Region::single(geometry),
            opacity: 1.0,
            crop: None,
            transform: sophia_protocol::Transform::IDENTITY,
            generation: 1,
            resize_sync: sophia_protocol::ResizeSyncCapability::ImplicitOnly,
        }],
        requested_sizes: std::collections::BTreeMap::from([(surface, extent)]),
        presentation_states: std::collections::BTreeMap::new(),
        configure_deliveries: 0,
        focus: None,
        timeout: Duration::from_secs(1),
        update: sophia_engine::WmTransactionUpdate {
            commit: sophia_protocol::TransactionCommit {
                transaction: recovery,
                outcome: sophia_protocol::TransactionOutcome::Committed,
                applied_surfaces: vec![surface],
            },
        },
        moved_surfaces: 0,
        source: None,
        policy_settlement: None,
    };
    assert!(
        layout
            .stage(
                proposal,
                &mut crate::session_control::SessionControlQueue::default()
            )
            .unwrap()
            .is_none()
    );
    assert!(layout.resolve_pending().is_some());
    let (_, released) =
        layout.projected_batch(&wm_update_coordinator_batch(TransactionId::from_raw(8002)));
    assert_eq!(released.len(), 1, "successor leaves admission quarantine");
    assert_eq!(released[0].transactions[0].key(), successor);
    let stale = LivePresentFeedbackOutcome {
        feedback: vec![LivePresentProtocolFeedback::Complete {
            transaction: candidate.transaction,
            ust: 0,
            msc: 0,
            disposition: LivePresentBufferDisposition::Skipped,
        }],
        idle_fence_triggered: false,
        layout_comparison: None,
    };
    layout.observe_terminal_present_feedback(&stale);
    assert!(
        layout
            .awaiting_visual_commits
            .exact_candidate(successor, extent)
    );
    assert_eq!(
        layout
            .layout_epochs
            .safe_observation(surface)
            .unwrap()
            .candidate,
        Some(successor)
    );
    assert_eq!(
        layout.admissions.state(surface),
        SurfacePresentationAdmissionState::AwaitingRetirement {
            admission_transaction: admission,
            visual_candidate: successor,
            geometry,
        }
    );
    assert!(
        !layout
            .admissions
            .reject_retirement(sophia_protocol::SurfaceTransactionKey {
                target_buffer: sophia_protocol::BufferSource::None,
                ..successor
            })
    );
    assert!(layout.complete_visual_commit(successor, extent));
    assert!(layout.complete_admission_retirement(successor));
    layout.observe_terminal_present_feedback(&stale);
    assert_eq!(
        layout.admissions.state(surface),
        SurfacePresentationAdmissionState::Managed
    );
}

impl Drop for FeedbackFrontend {
    fn drop(&mut self) {
        let _ = self
            .stop
            .try_send(XServerFrontendServiceCommand::StopAccepting);
        if let Some(thread) = self.thread.take() {
            let result = thread.join();
            if !std::thread::panicking() {
                result.expect("feedback frontend exits cleanly");
            }
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

fn request(opcode: u8, minor: u8, size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    bytes[0] = opcode;
    bytes[1] = minor;
    bytes[2..4].copy_from_slice(&u16::try_from(size / 4).unwrap().to_le_bytes());
    bytes
}

fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn packet(stream: &mut UnixStream) -> Vec<u8> {
    let mut bytes = vec![0; 32];
    stream.read_exact(&mut bytes).expect("frontend packet");
    assert_ne!(bytes[0], 0, "unexpected X error: {bytes:?}");
    if matches!(bytes[0], 1 | 35) {
        bytes.resize(32 + usize::try_from(u32_at(&bytes, 4)).unwrap() * 4, 0);
        stream.read_exact(&mut bytes[32..]).unwrap();
    }
    bytes
}

struct FeedbackClient {
    // Disconnect the socket before joining the frontend during unwinding.
    stream: UnixStream,
    observer: XPresentSessionObserver,
    window: u32,
    pixmap: u32,
    event_id: u32,
    _frontend: FeedbackFrontend,
    // Resource teardown may publish observations while the frontend joins.
    observed: mpsc::Receiver<sophia_x_authority::XAuthorityObservedTransactionBatch>,
}

impl FeedbackClient {
    fn new() -> Self {
        let socket = std::env::temp_dir().join(format!(
            "sophia-feedback-{}-{}.sock",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(32).unwrap());
        let observer = XPresentSessionObserver::new(broker.protocol_router());
        let (observations, observed) = mpsc::sync_channel(128);
        let (stop, stopped) = mpsc::sync_channel(1);
        let config = XServerFrontendConfig::new(&socket, NamespaceId::from_raw(997)).unwrap();
        let thread = std::thread::spawn(move || {
            run_x_server_frontend_routed_until_stopped(config, observations, broker, stopped)
                .unwrap();
        });
        // Declared before the client so panic unwinding disconnects it before join.
        let _frontend = FeedbackFrontend {
            socket: socket.clone(),
            stop,
            thread: Some(thread),
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match UnixStream::connect(&socket) {
                Ok(stream) => break stream,
                Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(1)),
                Err(error) => panic!("feedback frontend did not bind: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .write_all(&[b'l', 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0])
            .unwrap();
        let mut prefix = [0; 8];
        stream.read_exact(&mut prefix).unwrap();
        assert_eq!(prefix[0], 1, "X setup succeeds");
        let mut setup = vec![0; usize::from(u16::from_le_bytes([prefix[6], prefix[7]])) * 4];
        stream.read_exact(&mut setup).unwrap();
        let base = u32_at(&setup, 4);
        let vendor_len = usize::from(u16::from_le_bytes([setup[16], setup[17]]));
        let root = u32_at(
            &setup,
            32 + vendor_len.next_multiple_of(4) + usize::from(setup[21]) * 8,
        );
        let window = base | 1;
        let pixmap = base | 2;
        let event_id = base | 3;
        let mut create = request(1, 0, 36);
        put(&mut create, 4, window);
        put(&mut create, 8, root);
        create[16..18].copy_from_slice(&2_u16.to_le_bytes());
        create[18..20].copy_from_slice(&2_u16.to_le_bytes());
        create[22..24].copy_from_slice(&1_u16.to_le_bytes());
        put(&mut create, 28, 1 << 9);
        put(&mut create, 32, 1);
        stream.write_all(&create).unwrap();
        let mut map = request(8, 0, 8);
        put(&mut map, 4, window);
        stream.write_all(&map).unwrap();
        let mut create_pixmap = request(53, 24, 16);
        put(&mut create_pixmap, 4, pixmap);
        put(&mut create_pixmap, 8, window);
        create_pixmap[12..14].copy_from_slice(&2_u16.to_le_bytes());
        create_pixmap[14..16].copy_from_slice(&2_u16.to_le_bytes());
        stream.write_all(&create_pixmap).unwrap();
        let gc = base | 4;
        let mut create_gc = request(55, 0, 20);
        put(&mut create_gc, 4, gc);
        put(&mut create_gc, 8, pixmap);
        put(&mut create_gc, 12, 1 << 2);
        put(&mut create_gc, 16, 0x214365);
        stream.write_all(&create_gc).unwrap();
        let mut image = request(72, 2, 40);
        put(&mut image, 4, pixmap);
        put(&mut image, 8, gc);
        image[12..14].copy_from_slice(&2_u16.to_le_bytes());
        image[14..16].copy_from_slice(&2_u16.to_le_bytes());
        image[21] = 24;
        image[24..].copy_from_slice(&[0x21, 0x43, 0x65, 0xff].repeat(4));
        stream.write_all(&image).unwrap();
        let mut select = request(X_PRESENT_MAJOR_OPCODE, 3, 16);
        put(&mut select, 4, event_id);
        put(&mut select, 8, window);
        put(&mut select, 12, 6);
        stream.write_all(&select).unwrap();
        Self {
            stream,
            observer,
            observed,
            window,
            pixmap,
            event_id,
            _frontend,
        }
    }

    fn present(&mut self, serial: u32, options: u32) -> TransactionId {
        self.present_batch(serial, options).transaction
    }

    fn present_batch(
        &mut self,
        serial: u32,
        options: u32,
    ) -> sophia_x_authority::XAuthorityObservedTransactionBatch {
        let mut present = request(X_PRESENT_MAJOR_OPCODE, 1, 72);
        put(&mut present, 4, self.window);
        put(&mut present, 8, self.pixmap);
        put(&mut present, 12, serial);
        put(&mut present, 40, options);
        self.stream.write_all(&present).unwrap();
        self.stream.write_all(&request(43, 0, 4)).unwrap();
        assert_eq!(
            packet(&mut self.stream)[0],
            1,
            "Present request was accepted"
        );
        loop {
            let batch = self.observed.recv_timeout(Duration::from_secs(2)).unwrap();
            if !batch.software_present_submissions.is_empty() {
                break batch;
            }
        }
    }

    fn barrier(&mut self) {
        self.stream.write_all(&request(43, 0, 4)).unwrap();
        assert_eq!(
            packet(&mut self.stream)[0],
            1,
            "no extra feedback precedes the reply"
        );
    }
}

#[test]
fn feedback_progress_precedes_no_engine_work_with_a_disarmed_native_deadline() {
    let mut client = FeedbackClient::new();
    let transaction = client.present(71, 0);
    let output = OutputId::from_raw(1);
    let mut runtime = LiveProductionVisualRuntime::new(
        &[HeadlessOutput {
            id: output,
            size: Size {
                width: 2,
                height: 2,
            },
            scale: 1,
        }],
        None,
    )
    .unwrap();
    let native_request = OutputFrameServiceRequest {
        outputs: vec![OutputFrameServiceObservation {
            output,
            primary: true,
            native_phase: OutputNativeFramePhase::Idle,
            pending_frame: false,
        }],
        ..Default::default()
    };
    let deadline_armed = false;
    assert!(!native_frame_service_should_preempt_authority(
        &native_request,
        false,
        false,
        0,
        deadline_armed,
    ));
    // Stand in for an already settled retirement, not a frame submission.
    // The real router owns the transaction from the client's Present request.
    runtime.route_present_feedback(LivePresentFeedbackOutcome {
        feedback: vec![
            LivePresentProtocolFeedback::Idle { transaction },
            LivePresentProtocolFeedback::Complete {
                transaction,
                ust: 100,
                msc: 4,
                disposition: LivePresentBufferDisposition::Copied,
            },
        ],
        idle_fence_triggered: false,
        layout_comparison: None,
    });
    let mut pending = Vec::new();
    let mut skipped = 0;
    for ticket in 100..108 {
        // This is the lifecycle observation boundary used by the owner. The
        // native retirement itself and the include call site remain outside
        // this headless test; neither a timeout nor a WM update rescues it.
        client
            .observer
            .drain_pending_feedback_observed(&mut runtime, &mut pending, |_, _| None)
            .unwrap();
        let batch = wm_update_coordinator_batch(TransactionId::from_raw(ticket));
        if !authority_batch_has_engine_work(&batch) {
            skipped += 1;
            continue;
        }
        panic!("the fixture must take the no-engine-work path");
    }
    assert_eq!(skipped, 8);
    assert_eq!(
        (client.observer.idle_routed, client.observer.complete_routed),
        (1, 1)
    );
    let idle = packet(&mut client.stream);
    let complete = packet(&mut client.stream);
    assert_eq!(
        (idle[0], idle[1], u16::from_le_bytes([idle[8], idle[9]])),
        (35, X_PRESENT_MAJOR_OPCODE, 2)
    );
    assert_eq!(
        (
            complete[0],
            complete[1],
            u16::from_le_bytes([complete[8], complete[9]])
        ),
        (35, X_PRESENT_MAJOR_OPCODE, 1)
    );
    assert_eq!(u32_at(&idle, 20), 71);
    assert_eq!(u32_at(&complete, 20), 71);
    // A round trip after both events makes duplicates visible without a sleep.
    client.stream.write_all(&request(43, 0, 4)).unwrap();
    assert_eq!(
        packet(&mut client.stream)[0],
        1,
        "no duplicate feedback precedes the reply"
    );
}

fn unrelated_layout_comparison() -> sophia_x_authority::XPresentLayoutComparison {
    sophia_x_authority::XPresentLayoutComparison {
        surface: sophia_protocol::SurfaceId::new(990, 1),
        buffer: sophia_protocol::BufferHandle::from_raw(991),
        format: sophia_protocol::DRM_FORMAT_XRGB8888,
        original_modifier: 0x0200_0000_28a6_bf04,
        alternative_modifier: 0,
        preference_generation: 1,
        topology_generation: 1,
        native_context: sophia_x_authority::XWindowAllocationContext {
            generation: 1,
            output: OutputId::from_raw(1),
        },
        device_identity: sophia_x_authority::XRenderDeviceIdentity {
            device: 1,
            inode: 2,
            device_number: 3,
        },
        geometry: sophia_protocol::Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        },
    }
}

fn assert_present_event(bytes: &[u8], client: &FeedbackClient, serial: u32, kind: u16) {
    assert_eq!(bytes[0], 35);
    assert_eq!(bytes[1], X_PRESENT_MAJOR_OPCODE);
    assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), kind);
    assert_eq!(u32_at(bytes, 12), client.event_id);
    assert_eq!(u32_at(bytes, 16), client.window);
    assert_eq!(u32_at(bytes, 20), serial);
    assert_eq!(bytes.len(), if kind == 1 { 40 } else { 32 });
}

#[test]
fn disposition_feedback_preserves_wire_modes_ownership_counters_and_cadence() {
    use LivePresentBufferDisposition::{Copied, Flipped, Retained, Skipped};
    use sophia_x_authority::XPresentCompletionMode::{Copy, Flip, Skip};

    let mut client = FeedbackClient::new();
    // The optional comparison is deliberately unrelated to these CPU pixmaps.
    // Positive native layout proof belongs to the frontend's separate harness.
    let cases = [
        (Copied, Copy, 10_000, false),
        (Retained, Flip, 20_000, true),
        (Flipped, Flip, 30_000, true),
        (Skipped, Skip, 999_000, true),
        (Copied, Copy, 40_000, true),
        (Copied, Copy, 50_000, true),
    ];
    for (index, (disposition, expected_mode, ust, supplied_comparison)) in
        cases.into_iter().enumerate()
    {
        let serial = 100 + u32::try_from(index).unwrap();
        // Opt-in alone cannot promote the source ownership outcome.
        let transaction = client.present(serial, 0x8);
        let msc = u64::from(serial);
        let complete = LivePresentProtocolFeedback::Complete {
            transaction,
            ust,
            msc,
            disposition,
        };
        let idle = LivePresentProtocolFeedback::Idle { transaction };
        let comparison = supplied_comparison.then_some((
            if index == 4 {
                TransactionId::from_raw(transaction.raw() + 1)
            } else {
                transaction
            },
            unrelated_layout_comparison(),
        ));
        let idle_before = client.observer.idle;
        let cadence_before = client.observer.displayed_cadence.previous_ust;
        let feedback = match disposition {
            Copied => vec![idle, complete],
            Retained | Flipped => vec![complete],
            Skipped => vec![complete, idle],
        };
        client.observer.observe_feedback(
            LivePresentFeedbackOutcome {
                feedback,
                idle_fence_triggered: disposition == Copied,
                layout_comparison: None,
            },
            comparison,
        );

        if disposition == Copied {
            let event = packet(&mut client.stream);
            assert_present_event(&event, &client, serial, 2);
            assert_eq!(u32_at(&event, 24), client.pixmap);
        }
        let event = packet(&mut client.stream);
        assert_present_event(&event, &client, serial, 1);
        assert_eq!(event[10], 0, "the completion is for a pixmap");
        assert_eq!(event[11], expected_mode as u8, "case {index}");
        assert_eq!(u64::from_le_bytes(event[24..32].try_into().unwrap()), ust);
        assert_eq!(u64::from_le_bytes(event[32..40].try_into().unwrap()), msc);

        if matches!(disposition, Retained | Flipped) {
            assert_eq!(client.observer.idle, idle_before);
            client.barrier();
            // Complete does not release either form of retained source.
            client.observer.observe_feedback(
                LivePresentFeedbackOutcome {
                    feedback: vec![idle],
                    idle_fence_triggered: false,
                    layout_comparison: None,
                },
                None,
            );
        }
        if disposition != Copied {
            let event = packet(&mut client.stream);
            assert_present_event(&event, &client, serial, 2);
            assert_eq!(u32_at(&event, 24), client.pixmap);
        }
        assert_eq!(client.observer.idle, idle_before + 1);
        if disposition == Skipped {
            assert_eq!(
                client.observer.displayed_cadence.previous_ust,
                cadence_before
            );
        } else {
            assert_eq!(client.observer.displayed_cadence.previous_ust, Some(ust));
        }
        client.barrier();
    }

    let observer = &client.observer;
    assert_eq!(observer.complete_copy, 3);
    assert_eq!(observer.complete_flip, 1);
    assert_eq!(observer.complete_direct, 1);
    assert_eq!(observer.complete_flip_modes(), 2);
    assert_eq!(observer.complete_skip, 1);
    assert_eq!((observer.complete_routed, observer.idle_routed), (6, 6));
    assert_eq!(observer.idle_fence_triggers, 3);
    assert_eq!(observer.route_failures, 0);
    let cadence = observer.displayed_cadence.summary().unwrap();
    assert_eq!(cadence.samples, 5);
    assert_eq!(cadence.advancing_intervals, 4);
    assert_eq!(cadence.nonadvancing, 0);
    assert_eq!(cadence.mean_fps, 100.0);
    assert_eq!(cadence.p95_frame_msec, 10.0);
}
