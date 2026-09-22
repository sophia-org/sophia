#[cfg(unix)]
mod pointer_queries {
    use super::*;
    use std::{
        io::{Read, Write},
        num::NonZeroUsize,
        os::unix::net::UnixStream,
        sync::{Arc, mpsc},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    pub(super) struct Fixture {
        path: std::path::PathBuf,
        input: XAuthorityRoutedInputSender,
        controls: mpsc::SyncSender<XAuthorityClientControlCommand>,
        acks: mpsc::Receiver<XAuthorityClientControlAck>,
        owners: std::collections::BTreeMap<SurfaceId, XServerFrontendClientId>,
        deliveries: mpsc::Receiver<XAuthorityClientInputDelivery>,
        transactions: mpsc::Receiver<XAuthorityObservedTransactionBatch>,
        stop: mpsc::SyncSender<XServerFrontendServiceCommand>,
        server: Option<std::thread::JoinHandle<()>>,
        clients: u32,
        serial: u64,
        lease: Option<sophia_protocol::ApplicationRouteLeaseIdentity>,
        lease_updates: mpsc::Receiver<XAuthorityRouteLeaseUpdate>,
    }

    impl Fixture {
        pub(super) fn new(confined: bool) -> Self {
            Self::with_grabs(confined, None)
        }

        fn with_grabs(confined: bool, grabs: Option<XAuthorityExplicitPointerGrabClient>) -> Self {
            let path = std::env::temp_dir().join(format!(
                "sophia-pointer-query-{}-{}.sock",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let (tx, transactions) = mpsc::sync_channel(128);
            let (ack, acks) = mpsc::sync_channel(8);
            let (delivery, deliveries) = mpsc::channel();
            let (lease_tx, lease_updates) = mpsc::sync_channel(128);
            let mut broker =
                XServerFrontendRouteBroker::with_route_capacities_xkb_and_lease_updates(
                    XServerFrontendRouteCapacities::uniform(NonZeroUsize::new(16).unwrap()),
                    ack,
                    delivery,
                    lease_tx,
                    XkbRmlvoConfig::default(),
                )
                .unwrap();
            if let Some(grabs) = grabs {
                broker = broker.with_explicit_pointer_grab_client(grabs);
            }
            let input = broker.routed_input_sender();
            let controls = broker.control_sender();
            let (stop, stopped) = mpsc::sync_channel(1);
            let mut config = XServerFrontendConfig::new(&path, NamespaceId::from_raw(986))
                .unwrap()
                .with_max_concurrent_clients(NonZeroUsize::new(4).unwrap());
            if confined {
                let namespaces = [986, 987].map(|id| {
                    NamespaceContext::new(
                        NamespaceId::from_raw(id),
                        NamespaceProfile::Confined,
                        NamespaceCapabilities::NONE,
                    )
                    .unwrap()
                });
                config = config.with_admission_policy(Arc::new(SequencedXAdmissionPolicy {
                    namespaces,
                    next_client: std::sync::atomic::AtomicU64::new(0),
                    revoked: std::sync::Mutex::new(Vec::new()),
                }));
            }
            let server = std::thread::spawn(move || {
                run_x_server_frontend_routed_until_stopped(config, tx, broker, stopped).unwrap()
            });
            wait_for_socket(&path);
            Self {
                path,
                input,
                controls,
                acks,
                owners: Default::default(),
                deliveries,
                transactions,
                stop,
                server: Some(server),
                clients: 0,
                serial: 0,
                lease: None,
                lease_updates,
            }
        }

        pub(super) fn connect(&mut self, order: XByteOrder) -> Client {
            let mut stream = connect_x_socket(&self.path);
            stream
                .write_all(&setup_request(order, 11, 0, b"", b""))
                .unwrap();
            read_setup_success(&mut stream, order);
            self.clients += 1;
            Client {
                stream,
                order,
                next: self.clients * 0x0020_0000 + 1,
            }
        }

        pub(super) fn surface(&mut self, client: &mut Client, window: u32) -> SurfaceId {
            client
                .stream
                .write_all(&sophia_present_pixmap_request(
                    client.order,
                    window,
                    window + 0x1000,
                    (0, 0, 16, 16),
                    1,
                    1,
                ))
                .unwrap();
            client.barrier();
            loop {
                let batch = self
                    .transactions
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap();
                if let Some(transaction) = batch.transactions.first() {
                    self.owners
                        .insert(transaction.surface, batch.client.unwrap());
                    return transaction.surface;
                }
            }
        }

        fn send(
            &mut self,
            surface: SurfaceId,
            kind: InputEventKind,
            global: (f64, f64),
            local: (f64, f64),
        ) -> XAuthorityInputDeliveryId {
            self.serial += 1;
            let id = XAuthorityInputDeliveryId::from_raw(self.serial);
            self.input
                .send(XAuthorityRoutedInput {
                    request: RoutedInputRequest {
                        serial: self.serial,
                        seat: SeatId::from_raw(1),
                        device: DeviceId::from_raw(1),
                        time_msec: self.serial,
                        target_surface: surface,
                        global_position: Point {
                            x: global.0,
                            y: global.1,
                        },
                        local_position: Point {
                            x: local.0,
                            y: local.1,
                        },
                        kind,
                    },
                    route_lease: self.lease,
                    delivery: Some(id),
                    mode: XAuthorityRoutedInputMode::Deliver,
                    origin: XAuthorityRoutedInputOrigin::Physical,
                })
                .unwrap();
            id
        }

        pub(super) fn route(&mut self, surface: SurfaceId, kind: InputEventKind) {
            let id = self.send(surface, kind, (143.0, 259.0), (43.0, 59.0));
            let delivered = self
                .deliveries
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            assert_eq!(delivered.delivery, id);
            assert_eq!(delivered.outcome, XAuthorityInputDeliveryOutcome::Flushed);
        }

        fn route_barrier(&mut self, surface: SurfaceId) {
            self.serial += 1;
            self.controls
                .send(XAuthorityClientControlCommand {
                    client: self.owners[&surface],
                    command: XAuthorityControlCommand::FocusSurface {
                        transaction: TransactionId::from_raw(self.serial),
                        surface,
                    },
                })
                .unwrap();
            assert_eq!(
                self.acks
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap()
                    .acknowledgement
                    .outcome,
                XAuthorityControlOutcome::Delivered
            );
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.stop.send(XServerFrontendServiceCommand::StopAccepting);
            if let Some(server) = self.server.take() {
                let result = server.join();
                if !std::thread::panicking() {
                    result.unwrap();
                }
            }
        }
    }

    pub(super) struct Client {
        pub(super) stream: UnixStream,
        pub(super) order: XByteOrder,
        pub(super) next: u32,
    }
    impl Client {
        pub(super) fn window(&mut self, parent: u32, geometry: (i16, i16, u16, u16)) -> u32 {
            let window = self.next;
            self.next += 1;
            let (x, y, w, h) = geometry;
            self.stream
                .write_all(&create_window_request_with_parent(
                    self.order, window, parent, x, y, w, h,
                ))
                .unwrap();
            // Avoid the legacy key-writer startup wait; pointer masks remain empty.
            self.stream
                .write_all(&change_window_event_mask_request(self.order, window, 3))
                .unwrap();
            self.window_request(8, window);
            self.barrier();
            window
        }
        pub(super) fn window_request(&mut self, opcode: u8, window: u32) {
            let mut request = vec![opcode, 0];
            push_u16(&mut request, self.order, 2);
            push_u32(&mut request, self.order, window);
            self.stream.write_all(&request).unwrap();
        }
        pub(super) fn reply(&mut self) -> Vec<u8> {
            loop {
                let mut record = read_x_record(&mut self.stream).to_vec();
                if record[0] == 1 || record[0] == 35 {
                    let len = read_u32(self.order, &record[4..8]) as usize * 4;
                    assert!(len < 65536);
                    record.resize(32 + len, 0);
                    self.stream.read_exact(&mut record[32..]).unwrap();
                }
                if record[0] <= 1 {
                    return record;
                }
            }
        }
        pub(super) fn barrier(&mut self) {
            let mut request = vec![43, 0];
            push_u16(&mut request, self.order, 1);
            self.stream.write_all(&request).unwrap();
            assert_eq!(self.reply()[0], 1);
        }
        fn query(&mut self, window: u32) -> (u32, [i16; 4], u16) {
            self.window_request(38, window);
            let reply = self.reply();
            assert_eq!(reply[0], 1);
            let values = [16, 18, 20, 22].map(|i| read_u16(self.order, &reply[i..i + 2]) as i16);
            (
                read_u32(self.order, &reply[12..16]),
                values,
                read_u16(self.order, &reply[24..26]),
            )
        }
        fn xi_query(&mut self, window: u32) -> (u32, [i16; 4], u16) {
            let mut request = vec![X_INPUT_MAJOR_OPCODE, X_INPUT_QUERY_POINTER_MINOR_OPCODE];
            push_u16(&mut request, self.order, 3);
            push_u32(&mut request, self.order, window);
            push_u16(&mut request, self.order, 2);
            push_u16(&mut request, self.order, 0);
            self.stream.write_all(&request).unwrap();
            let reply = self.reply();
            assert_eq!(reply[0], 1);
            let values = [16, 20, 24, 28]
                .map(|i| ((read_u32(self.order, &reply[i..i + 4]) as i32) >> 16) as i16);
            let buttons = if reply.len() > 56 {
                read_u32(self.order, &reply[56..60])
            } else {
                0
            };
            (
                read_u32(self.order, &reply[12..16]),
                values,
                read_u32(self.order, &reply[48..52]) as u16 | (((buttons >> 1) as u16) << 8),
            )
        }

        fn valuators(&mut self) -> [i64; 4] {
            self.device_valuators(X_INPUT_MASTER_POINTER_ID)
        }

        fn device_valuators(&mut self, device_id: u16) -> [i64; 4] {
            let mut request = vec![X_INPUT_MAJOR_OPCODE, X_INPUT_QUERY_DEVICE_MINOR_OPCODE];
            push_u16(&mut request, self.order, 2);
            push_u16(&mut request, self.order, device_id);
            push_u16(&mut request, self.order, 0);
            self.stream.write_all(&request).unwrap();
            let reply = self.reply();
            assert_eq!(reply[0], 1);
            assert_eq!(read_u16(self.order, &reply[8..10]), 1);
            let classes = read_u16(self.order, &reply[38..40]);
            let name_len = usize::from(read_u16(self.order, &reply[40..42]));
            let mut offset = 44 + ((name_len + 3) & !3);
            let mut values = [0; 4];
            for _ in 0..classes {
                if read_u16(self.order, &reply[offset..offset + 2]) == 2 {
                    let number = usize::from(read_u16(self.order, &reply[offset + 6..offset + 8]));
                    if number < 4 {
                        values[number] = i64::from(read_u32(
                            self.order,
                            &reply[offset + 28..offset + 32],
                        ) as i32);
                        assert_eq!(read_u32(self.order, &reply[offset + 32..offset + 36]), 0);
                    }
                }
                offset += usize::from(read_u16(self.order, &reply[offset + 2..offset + 4])) * 4;
            }
            values
        }

        fn grab(&mut self, window: u32, frozen: bool) {
            self.start_grab(window, frozen);
            let reply = self.reply();
            assert_eq!(&reply[..2], &[1, 0]);
        }

        fn start_grab(&mut self, window: u32, frozen: bool) {
            let mut request = vec![26, 0];
            push_u16(&mut request, self.order, 6);
            push_u32(&mut request, self.order, window);
            push_u16(&mut request, self.order, 0x7f);
            request.extend_from_slice(&[u8::from(!frozen), 1]);
            for _ in 0..3 {
                push_u32(&mut request, self.order, 0);
            }
            self.stream.write_all(&request).unwrap();
        }

        fn thaw(&mut self) {
            let mut request = vec![35, 0];
            push_u16(&mut request, self.order, 2);
            push_u32(&mut request, self.order, 0);
            self.stream.write_all(&request).unwrap();
            self.barrier();
        }
    }

    #[test]
    fn pointer_query_shares_admitted_position_with_new_connections() {
        for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let mut f = Fixture::new(false);
            let mut owner = f.connect(order);
            let top = owner.window(X_SETUP_DEFAULT_ROOT, (100, 200, 320, 240));
            let surface = f.surface(&mut owner, top);
            let child = owner.window(top, (10, 20, 100, 100));
            f.route(surface, InputEventKind::PointerMotion);
            let mut observer = f.connect(order);
            assert_eq!(
                observer.query(X_SETUP_DEFAULT_ROOT),
                (top, [143, 259, 143, 259], 0)
            );
            assert_eq!(observer.query(top), (child, [143, 259, 43, 59], 0));
            assert_eq!(observer.query(child), (0, [143, 259, 33, 39], 0));
            assert_eq!(observer.xi_query(child), observer.query(child));
            assert_eq!(observer.valuators(), [143, 259, 0, 0]);
            // A later connection's cleanup must not erase its peers' observation.
            {
                let mut short = f.connect(order);
                assert_eq!(short.query(top).1, [143, 259, 43, 59]);
            }
            f.route(
                surface,
                InputEventKind::PointerButton {
                    button: 272,
                    pressed: true,
                },
            );
            assert_eq!(observer.query(top).2, 1 << 8);
            f.route(
                surface,
                InputEventKind::Key {
                    keycode: 42,
                    pressed: true,
                },
            );
            assert_eq!(observer.query(top).2, (1 << 8) | 1);
            assert_eq!(observer.xi_query(top), observer.query(top));
            f.route(
                surface,
                InputEventKind::PointerButton {
                    button: 272,
                    pressed: false,
                },
            );
            f.route(
                surface,
                InputEventKind::Key {
                    keycode: 42,
                    pressed: false,
                },
            );
            assert_eq!(observer.query(top).2, 0);
            f.route(
                surface,
                InputEventKind::PointerAxis {
                    horizontal_v120: 120,
                    vertical_v120: -240,
                },
            );
            assert_eq!(observer.valuators(), [143, 259, 120, -240]);
            assert_eq!(observer.query(top).2, 0);
        }
    }

    #[test]
    fn pointer_query_revalidates_hierarchy_and_stationary_geometry() {
        let mut f = Fixture::new(false);
        let mut owner = f.connect(XByteOrder::LittleEndian);
        let top = owner.window(X_SETUP_DEFAULT_ROOT, (100, 200, 320, 240));
        let surface = f.surface(&mut owner, top);
        let child = owner.window(top, (10, 20, 100, 100));
        let sibling = owner.window(top, (10, 20, 100, 100));
        f.route(surface, InputEventKind::PointerMotion);
        let mut observer = f.connect(XByteOrder::BigEndian);
        assert_eq!(observer.query(top).0, sibling);
        owner.window_request(10, sibling);
        owner.barrier();
        assert_eq!(observer.query(top).0, child);
        owner
            .stream
            .write_all(&configure_window_request(owner.order, child, 3, &[20, 30]))
            .unwrap();
        owner.barrier();
        assert_eq!(observer.query(child).1, [143, 259, 23, 29]);
        f.controls
            .send(XAuthorityClientControlCommand {
                client: f.owners[&surface],
                command: XAuthorityControlCommand::ConfigureSurface {
                    transaction: TransactionId::from_raw(90),
                    surface,
                    geometry: Rect {
                        x: 110,
                        y: 210,
                        width: 320,
                        height: 240,
                    },
                },
            })
            .unwrap();
        assert_eq!(
            f.acks
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .acknowledgement
                .outcome,
            XAuthorityControlOutcome::Delivered
        );
        assert_eq!(observer.query(top).1, [143, 259, 33, 49]);
        assert_eq!(observer.query(child).1, [143, 259, 13, 19]);
        let mut reparent = vec![7, 0];
        push_u16(&mut reparent, owner.order, 4);
        push_u32(&mut reparent, owner.order, child);
        push_u32(&mut reparent, owner.order, sibling);
        push_i16(&mut reparent, owner.order, 0);
        push_i16(&mut reparent, owner.order, 0);
        owner.stream.write_all(&reparent).unwrap();
        owner.barrier();
        assert_eq!(observer.query(top).0, 0);
        owner.window_request(4, child);
        owner.barrier();
        assert_eq!(observer.query(top).0, 0);
        observer.window_request(38, child);
        assert_eq!(observer.reply()[1], XErrorCode::BadWindow.wire_code());
        // Engine's transformed local coordinates remain the anchor for X descendants.
        let id = f.send(
            surface,
            InputEventKind::PointerMotion,
            (300.0, 400.0),
            (25.0, 35.0),
        );
        assert_eq!(
            f.deliveries
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .delivery,
            id
        );
        assert_eq!(observer.query(top).1, [300, 400, 25, 35]);
        assert_eq!(observer.xi_query(top), observer.query(top));
        owner.window_request(4, top);
        owner.barrier();
        assert_eq!(observer.query(X_SETUP_DEFAULT_ROOT).0, 0);
    }

    #[test]
    fn pointer_query_does_not_disclose_another_namespace() {
        let mut f = Fixture::new(true);
        let mut first = f.connect(XByteOrder::LittleEndian);
        let top = first.window(X_SETUP_DEFAULT_ROOT, (100, 200, 320, 240));
        let surface = f.surface(&mut first, top);
        f.route(surface, InputEventKind::PointerMotion);
        let mut second = f.connect(XByteOrder::BigEndian);
        assert_eq!(second.query(X_SETUP_DEFAULT_ROOT), (0, [0; 4], 0));
        assert_eq!(second.xi_query(X_SETUP_DEFAULT_ROOT), (0, [0; 4], 0));
        assert_eq!(second.valuators(), [0; 4]);
        second.window_request(38, top);
        assert_eq!(second.reply()[1], XErrorCode::BadAccess.wire_code());
        f.route(
            surface,
            InputEventKind::Key {
                keycode: 42,
                pressed: true,
            },
        );
        assert_eq!(second.query(X_SETUP_DEFAULT_ROOT).2, 0);
        assert_eq!(first.query(top).2, 1);
    }

    #[test]
    fn pointer_query_keeps_engine_anchor_across_grabs_and_freeze() {
        let mut f = Fixture::new(false);
        let mut owner = f.connect(XByteOrder::LittleEndian);
        let top = owner.window(X_SETUP_DEFAULT_ROOT, (100, 200, 320, 240));
        let surface = f.surface(&mut owner, top);
        f.route(surface, InputEventKind::PointerMotion);
        let mut grabber = f.connect(XByteOrder::BigEndian);
        let popup = grabber.window(X_SETUP_DEFAULT_ROOT, (1000, 1000, 100, 100));
        grabber.grab(popup, false);
        f.route(surface, InputEventKind::PointerMotion);
        assert_eq!(grabber.query(X_SETUP_DEFAULT_ROOT).0, top);
        assert_eq!(grabber.query(top).1, [143, 259, 43, 59]);
        assert_eq!(grabber.query(popup), (0, [143, 259, -857, -741], 0));
        grabber.grab(popup, true);
        let pending = f.send(
            surface,
            InputEventKind::PointerMotion,
            (160.0, 270.0),
            (60.0, 70.0),
        );
        f.route_barrier(surface);
        assert!(f.deliveries.try_recv().is_err());
        assert_eq!(grabber.query(top).1, [143, 259, 43, 59]);
        grabber.thaw();
        let delivered = f.deliveries.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(delivered.delivery, pending);
        assert_eq!(delivered.outcome, XAuthorityInputDeliveryOutcome::Flushed);
        assert_eq!(grabber.query(top).1, [160, 270, 60, 70]);
        grabber.grab(popup, true);
        let revoked = f.send(
            surface,
            InputEventKind::PointerMotion,
            (180.0, 280.0),
            (80.0, 80.0),
        );
        f.route_barrier(surface);
        assert!(f.input.advance_control_epoch(f.input.control_epoch() + 1));
        let delivered = f.deliveries.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(delivered.delivery, revoked);
        assert_eq!(
            delivered.outcome,
            XAuthorityInputDeliveryOutcome::EpochRevoked
        );
        assert_eq!(grabber.query(X_SETUP_DEFAULT_ROOT), (0, [0; 4], 0));
    }
    include!("pointer_grab_lifecycle.rs");
    include!("xi_virtual_source_routing.rs");
}
