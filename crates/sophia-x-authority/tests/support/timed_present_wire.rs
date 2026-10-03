//! Real socket dispatch, bounded reservations, publication and service with
//! supplied clock observations. No KMS or physical timing claim.
use super::*;
use std::io::{Read, Write};
#[path = "timed_present_wire_publication.rs"]
mod publication;

struct WireFixture {
    path: std::path::PathBuf,
    state: X11CoreSocketServerState,
    registry: XServerFrontendRouteRegistry,
    clock: XServerFrontendPresentClockRouter,
    owner: sophia_wake::Wake,
    gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
    observed: Receiver<(TransactionId, u16)>,
    batches: Receiver<XAuthorityObservedTransactionBatch>,
    commands: sophia_wake::SignalSender<XServerFrontendServiceCommand>,
    worker: Option<std::thread::JoinHandle<Result<u64, X11SetupSocketError>>>,
}

impl WireFixture {
    fn new(block: bool) -> Self {
        let path = private_service_socket("timed-wire");
        let owner = sophia_wake::Wake::new().unwrap();
        let service = sophia_wake::WakeSlot::default();
        let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(256).unwrap())
            .with_present_clock_admission(owner.notifier());
        let registry = broker.registry.clone();
        let clock = broker.present_clock_router();
        let mut frontend = XServerFrontend::bind(
            XServerFrontendConfig::new(&path, NamespaceId::from_raw(993))
                .unwrap()
                .with_service_wake(service.clone()),
        )
        .unwrap();
        let state = frontend.state.clone();
        let (sender, batches) = sync_channel(256);
        let egress = Arc::new(XAuthorityOrderedEgress::new(
            sender,
            Arc::new(AtomicBool::new(false)),
            Arc::new(|_| {}),
        ));
        let gate = Arc::new((Mutex::new(!block), std::sync::Condvar::new()));
        let observed_gate = gate.clone();
        let (observed_tx, observed) = channel();
        let observer_egress = egress.clone();
        let observer: Arc<X11CoreTraceObserver> = Arc::new(move |trace| {
            if trace.major_opcode == crate::X_PRESENT_MAJOR_OPCODE
                && [
                    crate::X_PRESENT_NOTIFY_MSC_MINOR_OPCODE,
                    crate::X_PRESENT_PIXMAP_MINOR_OPCODE,
                ]
                .contains(&(trace.minor_opcode as u8))
            {
                observed_tx
                    .send((trace.transaction, trace.sequence))
                    .unwrap();
                let (lock, ready) = &*observed_gate;
                drop(
                    ready
                        .wait_while(lock.lock().unwrap(), |open| !*open)
                        .unwrap(),
                );
            }
            let batch = XAuthorityObservedTransactionBatch::from_dispatch_observation(&trace);
            let receipt = batch.as_ref().map(|b| b.transaction);
            observer_egress.submit_blocking(XAuthorityBoundedEgressEnvelope::new(
                trace.transaction,
                batch,
            ))?;
            Ok(receipt)
        });
        let (commands, inbox) = sync_channel(1);
        let commands = sophia_wake::SignalSender::new(commands, service);
        let worker = std::thread::spawn(move || {
            let mut broker = broker;
            let mut generated = XGeneratedEgress::default();
            drive_routed_service(
                &mut frontend,
                &mut broker,
                &inbox,
                &egress,
                &observer,
                &mut generated,
            )?;
            Ok(generated.admission_passes)
        });
        Self {
            path,
            state,
            registry,
            clock,
            owner,
            gate,
            observed,
            batches,
            commands,
            worker: Some(worker),
        }
    }

    fn release(&self) {
        let (lock, ready) = &*self.gate;
        *lock.lock().unwrap() = true;
        ready.notify_all();
    }

    fn wake_ready(&self) -> bool {
        let mut fds = [rustix::event::PollFd::new(
            &self.owner,
            rustix::event::PollFlags::IN,
        )];
        sophia_wake::wait(&mut fds, Some(Instant::now())).unwrap();
        fds[0].revents().contains(rustix::event::PollFlags::IN)
    }
}

impl Drop for WireFixture {
    fn drop(&mut self) {
        self.release();
        let _ = self
            .commands
            .send(XServerFrontendServiceCommand::StopAndDisconnect);
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !std::thread::panicking() {
                result.unwrap().unwrap();
            }
        }
        std::fs::remove_file(&self.path).ok();
    }
}

struct Client {
    stream: UnixStream,
    sequence: u16,
    base: u32,
}

impl Client {
    fn new(f: &WireFixture) -> Self {
        let mut stream = UnixStream::connect(&f.path).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .write_all(&[b'l', 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0])
            .unwrap();
        let mut header = [0; 8];
        stream.read_exact(&mut header).unwrap();
        assert_eq!(header[0], 1);
        let mut body =
            vec![0; usize::from(u16::from_le_bytes(header[6..8].try_into().unwrap())) * 4];
        stream.read_exact(&mut body).unwrap();
        Self {
            stream,
            sequence: 0,
            base: u32::from_le_bytes(body[4..8].try_into().unwrap()),
        }
    }
    fn send(&mut self, bytes: &[u8]) -> u16 {
        self.stream.write_all(bytes).unwrap();
        self.sequence = self.sequence.wrapping_add(1);
        self.sequence
    }
    fn record(&mut self) -> Vec<u8> {
        let mut data = vec![0; 32];
        self.stream.read_exact(&mut data).unwrap();
        if matches!(data[0], 1 | 35) {
            let extra = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize * 4;
            data.resize(32 + extra, 0);
            self.stream.read_exact(&mut data[32..]).unwrap();
        }
        data
    }
    fn silent(&mut self) {
        self.stream
            .set_read_timeout(Some(Duration::from_millis(25)))
            .unwrap();
        let mut one = [0];
        assert!(
            self.stream
                .read_exact(&mut one)
                .is_err_and(|e| matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut))
        );
        self.stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
    }
    fn barrier(&mut self) {
        let seq = self.send(&[43, 0, 1, 0]);
        let reply = self.record();
        assert_eq!(reply[0], 1);
        assert_eq!(u16::from_le_bytes(reply[2..4].try_into().unwrap()), seq);
    }
    fn window(&mut self) -> u32 {
        let window = self.base + 1;
        let mut bytes = vec![1, 0, 8, 0];
        for v in [window, crate::X_SETUP_DEFAULT_ROOT] {
            bytes.extend(v.to_le_bytes());
        }
        for v in [0u16, 0, 4, 4, 0, 1] {
            bytes.extend(v.to_le_bytes());
        }
        bytes.extend([0u8; 8]);
        self.send(&bytes);
        self.barrier();
        window
    }
    fn subscribe(&mut self, window: u32) {
        let mut bytes = vec![
            crate::X_PRESENT_MAJOR_OPCODE,
            crate::X_PRESENT_SELECT_INPUT_MINOR_OPCODE,
            4,
            0,
        ];
        for v in [self.base + 2, window, 6] {
            bytes.extend(v.to_le_bytes());
        }
        self.send(&bytes);
        self.barrier();
    }
    fn notify(
        &mut self,
        window: u32,
        serial: u32,
        target: u64,
        divisor: u64,
        remainder: u64,
    ) -> u16 {
        let mut bytes = vec![
            crate::X_PRESENT_MAJOR_OPCODE,
            crate::X_PRESENT_NOTIFY_MSC_MINOR_OPCODE,
            10,
            0,
        ];
        for v in [window, serial, 0] {
            bytes.extend(v.to_le_bytes());
        }
        for v in [target, divisor, remainder] {
            bytes.extend(v.to_le_bytes());
        }
        self.send(&bytes)
    }
    fn complete(&mut self, kind: u8, serial: u32, msc: u64) -> Vec<u8> {
        let record = self.record();
        assert_eq!(record[0], 35);
        assert_eq!(record.len(), 40);
        assert_eq!(record[10], kind);
        assert_eq!(
            u32::from_le_bytes(record[20..24].try_into().unwrap()),
            serial
        );
        assert_eq!(u64::from_le_bytes(record[32..40].try_into().unwrap()), msc);
        record
    }
}

fn hardware(msc: u64) -> crate::XPresentClockSample {
    crate::XPresentClockSample {
        source: crate::XPresentClockSource::Hardware {
            domain: 993,
            incarnation: 1,
        },
        ust: msc * 1000,
        msc,
    }
}

#[test]
fn notify_wire_waits_for_publication_then_wakes_the_owner_and_observes_its_target() {
    let f = WireFixture::new(true);
    let mut client = Client::new(&f);
    let window = client.window();
    client.subscribe(window);
    f.owner.clear().unwrap();
    let sequence = client.notify(window, 72, 12, 0, 0);
    let (id, seen) = f.observed.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(seen, sequence);
    assert!(f.clock.admissions().unwrap().is_empty());
    assert!(!f.clock.bind_admission(id, hardware(10), None).unwrap());
    assert!(!f.wake_ready());
    let stats = f.clock.wire_timing_statistics().unwrap();
    assert_eq!(
        (
            stats.wire_prepared,
            stats.wire_published,
            stats.wire_owner_notifications,
            stats.wire_bound
        ),
        (1, 0, 0, 0)
    );
    client.silent();
    f.release();
    assert!(waited_for(|| f.wake_ready()));
    let admissions = f.clock.admissions().unwrap();
    assert_eq!(admissions.len(), 1);
    assert_eq!(admissions[0].request, id);
    assert_eq!(admissions[0].target, None);
    f.clock.bind_admission(id, hardware(10), None).unwrap();
    client.silent();
    f.clock.observe_source(hardware(12)).unwrap();
    let record = client.complete(1, 72, 12);
    assert_eq!(
        u16::from_le_bytes(record[2..4].try_into().unwrap()),
        sequence
    );
    assert_eq!(record[11], 0);
    client.silent();
    client.barrier();
    assert_eq!(
        f.state.runtime.lock().unwrap().prepared_msc_notify_count(),
        0
    );
    assert!(
        f.registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .is_empty()
    );
    assert!(
        f.batches
            .try_iter()
            .all(|b| b.software_present_submissions.is_empty() && b.present_submissions.is_empty())
    );
}

#[test]
fn invalid_notify_wire_keeps_its_sequence_and_does_not_publish_clock_work() {
    let f = WireFixture::new(false);
    let mut client = Client::new(&f);
    let window = client.window();
    f.owner.clear().unwrap();
    let seq = client.notify(window, 73, 12, 0, 1);
    let error = client.record();
    assert_eq!(error[0], 0);
    assert_eq!(error[1], 2);
    assert_eq!(u16::from_le_bytes(error[2..4].try_into().unwrap()), seq);
    assert!(f.clock.admissions().unwrap().is_empty());
    assert!(!f.wake_ready());
    client.barrier();
}

impl Client {
    fn xid_request(&mut self, opcode: u8, id: u32) {
        let mut bytes = vec![opcode, 0, 2, 0];
        bytes.extend(id.to_le_bytes());
        self.send(&bytes);
    }
    fn pixmap(&mut self, pixel: u8) -> u32 {
        let pixmap = self.base + 3;
        let gc = self.base + 4;
        let mut bytes = vec![53, 24, 4, 0];
        bytes.extend(pixmap.to_le_bytes());
        bytes.extend(crate::X_SETUP_DEFAULT_ROOT.to_le_bytes());
        bytes.extend(4u16.to_le_bytes());
        bytes.extend(4u16.to_le_bytes());
        self.send(&bytes);
        let mut bytes = vec![55, 0, 4, 0];
        for v in [gc, pixmap, 0] {
            bytes.extend(v.to_le_bytes());
        }
        self.send(&bytes);
        let mut bytes = vec![72, 2, 22, 0];
        for v in [pixmap, gc] {
            bytes.extend(v.to_le_bytes());
        }
        for v in [4u16, 4, 0, 0] {
            bytes.extend(v.to_le_bytes());
        }
        bytes.extend([0, 24, 0, 0]);
        bytes.extend([pixel; 64]);
        self.send(&bytes);
        self.xid_request(60, gc);
        self.barrier();
        pixmap
    }
    fn present(&mut self, window: u32, pixmap: u32, serial: u32, target: u64) -> u16 {
        let mut bytes = vec![
            crate::X_PRESENT_MAJOR_OPCODE,
            crate::X_PRESENT_PIXMAP_MINOR_OPCODE,
            18,
            0,
        ];
        for v in [window, pixmap, serial] {
            bytes.extend(v.to_le_bytes());
        }
        bytes.resize(48, 0);
        for v in [target, 0, 0] {
            bytes.extend(v.to_le_bytes());
        }
        self.send(&bytes)
    }
    fn image(&mut self, window: u32) -> Vec<u8> {
        let mut bytes = vec![73, 2, 5, 0];
        bytes.extend(window.to_le_bytes());
        for v in [0u16, 0, 4, 4] {
            bytes.extend(v.to_le_bytes());
        }
        bytes.extend(u32::MAX.to_le_bytes());
        self.send(&bytes);
        let reply = self.record();
        assert_eq!(reply[0], 1, "GetImage failed: {reply:?}");
        reply[32..].to_vec()
    }
    fn idle(&mut self, serial: u32, pixmap: u32) {
        let idle = self.record();
        assert_eq!(idle[0], 35);
        assert_eq!(u16::from_le_bytes(idle[8..10].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(idle[20..24].try_into().unwrap()), serial);
        assert_eq!(u32::from_le_bytes(idle[24..28].try_into().unwrap()), pixmap);
    }
}

fn published(f: &WireFixture, id: TransactionId) {
    assert!(waited_for(|| f
        .clock
        .admissions()
        .unwrap()
        .iter()
        .any(|a| a.request == id)));
}
fn no_reservations(f: &WireFixture) -> bool {
    f.registry
        .pending_presentations
        .entries
        .lock()
        .unwrap()
        .is_empty()
}

#[test]
fn pixmap_wire_retains_old_content_until_execution_and_rekeys_feedback_after_publication() {
    let f = WireFixture::new(true);
    let mut client = Client::new(&f);
    let window = client.window();
    let pixmap = client.pixmap(0x55);
    client.subscribe(window);
    // Window creation publishes its real route; all content comes from
    // wire requests and the normal timed execution path.
    let xid = XResourceId::new(u64::from(window), 1);
    let ns = NamespaceId::from_raw(993);
    let (_, surface, _, _) = f
        .state
        .runtime
        .lock()
        .unwrap()
        .window_presentation_root_and_offset(ns, xid)
        .unwrap();
    assert!(
        f.registry
            .surface_route_observation(surface)
            .unwrap()
            .is_some()
    );
    let mut attributes = vec![2, 0, 4, 0];
    for v in [window, 1u32 << 9, 1] {
        attributes.extend(v.to_le_bytes());
    }
    client.send(&attributes);
    client.xid_request(8, window);
    client.barrier();
    let before = client.image(window);
    assert_eq!(before.len(), 64);
    assert_ne!(before, vec![0x55; 64]);
    let _ = f.batches.try_iter().collect::<Vec<_>>();
    client.present(window, pixmap, 81, 12);
    let (id, _) = f.observed.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(!f.clock.bind_admission(id, hardware(10), None).unwrap());
    assert!(f.batches.try_recv().is_err());
    assert!(
        f.registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .contains_key(&id)
    );
    f.release();
    published(&f, id);
    f.clock.bind_admission(id, hardware(10), None).unwrap();
    assert_eq!(client.image(window), before);
    // Public FreePixmap and XID reuse must not replace the retained backing.
    client.xid_request(54, pixmap);
    assert_eq!(client.pixmap(0x77), pixmap);
    assert_eq!(client.image(window), before);
    assert!(
        f.batches
            .try_iter()
            .all(|b| b.software_present_submissions.is_empty())
    );
    f.clock.observe_source(hardware(11)).unwrap(); // hardware lead of one field
    let batch = loop {
        let b = f.batches.recv_timeout(Duration::from_secs(5)).unwrap();
        if !b.software_present_submissions.is_empty() {
            break b;
        }
    };
    assert!(batch.transaction.raw() > id.raw());
    assert_eq!(batch.software_present_submissions.len(), 1);
    assert_eq!(batch.transactions[0].surface, surface);
    // Present publishes the presentation raster. GetImage reads the separate
    // core-drawing backing (pre-existing); inspect the renderer payload here.
    let pixels = batch
        .cpu_buffer_updates
        .iter()
        .flat_map(|update| match update {
            crate::XAuthorityCpuBufferUpdate::Replace(snapshot) => vec![snapshot.bytes.as_slice()],
            crate::XAuthorityCpuBufferUpdate::Patch(patch) => vec![patch.bytes.as_slice()],
            crate::XAuthorityCpuBufferUpdate::PatchBatch(batch) => {
                batch.patches.iter().map(|p| p.bytes.as_slice()).collect()
            }
        })
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(pixels.len(), 64);
    assert!(pixels.chunks_exact(4).all(|p| p[..3] == [0x55; 3]));
    assert!(
        !f.registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .contains_key(&id)
    );
    client.silent(); // execution is not Complete or Idle permission
    f.clock.observe_source(hardware(12)).unwrap();
    assert!(
        f.registry
            .route_bound_present_complete(
                batch.transaction,
                hardware(12),
                XPresentCompletionMode::Copy,
                None
            )
            .unwrap()
            .routed
    );
    client.complete(0, 81, 12);
    let stats = f.clock.wire_timing_statistics().unwrap();
    assert_eq!(
        (
            stats.wire_prepared,
            stats.wire_published,
            stats.wire_owner_notifications,
            stats.wire_bound,
            stats.wire_hardware_bound,
            stats.wire_executions
        ),
        (1, 1, 1, 1, 1, 1)
    );
    assert_eq!(
        stats.wire_execution_wait_usec,
        stats.wire_execution_wait_max_usec
    );
    assert!(stats.wire_execution_wait_usec > 0);
    assert!(f.registry.route_present_idle(batch.transaction).unwrap());
    client.idle(81, pixmap);
    assert!(no_reservations(&f));
    client.silent();
}

#[test]
fn rootless_pixmap_wire_skips_at_its_bound_target_without_an_execution_ticket() {
    let f = WireFixture::new(false);
    let mut client = Client::new(&f);
    let window = crate::X_SETUP_DEFAULT_ROOT;
    let pixmap = client.pixmap(0x55);
    client.subscribe(window);
    client.present(window, pixmap, 82, 12);
    let (id, _) = f.observed.recv_timeout(Duration::from_secs(5)).unwrap();
    published(&f, id);
    f.clock.bind_admission(id, hardware(10), None).unwrap();
    client.silent();
    f.clock.observe_source(hardware(11)).unwrap();
    client.idle(82, pixmap);
    // Skip has no hardware lead: Idle is immediate after scrap, but
    // Complete waits for the target field, with no execution ticket.
    client.silent();
    f.clock.observe_source(hardware(12)).unwrap();
    let complete = client.complete(0, 82, 12);
    assert_eq!(complete[11], 2);
    assert!(no_reservations(&f));
    assert!(
        f.batches
            .try_iter()
            .all(|b| b.software_present_submissions.is_empty() && b.present_submissions.is_empty())
    );
    client.barrier();
}

#[test]
fn combined_wire_capacity_refuses_only_the_extra_request_and_destroy_releases_reservations() {
    let f = WireFixture::new(false);
    let mut client = Client::new(&f);
    let window = client.window();
    let pixmap = client.pixmap(0x55);
    for serial in 0..64 {
        client.notify(window, serial, 1000, 0, 0);
    }
    client.barrier();
    assert_eq!(f.clock.admissions().unwrap().len(), 64);
    let seq = client.present(window, pixmap, 99, 1000);
    let error = client.record();
    assert_eq!((error[0], error[1]), (0, 11)); // BadAlloc
    assert_eq!(u16::from_le_bytes(error[2..4].try_into().unwrap()), seq);
    assert!(no_reservations(&f)); // the provisional Pixmap reservation was cancelled
    assert_eq!(f.clock.admissions().unwrap().len(), 64);
    client.xid_request(4, window);
    client.barrier();
    assert!(f.clock.admissions().unwrap().is_empty());
    let window = client.window();
    client.subscribe(window);
    client.notify(window, 100, 10, 0, 0);
    client.barrier();
    let request = f.clock.admissions().unwrap()[0].request;
    f.clock.bind_admission(request, hardware(10), None).unwrap();
    client.complete(1, 100, 10);
    client.silent();
}

#[test]
fn pixmap_capacity_wait_leaves_runtime_available_and_destroy_releases_all_64() {
    let f = WireFixture::new(false);
    let mut client = Client::new(&f);
    let window = client.window();
    let pixmap = client.pixmap(0x55);
    for serial in 0..64 {
        client.present(window, pixmap, serial, 1000);
    }
    client.barrier();
    assert_eq!(f.clock.admissions().unwrap().len(), 64);
    let seq = client.present(window, pixmap, 65, 1000);
    assert!(waited_for(|| f
        .registry
        .pending_presentations
        .capacity_waits
        .load(Ordering::Relaxed)
        > 0));
    // The blocked socket must not prevent execution/cancellation on another thread.
    assert!(f.state.runtime.try_lock().is_ok());
    let error = client.record();
    assert_eq!((error[0], error[1]), (0, 11));
    assert_eq!(u16::from_le_bytes(error[2..4].try_into().unwrap()), seq);
    client.xid_request(4, window);
    client.barrier();
    assert!(no_reservations(&f));
    assert_eq!(f.state.runtime.lock().unwrap().prepared_present_count(), 0);
    let window = client.window();
    client.present(window, pixmap, 66, 1000);
    client.barrier();
    assert_eq!(f.clock.admissions().unwrap().len(), 1);
    drop(client);
    assert!(waited_for(
        || no_reservations(&f) && f.clock.admissions().unwrap().is_empty()
    ));
}

#[test]
fn no_native_owner_notify_uses_fake_deadline_without_more_socket_requests() {
    let f = WireFixture::new(false);
    let mut client = Client::new(&f);
    let window = client.window();
    client.subscribe(window);
    let sample = crate::XPresentClockSample::background(present_monotonic_usec());
    client.notify(window, 90, sample.msc + 1, 0, 0);
    let (id, _) = f.observed.recv_timeout(Duration::from_secs(5)).unwrap();
    published(&f, id);
    assert_eq!(f.clock.admissions().unwrap()[0].target, None);
    let start = Instant::now();
    f.clock.bind_admission(id, sample, None).unwrap();
    let event = client.record();
    assert_eq!((event[0], event[10]), (35, 1));
    assert!(u64::from_le_bytes(event[32..40].try_into().unwrap()) > sample.msc);
    assert!(start.elapsed() < Duration::from_secs(2));
    client.silent();
}

#[test]
fn disconnect_before_publication_cancels_both_kinds_without_clock_or_feedback_debt() {
    for pixmap_request in [false, true] {
        let f = WireFixture::new(true);
        let mut client = Client::new(&f);
        let window = client.window();
        let pixmap = client.pixmap(0x55);
        if pixmap_request {
            client.present(window, pixmap, 91, 1000);
        } else {
            client.notify(window, 91, 1000, 0, 0);
        }
        f.observed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(f.clock.admissions().unwrap().is_empty());
        drop(client);
        f.release();
        assert!(waited_for(|| {
            let runtime = f.state.runtime.lock().unwrap();
            runtime.prepared_present_count() == 0 && runtime.prepared_msc_notify_count() == 0
        }));
        assert!(no_reservations(&f));
    }
}

#[test]
fn timed_wire_resource_errors_precede_timing_errors_and_cancel_pixmap_reservations() {
    let f = WireFixture::new(false);
    let mut client = Client::new(&f);
    let window = client.window();
    let pixmap = client.pixmap(0x55);
    for (target, source, options, expected) in [
        (window + 99, pixmap, 4u32, 3u8), // BadWindow before unsupported UST
        (window, pixmap + 99, 4, 4),      // BadPixmap before unsupported UST
        (window, pixmap, 4, 2),           // UST remains a documented BadValue
        (window, pixmap, 16, 2),          // Present 1.2: no AsyncMayTear
    ] {
        let mut bytes = vec![
            crate::X_PRESENT_MAJOR_OPCODE,
            crate::X_PRESENT_PIXMAP_MINOR_OPCODE,
            18,
            0,
        ];
        for v in [target, source, 92] {
            bytes.extend(v.to_le_bytes());
        }
        bytes.resize(72, 0);
        bytes[40..44].copy_from_slice(&options.to_le_bytes());
        let sequence = client.send(&bytes);
        let error = client.record();
        assert_eq!((error[0], error[1]), (0, expected));
        assert_eq!(
            u16::from_le_bytes(error[2..4].try_into().unwrap()),
            sequence
        );
        assert!(no_reservations(&f));
        assert!(f.clock.admissions().unwrap().is_empty());
        assert_eq!(f.state.runtime.lock().unwrap().prepared_present_count(), 0);
    }
    let sequence = client.notify(window + 99, 93, 0, 0, 1);
    let error = client.record();
    assert_eq!((error[0], error[1]), (0, 3)); // window validation precedes invalid modulus
    assert_eq!(
        u16::from_le_bytes(error[2..4].try_into().unwrap()),
        sequence
    );
    client.barrier();
}

#[test]
fn bound_notify_destroyed_before_target_never_emits_a_late_event() {
    let f = WireFixture::new(false);
    let mut client = Client::new(&f);
    let window = client.window();
    client.subscribe(window);
    client.notify(window, 94, 12, 0, 0);
    let (id, _) = f.observed.recv_timeout(Duration::from_secs(5)).unwrap();
    published(&f, id);
    f.clock.bind_admission(id, hardware(10), None).unwrap();
    client.xid_request(4, window);
    client.barrier();
    f.clock.observe_source(hardware(12)).unwrap();
    client.silent();
    assert_eq!(
        f.state.runtime.lock().unwrap().prepared_msc_notify_count(),
        0
    );
    assert!(f.clock.bound_sources().unwrap().is_empty());
}

#[test]
fn several_clients_execute_earlier_targets_first_without_per_present_owner_round_trips() {
    let mut f = WireFixture::new(false);
    let mut clients = Vec::new();
    for _ in 0..8 {
        let mut client = Client::new(&f);
        let window = client.window();
        let pixmap = client.pixmap(0x55);
        client.subscribe(window);
        client.present(window, pixmap, 101, 14);
        client.present(window, pixmap, 102, 12);
        client.barrier();
        clients.push((client, pixmap));
    }
    let admissions = f.clock.admissions().unwrap();
    assert_eq!(admissions.len(), 16);
    for admission in admissions {
        assert!(
            f.clock
                .bind_admission(admission.request, hardware(10), None)
                .unwrap()
        );
    }
    let mut previous_execution = 0;
    for (serial, field) in [(102, 12), (101, 14)] {
        // One source observation releases the whole ready batch. The test
        // supplies no authority consumption/reply between executions.
        f.clock.observe_source(hardware(field - 1)).unwrap();
        assert!(waited_for(|| {
            f.registry
                .pending_presentations
                .entries
                .lock()
                .unwrap()
                .values()
                .filter(|pending| pending.serial == serial && pending.clock.is_some())
                .count()
                == 8
        }));
        let mut executions = Vec::new();
        for _ in 0..8 {
            let batch = loop {
                let batch = f.batches.recv_timeout(Duration::from_secs(5)).unwrap();
                if !batch.software_present_submissions.is_empty() {
                    break batch;
                }
            };
            assert!(batch.transaction.raw() > previous_execution);
            previous_execution = batch.transaction.raw();
            assert_eq!(
                f.registry.pending_presentations.entries.lock().unwrap()[&batch.transaction].serial,
                serial
            );
            executions.push(batch.transaction);
        }
        for transaction in executions {
            assert!(
                f.registry
                    .route_bound_present_complete(
                        transaction,
                        hardware(field),
                        XPresentCompletionMode::Copy,
                        None
                    )
                    .unwrap()
                    .routed
            );
            assert!(f.registry.route_present_idle(transaction).unwrap());
        }
        for (client, pixmap) in &mut clients {
            client.complete(0, serial, field);
            client.idle(serial, *pixmap);
        }
    }
    assert!(no_reservations(&f));
    let stats = f.clock.wire_timing_statistics().unwrap();
    assert_eq!(
        (
            stats.wire_prepared,
            stats.wire_published,
            stats.wire_owner_notifications,
            stats.wire_bound,
            stats.wire_hardware_bound,
            stats.wire_executions
        ),
        (16, 16, 16, 16, 16, 16)
    );
    assert!(stats.wire_execution_wait_usec >= stats.wire_execution_wait_max_usec);
    f.commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .unwrap();
    let passes = f.worker.take().unwrap().join().unwrap().unwrap();
    // Includes setup, request and idle-wake visits; not a CPU/latency result.
    eprintln!("timed_wire_batch clients=8 executions=16 total_service_passes={passes}");
    assert!(passes >= 16);
}
