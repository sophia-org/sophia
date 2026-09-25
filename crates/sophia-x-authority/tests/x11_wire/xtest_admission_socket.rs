// XTEST admitted to the routed frontend, over real sockets.
//
// WHAT THESE PROVE. Admission is per connection and keyed on the namespace
// the client was admitted into: one client is issued an injector and another,
// in a second namespace, is not -- for it XTEST is absent from discovery and
// every opcode answers BadAccess. An injected key lands on the focused window
// of the injector's own namespace, and the connection is served again after
// it, which is the completion barrier settling after the effect rather than
// the connection parking on it for ever. And a synthetic press that would
// complete the reserved chord, while the seat holds both its modifiers
// physically, is refused on the shared path without parking the connection.

#[cfg(unix)]
mod xtest_admission_socket {
    use super::*;
    use std::{
        io::Write,
        num::NonZeroUsize,
        os::unix::net::UnixStream,
        sync::{Arc, mpsc},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    /// Issues an injector to clients in one namespace and denies the rest.
    struct AdmitOneNamespace {
        admitted: NamespaceId,
        sender: XAuthorityRoutedInputSender,
    }

    impl XServerFrontendInjectionPolicy for AdmitOneNamespace {
        fn issue(
            &self,
            context: ClientAdmissionContext,
            _device: DeviceId,
        ) -> Result<Box<dyn XTestInjector>, XServerFrontendInjectionError> {
            if context.namespace.id != self.admitted {
                return Err(XServerFrontendInjectionError::Denied);
            }
            Ok(Box::new(RoutedXTestInjector::new(
                self.sender.clone(),
                SeatId::from_raw(1),
                DeviceId::from_raw(3),
            )))
        }
    }

    pub(super) struct XtestClient {
        pub(super) stream: UnixStream,
        pub(super) order: XByteOrder,
        pub(super) next: u32,
    }

    impl XtestClient {
        /// GetInputFocus and its reply: a round trip that only completes if the
        /// connection is still being served.
        pub(super) fn barrier(&mut self) {
            let mut request = vec![43, 0];
            push_u16(&mut request, self.order, 1);
            self.stream.write_all(&request).unwrap();
            assert_eq!(read_x_record(&mut self.stream)[0], 1, "the barrier must answer with a reply");
        }

        /// The barrier for a connection that may also have events queued:
        /// GetInputFocus, then read past events to its reply.
        pub(super) fn settle(&mut self) {
            let mut request = vec![43, 0];
            push_u16(&mut request, self.order, 1);
            self.stream.write_all(&request).unwrap();
            for _ in 0..32 {
                if read_x_record(&mut self.stream)[0] == 1 {
                    return;
                }
            }
            panic!("the settle barrier never answered with a reply");
        }

        /// A FakeInput carrying root coordinates, which only motion reads.
        fn fake_input_at(&mut self, event_type: u8, detail: u8, x: i16, y: i16) {
            let mut body = [0u8; 32];
            body[0] = event_type;
            body[1] = detail;
            let (x, y) = match self.order {
                XByteOrder::LittleEndian => (x.to_le_bytes(), y.to_le_bytes()),
                XByteOrder::BigEndian => (x.to_be_bytes(), y.to_be_bytes()),
            };
            body[20..22].copy_from_slice(&x);
            body[22..24].copy_from_slice(&y);
            self.stream
                .write_all(&xtest_request(self.order, X_TEST_FAKE_INPUT_MINOR_OPCODE, &body))
                .unwrap();
        }

        /// The next event of `code`, passing over pointer motion on the way.
        fn next_event(&mut self, code: u8) -> [u8; 32] {
            loop {
                let record = read_x_record(&mut self.stream);
                match record[0] & 0x7f {
                    c if c == code => return record,
                    6 => continue,
                    other => panic!("expected event {code}, got record {other}: {record:?}"),
                }
            }
        }

        fn fake_input(&mut self, event_type: u8, detail: u8) {
            let mut body = [0u8; 32];
            body[0] = event_type;
            body[1] = detail;
            self.stream
                .write_all(&xtest_request(self.order, X_TEST_FAKE_INPUT_MINOR_OPCODE, &body))
                .unwrap();
        }

        /// A round trip that must find nothing queued ahead of its reply.
        fn assert_quiet(&mut self, label: &str) {
            let mut request = vec![43, 0];
            push_u16(&mut request, self.order, 1);
            self.stream.write_all(&request).unwrap();
            let mut stray = Vec::new();
            loop {
                let record = read_x_record(&mut self.stream);
                if record[0] == 1 {
                    break;
                }
                stray.push(record);
            }
            assert!(stray.is_empty(), "{label}: {stray:?}");
        }
    }

    pub(super) struct XtestFixture {
        path: std::path::PathBuf,
        input: XAuthorityRoutedInputSender,
        controls: mpsc::SyncSender<XAuthorityClientControlCommand>,
        acks: mpsc::Receiver<XAuthorityClientControlAck>,
        transactions: mpsc::Receiver<XAuthorityObservedTransactionBatch>,
        stop: mpsc::SyncSender<XServerFrontendServiceCommand>,
        server: Option<std::thread::JoinHandle<()>>,
        clients: u32,
        serial: u64,
    }

    impl XtestFixture {
        pub(super) fn new() -> Self {
            Self::with_client_toplevel_placement(false)
        }

        /// Every client in the one namespace, each with an injector, and
        /// clients placing their own toplevels: peers of one window.
        pub(super) fn sharing_a_namespace() -> Self {
            Self::with_namespaces([901, 901, 901], true)
        }

        /// A further client of the shared namespace.
        pub(super) fn connect_into_namespace_of_owner(&mut self) -> XtestClient {
            self.connect()
        }

        /// The conformance host's posture: clients place their own
        /// toplevels, and the authority resolves which one a point is in.
        pub(super) fn placing_toplevels() -> Self {
            Self::with_client_toplevel_placement(true)
        }

        fn with_client_toplevel_placement(client_places: bool) -> Self {
            Self::with_namespaces([901, 902, 902], client_places)
        }

        fn with_namespaces(namespace_ids: [u64; 3], client_places: bool) -> Self {
            let path = std::env::temp_dir().join(format!(
                "sophia-xtest-admission-{}-{}.sock",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
            ));
            let (tx, transactions) = mpsc::sync_channel(128);
            let (ack, acks) = mpsc::sync_channel(8);
            let (delivery, _deliveries) = mpsc::channel();
            let (lease_tx, _lease_updates) = mpsc::sync_channel(128);
            let broker = XServerFrontendRouteBroker::with_route_capacities_xkb_and_lease_updates(
                XServerFrontendRouteCapacities::uniform(NonZeroUsize::new(16).unwrap()),
                ack,
                delivery,
                lease_tx,
                XkbRmlvoConfig::default(),
            )
            .unwrap();
            let input = broker.routed_input_sender();
            let controls = broker.control_sender();
            let (stop, stopped) = mpsc::sync_channel(1);
            let namespaces = namespace_ids.map(|id| {
                NamespaceContext::new(
                    NamespaceId::from_raw(id),
                    NamespaceProfile::Confined,
                    NamespaceCapabilities::NONE,
                )
                .unwrap()
            });
            let config = XServerFrontendConfig::new(&path, NamespaceId::from_raw(901))
                .unwrap()
                .with_client_toplevel_placement(client_places)
                .with_max_concurrent_clients(NonZeroUsize::new(4).unwrap())
                .with_admission_policy(Arc::new(SequencedXAdmissionPolicy {
                    namespaces: namespaces.to_vec(),
                    next_client: std::sync::atomic::AtomicU64::new(0),
                    revoked: std::sync::Mutex::new(Vec::new()),
                }))
                .with_injection_policy(Arc::new(AdmitOneNamespace {
                    admitted: NamespaceId::from_raw(901),
                    sender: input.clone(),
                }));
            let server = std::thread::spawn(move || {
                run_x_server_frontend_routed_until_stopped(config, tx, broker, stopped).unwrap()
            });
            wait_for_socket(&path);
            Self { path, input, controls, acks, transactions, stop, server: Some(server), clients: 0, serial: 0 }
        }

        pub(super) fn connect(&mut self) -> XtestClient {
            let order = XByteOrder::LittleEndian;
            let mut stream = connect_x_socket(&self.path);
            stream.write_all(&setup_request(order, 11, 0, b"", b"")).unwrap();
            read_setup_success(&mut stream, order);
            self.clients += 1;
            XtestClient { stream, order, next: self.clients * 0x0020_0000 + 1 }
        }

        /// A mapped window with a committed surface, focused on the seat.
        pub(super) fn focused_window(&mut self, client: &mut XtestClient) -> u32 {
            let window = client.next;
            client.next += 2;
            client.stream
                .write_all(&create_window_request(client.order, window, 0, 0, 16, 16))
                .unwrap();
            // KeyPress, KeyRelease and FocusChange: without selecting them the
            // authority delivers nothing to this window, and a test waiting on
            // an event that was never selected does not fail, it parks.
            client.stream
                .write_all(&change_window_event_mask_request(client.order, window, 3 | (1 << 21)))
                .unwrap();
            // A window has to be viewable before anything can focus it, which
            // is what a real client's MapWindow is for.
            client.stream
                .write_all(&map_window_request(client.order, window))
                .unwrap();
            client.stream
                .write_all(&sophia_present_pixmap_request(client.order, window, window + 0x1000, (0, 0, 16, 16), 1, 1))
                .unwrap();
            client.barrier();
            let (owner, surface) = loop {
                let batch = self.transactions.recv_timeout(Duration::from_secs(2)).unwrap();
                if let Some(transaction) = batch.transactions.first() {
                    break (batch.client.unwrap(), transaction.surface);
                }
            };
            self.controls
                .send(XAuthorityClientControlCommand {
                    client: owner,
                    command: XAuthorityControlCommand::FocusSurface {
                        transaction: TransactionId::from_raw(100),
                        surface,
                    },
                })
                .unwrap();
            let ack = self.acks.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(ack.acknowledgement.outcome, XAuthorityControlOutcome::Delivered);
            // The focus was on the root, so descending into this toplevel
            // is an ancestor move rather than a nonlinear one.
            assert_core_focus_event(&mut client.stream, true, window, X_FOCUS_DETAIL_ANCESTOR);
            window
        }

        /// A key from the seat's own keyboard, through the same ingress.
        fn physical_key(&mut self, keycode: u32, pressed: bool) {
            self.serial += 1;
            self.input
                .send(XAuthorityRoutedInput {
                    request: RoutedInputRequest {
                        serial: self.serial,
                        seat: SeatId::from_raw(1),
                        device: DeviceId::from_raw(1),
                        time_msec: self.serial,
                        target_surface: SurfaceId::new(0, 0),
                        global_position: Point::default(),
                        local_position: Point::default(),
                        kind: InputEventKind::Key { keycode, pressed },
                    },
                    route_lease: None,
                    delivery: None,
                    mode: XAuthorityRoutedInputMode::StateOnly,
                    origin: XAuthorityRoutedInputOrigin::Physical,
                })
                .unwrap();
        }
    }

    impl Drop for XtestFixture {
        fn drop(&mut self) {
            let _ = self.stop.send(XServerFrontendServiceCommand::StopAndDisconnect);
            if let Some(server) = self.server.take() {
                let _ = server.join();
            }
            let _ = std::fs::remove_file(&self.path);
        }
    }

    #[test]
    fn xtest_is_issued_to_the_admitted_namespace_and_absent_from_the_other() {
        let mut fixture = XtestFixture::new();
        let mut admitted = fixture.connect(); // namespace 901
        let mut denied = fixture.connect(); // namespace 902

        for (client, present) in [(&mut admitted, 1u8), (&mut denied, 0u8)] {
            client.stream
                .write_all(&query_extension_request(client.order, X_TEST_EXTENSION_NAME))
                .unwrap();
            let reply = read_x_record(&mut client.stream);
            assert_eq!(reply[0], 1);
            assert_eq!(reply[8], present, "QueryExtension present byte");
        }

        admitted.stream
            .write_all(&xtest_request(admitted.order, X_TEST_GET_VERSION_MINOR_OPCODE, &[2, 0, 1, 0]))
            .unwrap();
        let reply = read_x_record(&mut admitted.stream);
        assert_eq!(reply[0], 1, "an admitted client is answered");
        assert_eq!(reply[1], 2, "major");
        assert_eq!(u16::from_le_bytes([reply[8], reply[9]]), 1, "minor");

        denied.stream
            .write_all(&xtest_request(denied.order, X_TEST_GET_VERSION_MINOR_OPCODE, &[2, 0, 1, 0]))
            .unwrap();
        let error = read_x_record(&mut denied.stream);
        assert_eq!(error[0], 0, "a denied client is refused, not answered");
        assert_eq!(error[1], 10, "BadAccess: the request exists and this client may not make it");
        assert_eq!(error[10], X_TEST_MAJOR_OPCODE);
    }

    #[test]
    fn an_injected_key_reaches_the_focused_window_and_the_connection_is_served_after_it() {
        let mut fixture = XtestFixture::new();
        let mut client = fixture.connect();
        let window = fixture.focused_window(&mut client);

        // X keycode 38 is evdev 30; the adapter subtracts the minimum keycode
        // and the keyboard maps it back, so the delivered detail is 38 again.
        client.fake_input(2, 38);
        let press = read_x_record(&mut client.stream);
        assert_eq!(press[0], 2, "KeyPress");
        assert_eq!(press[1], 38);
        assert_eq!(u32::from_le_bytes([press[12], press[13], press[14], press[15]]), window);
        client.fake_input(3, 38);
        let release = read_x_record(&mut client.stream);
        assert_eq!(release[0], 3, "KeyRelease");

        // THE BARRIER SETTLED. FakeInput waits until the registry has taken the
        // effect; a connection parked on a completion nobody reports would never
        // read this request, and the test would hang here rather than fail.
        client.barrier();
    }

    /// A button happens where the pointer is. XTEST's contract ignores a
    /// button request's own root and coordinates, so the press and release
    /// must carry the position the last motion left the pointer at. They were
    /// submitted at `Point::default()`, which delivered every XTEST button at
    /// the screen origin with a crossing around it -- a drag into xterm became
    /// a press and release at one point, a zero-length selection (t155). The
    /// window sits at the origin, so the aim is (5, 7), which only a correct
    /// delivery reports.
    #[test]
    fn an_injected_button_is_delivered_where_the_pointer_is() {
        let mut fixture = XtestFixture::new();
        let mut client = fixture.connect();
        let window = fixture.focused_window(&mut client);
        // ButtonPress, ButtonRelease and PointerMotion beside the fixture's own.
        client.stream
            .write_all(&change_window_event_mask_request(
                client.order,
                window,
                3 | (1 << 21) | (1 << 2) | (1 << 3) | (1 << 6),
            ))
            .unwrap();
        client.barrier();

        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 5, 7);
        client.fake_input(4, 1);
        let press = client.next_event(4);
        client.fake_input(5, 1);
        let release = client.next_event(5);
        for (name, event) in [("ButtonPress", &press), ("ButtonRelease", &release)] {
            let at = |offset: usize| i16::from_le_bytes([event[offset], event[offset + 1]]);
            assert_eq!(event[1], 1, "{name} names button 1");
            assert_eq!(
                u32::from_le_bytes([event[12], event[13], event[14], event[15]]),
                window,
                "{name} is on the window under the pointer"
            );
            assert_eq!((at(20), at(22)), (5, 7), "{name} root position is the pointer's");
            assert_eq!((at(24), at(26)), (5, 7), "{name} window position is the pointer's");
        }
        client.barrier();
    }

    /// A drag is motion with a button down, and the core protocol reports it
    /// on the window that selected Button1Motion whether or not it asked for
    /// PointerMotion. That is xterm: a shell whose text-widget child selects
    /// `<Btn1Motion>: select-extend()` and nothing for plain motion. Motion was
    /// chosen by PointerMotion alone, so during a drag the child never matched
    /// and the record fell back to the shell, which does nothing with it; the
    /// operator saw the selection highlighted only when the button came up
    /// (t162). Red on the tree before the fix: the motion names the shell.
    #[test]
    fn motion_while_button_one_is_held_is_reported_on_the_child_selecting_button_one_motion() {
        let mut fixture = XtestFixture::new();
        let mut client = fixture.connect();
        let shell = fixture.focused_window(&mut client);
        // The text widget: the shell's child across its top half, asking for
        // Button1Motion (1 << 8) and ButtonRelease, and no PointerMotion.
        let text = client.next;
        client.next += 2;
        client.stream
            .write_all(&create_window_request_with_parent(
                client.order,
                text,
                shell,
                0,
                0,
                16,
                8,
            ))
            .unwrap();
        client.stream
            .write_all(&change_window_event_mask_request(
                client.order,
                text,
                (1 << 8) | (1 << 3),
            ))
            .unwrap();
        client.stream
            .write_all(&map_window_request(client.order, text))
            .unwrap();
        client.settle();

        // Which window a motion is reported on is the question here, so
        // records are read past until the drag's own motion arrives.
        let motion_at = |client: &mut XtestClient, x: i16| -> [u8; 32] {
            for _ in 0..16 {
                let record = read_x_record(&mut client.stream);
                if record[0] & 0x7f == 6
                    && i16::from_le_bytes([record[24], record[25]]) == x
                {
                    return record;
                }
            }
            panic!("no MotionNotify at x={x} arrived");
        };
        let event_window =
            |record: &[u8; 32]| u32::from_le_bytes([record[12], record[13], record[14], record[15]]);

        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 4, 4);
        client.settle();
        client.fake_input(4, 1);
        client.settle();
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 9, 4);
        let drag = motion_at(&mut client, 9);
        assert_eq!(
            event_window(&drag),
            text,
            "a drag's motion is reported on the child that selected Button1Motion, not the shell"
        );
        let state = u16::from_le_bytes([drag[28], drag[29]]);
        assert_eq!(state & 0x100, 0x100, "carrying Button1Mask: state={state:#x}");

        client.fake_input(5, 1);
        let release = client.next_event(5);
        assert_eq!(release[1], 1);
        // With the button up the child asked for no motion, so plain motion
        // propagates to the shell once the shell selects it; nobody
        // selecting it would mean no event at all.
        client.stream
            .write_all(&change_window_event_mask_request(client.order, shell, 3 | (1 << 21) | (1 << 6)))
            .unwrap();
        client.settle();
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 12, 4);
        let plain = motion_at(&mut client, 12);
        assert_eq!(event_window(&plain), shell, "plain motion is not the child's");
    }

    #[test]
    fn a_synthetic_press_completing_the_reserved_chord_is_refused_on_the_shared_path() {
        let mut fixture = XtestFixture::new();
        let mut client = fixture.connect();
        let _window = fixture.focused_window(&mut client);

        // Control and Alt held on the seat, physically: evdev 29 and 56, as the
        // session's own guard names them.
        fixture.physical_key(29, true);
        fixture.physical_key(56, true);
        client.barrier();

        // Backspace is evdev 14, X keycode 22. Synthetic, while both are held.
        client.fake_input(2, 22);
        // Nothing was delivered, and the connection is still served: the
        // barrier's reply is the next record, not a KeyPress.
        client.barrier();

        // With the modifiers released the same press is an ordinary key.
        fixture.physical_key(29, false);
        fixture.physical_key(56, false);
        client.barrier();
        client.fake_input(2, 22);
        let press = read_x_record(&mut client.stream);
        assert_eq!(press[0], 2, "KeyPress, once the chord cannot be completed");
        assert_eq!(press[1], 22);
        client.fake_input(3, 22);
        let _ = read_x_record(&mut client.stream);
        client.barrier();
    }

    /// XWarpPointer moves the pointer. On this authority the pointer is the
    /// Engine's, so a warp moves the routed pointer only for a client that
    /// may inject input, through its own injector, as an absolute motion:
    /// the motion and crossing events a warp owes are the ones any motion
    /// owes, and a press after it lands where the warp put the pointer. Red
    /// on the tree before the seam: the warp placed only QueryPointer's
    /// answer, no motion was reported, and the press below landed at the
    /// origin (XTS Xlib11 MotionNotify 1, ButtonPress 1).
    #[test]
    fn a_warp_from_a_client_that_may_inject_moves_the_routed_pointer() {
        let mut fixture = XtestFixture::new();
        let mut client = fixture.connect();
        let window = fixture.focused_window(&mut client);
        // ButtonPress, ButtonRelease and PointerMotion beside the fixture's own.
        client.stream
            .write_all(&change_window_event_mask_request(
                client.order,
                window,
                3 | (1 << 21) | (1 << 2) | (1 << 3) | (1 << 6),
            ))
            .unwrap();
        client.barrier();
        let at = |event: &[u8; 32], offset: usize| i16::from_le_bytes([event[offset], event[offset + 1]]);
        let next_motion = |client: &mut XtestClient| loop {
            let record = read_x_record(&mut client.stream);
            if record[0] & 0x7f == 6 {
                break record;
            }
        };

        // Into the window, at (5, 7) of it.
        client.stream
            .write_all(&warp_pointer_request(client.order, 0, window, 0, 0, 0, 0, 5, 7))
            .unwrap();
        let motion = next_motion(&mut client);
        assert_eq!(
            u32::from_le_bytes([motion[12], motion[13], motion[14], motion[15]]),
            window,
            "the warp is reported as motion on the window it entered"
        );
        assert_eq!((at(&motion, 20), at(&motion, 22)), (5, 7), "root position is the warp's");
        client.fake_input(4, 1);
        let press = client.next_event(4);
        assert_eq!((at(&press, 20), at(&press, 22)), (5, 7), "the press lands where the warp put the pointer");
        assert_eq!((at(&press, 24), at(&press, 26)), (5, 7), "window position too");
        client.fake_input(5, 1);
        let _ = client.next_event(5);

        // No destination window: an offset from where the pointer is.
        client.stream
            .write_all(&warp_pointer_request(client.order, 0, 0, 0, 0, 0, 0, 3, 2))
            .unwrap();
        let motion = next_motion(&mut client);
        assert_eq!((at(&motion, 20), at(&motion, 22)), (8, 9), "a relative warp moves from where the pointer was");

        // A source rectangle the pointer is outside of: no warp, no motion,
        // and the barrier answers with nothing in between.
        client.stream
            .write_all(&warp_pointer_request(client.order, window, window, 12, 12, 4, 4, 1, 1))
            .unwrap();
        client.barrier();
    }

    /// Where clients place their own toplevels (the conformance host), an
    /// injected pointer event goes to the toplevel under the pointer, not to
    /// the focused surface: a suite's plain, unfocused window selecting
    /// ButtonPress is told of a press over it, and motion into it is reported
    /// on it. Red before the fix: with no focused surface a button had no
    /// target and was dropped, and motion was reported on nothing (XTS Xlib11
    /// ButtonPress 1, MotionNotify 1).
    #[test]
    fn a_client_placed_toplevel_under_the_pointer_receives_injected_pointer_events() {
        let mut fixture = XtestFixture::placing_toplevels();
        let mut client = fixture.connect();
        let window = client.next;
        client.next += 2;
        // A plain toplevel at (20, 0), never focused, never presented.
        client.stream
            .write_all(&create_window_request(client.order, window, 20, 0, 16, 16))
            .unwrap();
        client.stream
            .write_all(&change_window_event_mask_request(client.order, window, (1 << 2) | (1 << 3) | (1 << 6)))
            .unwrap();
        client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        client.barrier();
        let at = |event: &[u8; 32], offset: usize| i16::from_le_bytes([event[offset], event[offset + 1]]);
        let event_window = |event: &[u8; 32]| u32::from_le_bytes([event[12], event[13], event[14], event[15]]);

        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 25, 5);
        let motion = loop {
            let record = read_x_record(&mut client.stream);
            if record[0] & 0x7f == 6 {
                break record;
            }
        };
        assert_eq!(event_window(&motion), window, "motion into the toplevel under the pointer");
        assert_eq!((at(&motion, 20), at(&motion, 22), at(&motion, 24), at(&motion, 26)), (25, 5, 5, 5));
        client.fake_input(4, 1);
        let press = client.next_event(4);
        assert_eq!(event_window(&press), window, "the press lands on the toplevel under the pointer");
        assert_eq!((at(&press, 20), at(&press, 22), at(&press, 24), at(&press, 26)), (25, 5, 5, 5));
        client.fake_input(5, 1);
        let release = client.next_event(5);
        assert_eq!(event_window(&release), window);
        // With the focus on the root, PointerRoot applies: a key goes to the
        // toplevel under the pointer too.
        client.stream
            .write_all(&change_window_event_mask_request(client.order, window, 3 | (1 << 2) | (1 << 3) | (1 << 6)))
            .unwrap();
        client.barrier();
        client.fake_input(2, 38);
        let key = client.next_event(2);
        assert_eq!(event_window(&key), window, "a key with the focus on the root lands under the pointer");
        assert_eq!(key[1], 38, "keycode");
        // A key carries the pointer's position: root and event-window coordinates.
        assert_eq!((at(&key, 20), at(&key, 22), at(&key, 24), at(&key, 26)), (25, 5, 5, 5), "the key names where the pointer is");
        client.fake_input(3, 38);
        let _ = client.next_event(3);
        // Over the bare root, a button or a key has nowhere to go and is
        // dropped without parking the connection.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 100, 100);
        client.fake_input(4, 1);
        client.fake_input(5, 1);
        client.fake_input(2, 38);
        client.fake_input(3, 38);
        client.settle();
    }

    /// A core device event reaches every client that selected it on the
    /// event window, not only the window's owner (t220): a peer that
    /// selected motion, button release and keys on the owner's toplevel is
    /// told of each, on that window and at the same coordinates, while the
    /// owner still hears everything it selected. Red before the fix: only
    /// the owner's queue was routed (XTS Xlib11 ButtonRelease 2, KeyPress
    /// 2, MotionNotify 1).
    #[test]
    fn a_peer_that_selected_on_the_owners_window_is_told_of_its_device_events() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut owner = fixture.connect();
        let mut peer = fixture.connect();
        let window = owner.next;
        owner.next += 2;
        // KeyPress, KeyRelease, ButtonPress, ButtonRelease, PointerMotion for
        // the owner; the peer selects the same but ButtonPress, which is one
        // client's to select.
        owner.stream
            .write_all(&create_window_request(owner.order, window, 20, 0, 16, 16))
            .unwrap();
        owner.stream
            .write_all(&change_window_event_mask_request(owner.order, window, 3 | (1 << 2) | (1 << 3) | (1 << 6)))
            .unwrap();
        owner.stream.write_all(&map_window_request(owner.order, window)).unwrap();
        owner.barrier();
        peer.stream
            .write_all(&change_window_event_mask_request(peer.order, window, 3 | (1 << 3) | (1 << 6)))
            .unwrap();
        peer.barrier();
        let at = |event: &[u8; 32], offset: usize| i16::from_le_bytes([event[offset], event[offset + 1]]);
        let event_window = |event: &[u8; 32]| u32::from_le_bytes([event[12], event[13], event[14], event[15]]);
        let next_motion = |client: &mut XtestClient| loop {
            let record = read_x_record(&mut client.stream);
            if record[0] & 0x7f == 6 {
                break record;
            }
        };

        owner.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 25, 5);
        for (name, client) in [("owner", &mut owner), ("peer", &mut peer)] {
            let motion = next_motion(client);
            assert_eq!(event_window(&motion), window, "{name}: motion on the window");
            assert_eq!((at(&motion, 20), at(&motion, 22), at(&motion, 24), at(&motion, 26)), (25, 5, 5, 5), "{name}: motion coordinates");
        }
        owner.fake_input(4, 1);
        let press = owner.next_event(4);
        assert_eq!(event_window(&press), window, "the owner's press");
        owner.fake_input(5, 1);
        for (name, client) in [("owner", &mut owner), ("peer", &mut peer)] {
            let release = client.next_event(5);
            assert_eq!(event_window(&release), window, "{name}: release on the window");
            assert_eq!((at(&release, 20), at(&release, 22)), (25, 5), "{name}: release coordinates");
        }
        owner.fake_input(2, 38);
        for (name, client) in [("owner", &mut owner), ("peer", &mut peer)] {
            let key = client.next_event(2);
            assert_eq!((event_window(&key), key[1]), (window, 38), "{name}: the key on the window");
        }
        owner.fake_input(3, 38);
        for client in [&mut owner, &mut peer] {
            let _ = client.next_event(3);
        }
        // The press the peer did not select never reached it: its stream
        // is quiet now.
        peer.barrier();
        owner.barrier();

        // Propagation is decided once for everyone: a third client that
        // selected motion only on the root hears nothing while the window
        // itself has selectors, and everything once the owner and the peer
        // stop selecting there.
        let mut above = fixture.connect_into_namespace_of_owner();
        above.stream
            .write_all(&change_window_event_mask_request(above.order, X_SETUP_DEFAULT_ROOT, 1 << 6))
            .unwrap();
        // A new selector of root motion may be told where the pointer is.
        above.settle();
        owner.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 26, 6);
        let _ = next_motion(&mut owner);
        let _ = next_motion(&mut peer);
        {
            let mut request = vec![43, 0];
            push_u16(&mut request, above.order, 1);
            above.stream.write_all(&request).unwrap();
            let mut stray = Vec::new();
            loop {
                let record = read_x_record(&mut above.stream);
                if record[0] == 1 {
                    break;
                }
                stray.push(record);
            }
            assert!(stray.is_empty(), "a client selecting only on the root hears nothing while the window has selectors: {stray:?}");
        }
        let quiet = |client: &mut XtestClient, label: &str| {
            let mut request = vec![43, 0];
            push_u16(&mut request, client.order, 1);
            client.stream.write_all(&request).unwrap();
            let mut stray = Vec::new();
            loop {
                let record = read_x_record(&mut client.stream);
                if record[0] == 1 {
                    break;
                }
                stray.push(record);
            }
            assert!(stray.is_empty(), "{label}: {stray:?}");
        };
        owner.stream
            .write_all(&change_window_event_mask_request(owner.order, window, 3 | (1 << 2) | (1 << 3)))
            .unwrap();
        quiet(&mut owner, "owner after dropping motion");
        peer.stream
            .write_all(&change_window_event_mask_request(peer.order, window, 3 | (1 << 3)))
            .unwrap();
        quiet(&mut peer, "peer after dropping motion");
        owner.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 27, 7);
        let motion = next_motion(&mut above);
        assert_eq!(event_window(&motion), X_SETUP_DEFAULT_ROOT, "with nobody selecting on the window, motion propagates to the root's selector");
        assert_eq!((at(&motion, 20), at(&motion, 22)), (27, 7), "root coordinates");
        quiet(&mut owner, "owner, motion no longer selected");
        quiet(&mut peer, "peer, motion no longer selected");
    }

    /// A press nobody selected on the event window or above it is no event
    /// at all (XTS Xlib11 ButtonPress 4), and a client that did not select
    /// it hears nothing when another client did (ButtonPress 6). The press
    /// used to be written to the surface's owner regardless: the implicit
    /// grab a press activates was taken for the owner's own grab, whose mask
    /// admits everything. Red on the tree before the fix: the owner reads a
    /// ButtonPress ahead of its barrier's reply.
    #[test]
    fn a_press_the_owner_did_not_select_is_not_written_to_it() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut owner = fixture.connect();
        let mut peer = fixture.connect();
        let window = owner.next;
        owner.next += 2;
        // Keys, ButtonRelease, motion of every kind and FocusChange: what a
        // client may select on a window but ButtonPress, less the crossing
        // masks, whose EnterNotify the motion below would have to read past.
        let all_but_press = 3 | (1 << 3) | (1 << 6) | (1 << 8) | (1 << 13) | (1 << 21);
        owner.stream
            .write_all(&create_window_request(owner.order, window, 20, 0, 16, 16))
            .unwrap();
        owner.stream
            .write_all(&change_window_event_mask_request(owner.order, window, all_but_press))
            .unwrap();
        owner.stream.write_all(&map_window_request(owner.order, window)).unwrap();
        owner.settle();
        let event_window = |event: &[u8; 32]| u32::from_le_bytes([event[12], event[13], event[14], event[15]]);

        owner.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 25, 5);
        let motion = owner.next_event(6);
        assert_eq!(event_window(&motion), window, "motion was selected");
        owner.fake_input(4, 1);
        owner.assert_quiet("a press nobody selected is discarded");
        owner.fake_input(5, 1);
        let release = owner.next_event(5);
        assert_eq!(event_window(&release), window, "the release was selected");

        // A peer selecting the press hears it; the owner still does not.
        peer.stream
            .write_all(&change_window_event_mask_request(peer.order, window, 1 << 2))
            .unwrap();
        peer.barrier();
        owner.fake_input(4, 1);
        let press = peer.next_event(4);
        assert_eq!(event_window(&press), window, "the peer's press");
        owner.assert_quiet("the owner did not select the press");
        owner.fake_input(5, 1);
        let release = owner.next_event(5);
        assert_eq!(event_window(&release), window, "the owner's release");
        peer.assert_quiet("the peer did not select the release");
    }

    /// A key is selected by direction: a client that selected KeyRelease and
    /// not KeyPress is written the release and not the press (XTS Xlib11
    /// KeyPress 3). The delivery rule checked one combined mask, so a press
    /// reached every client that had selected either. Red on the tree before
    /// the fix: the owner reads a KeyPress ahead of its barrier's reply.
    #[test]
    fn a_key_press_reaches_only_the_clients_that_selected_presses() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut owner = fixture.connect();
        let mut peer = fixture.connect();
        let window = fixture.focused_window(&mut owner);
        // KeyRelease, ButtonPress, ButtonRelease and the fixture's FocusChange.
        owner.stream
            .write_all(&change_window_event_mask_request(owner.order, window, (1 << 1) | (1 << 2) | (1 << 3) | (1 << 21)))
            .unwrap();
        owner.settle();
        peer.stream
            .write_all(&change_window_event_mask_request(peer.order, window, 1 << 0))
            .unwrap();
        peer.barrier();

        owner.fake_input(2, 38);
        let press = peer.next_event(2);
        assert_eq!(u32::from_le_bytes([press[12], press[13], press[14], press[15]]), window, "the peer's press");
        assert_eq!(press[1], 38);
        owner.assert_quiet("the owner selected releases, not presses");
        owner.fake_input(3, 38);
        let release = owner.next_event(3);
        assert_eq!(release[1], 38, "the owner's release");
        peer.assert_quiet("the peer selected presses, not releases");
    }

    /// A KeymapNotify follows every EnterNotify and FocusIn for a client
    /// that selected KeymapState on the window entered or focused, carrying
    /// the keys down (XTS Xlib11 KeymapNotify 1 and 2). None was written,
    /// and the suite's KeymapNotify 1 binary then crashed on its own
    /// "Missing %s event" report once no stray motion followed the
    /// EnterNotify. Red on the tree before the fix: the record after the
    /// EnterNotify is not a KeymapNotify.
    ///
    /// The pointer moves between two windows of the one client: a motion
    /// onto the root reaches no client's writer, so the return from it
    /// crosses nothing this layer can see (t211).
    #[test]
    fn a_keymap_notify_follows_an_enter_notify_and_a_focus_in() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let first = client.next;
        let second = client.next + 2;
        client.next += 4;
        // KeyPress, KeyRelease, EnterWindow, KeymapState and FocusChange on
        // both windows.
        for (window, x) in [(first, 20), (second, 40)] {
            client.stream
                .write_all(&create_window_request(client.order, window, x, 0, 16, 16))
                .unwrap();
            client.stream
                .write_all(&change_window_event_mask_request(client.order, window, 3 | (1 << 4) | (1 << 14) | (1 << 21)))
                .unwrap();
            client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        }
        client.settle();
        // A read that starves is a failure here, not a hang.
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let event_window = |event: &[u8; 32]| u32::from_le_bytes([event[12], event[13], event[14], event[15]]);
        let event_window_of_focus = |event: &[u8; 32]| u32::from_le_bytes([event[4], event[5], event[6], event[7]]);

        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 25, 5);
        let entered = client.next_event(7);
        assert_eq!(event_window(&entered), first, "EnterNotify on the first window");
        let keymap = read_x_record(&mut client.stream);
        assert_eq!(keymap[0], 11, "a KeymapNotify follows the EnterNotify: {keymap:?}");
        assert_eq!(keymap[4] & 0x40, 0, "no key is down yet");

        // Keycode 38 held: bit 38 of the bitmap is byte 4, bit 6, and the
        // KeymapNotify carries bytes 1 to 31 of the bitmap at 1 to 31.
        client.fake_input(2, 38);
        let _ = client.next_event(2);
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 45, 5);
        let entered = client.next_event(7);
        assert_eq!(event_window(&entered), second, "EnterNotify on the second window");
        let keymap = read_x_record(&mut client.stream);
        assert_eq!(keymap[0], 11, "a KeymapNotify follows the second EnterNotify: {keymap:?}");
        assert_eq!(keymap[4] & 0x40, 0x40, "keycode 38 is down in the bitmap: {keymap:?}");

        // FocusIn on the first window, then its KeymapNotify. The focus was
        // at the root with the pointer in the second window, so a FocusOut
        // with detail Pointer on the second window comes first.
        let mut request = vec![42, 0];
        push_u16(&mut request, client.order, 3);
        push_u32(&mut request, client.order, first);
        push_u32(&mut request, client.order, 0);
        client.stream.write_all(&request).unwrap();
        let focus = loop {
            let record = read_x_record(&mut client.stream);
            match record[0] & 0x7f {
                9 => break record,
                10 => assert_eq!((event_window_of_focus(&record), record[1]), (second, 5), "the pointer window's FocusOut"),
                other => panic!("expected a focus event, got record {other}: {record:?}"),
            }
        };
        assert_eq!(event_window_of_focus(&focus), first, "FocusIn on the first window");
        let keymap = read_x_record(&mut client.stream);
        assert_eq!(keymap[0], 11, "a KeymapNotify follows the FocusIn: {keymap:?}");
        assert_eq!(keymap[4] & 0x40, 0x40, "keycode 38 is still down: {keymap:?}");
        client.fake_input(3, 38);
        let _ = client.next_event(3);
        client.barrier();
    }

    /// What an injection owes the injecting client reaches its socket before
    /// the reply to its next request. FakeInput's barrier ended at routing,
    /// with the event still in the writer's queue, so a client that injected
    /// a motion and then asked anything saw the reply first and, reading
    /// what was pending after it, nothing (t229; XTS Xlib11 KeymapNotify 1
    /// on a loaded machine). Red on the tree before the fix, on a fraction
    /// of the rounds; the fraction is what a race gives, so the test rounds.
    #[test]
    fn an_injections_events_precede_the_reply_to_the_next_request() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let window = client.next;
        client.next += 2;
        // PointerMotion and EnterWindow.
        client.stream
            .write_all(&create_window_request(client.order, window, 20, 0, 16, 16))
            .unwrap();
        client.stream
            .write_all(&change_window_event_mask_request(client.order, window, (1 << 4) | (1 << 6)))
            .unwrap();
        client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        client.settle();
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

        let mut replies_first = Vec::new();
        for round in 0..40 {
            let x = 25 + (round % 2) as i16;
            client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, x, 5);
            let mut request = vec![43, 0];
            push_u16(&mut request, client.order, 1);
            client.stream.write_all(&request).unwrap();
            // Everything up to the reply, then whatever the motion still
            // owes if it came after the reply.
            let mut before = Vec::new();
            loop {
                let record = read_x_record(&mut client.stream);
                if record[0] == 1 {
                    break;
                }
                before.push(record[0] & 0x7f);
            }
            if !before.contains(&6) {
                replies_first.push(round);
                let _ = client.next_event(6);
            }
        }
        assert!(
            replies_first.is_empty(),
            "the reply overtook the injected motion in rounds {replies_first:?}"
        );
    }

    /// A core event's `child` names the child of the event window on the
    /// way to the source: the source itself when it is a child, the ancestor
    /// of the source that is a child of the event window when it is deeper,
    /// and None when the source is the event window (XTS Xlib11 ButtonPress
    /// 8 to 10, KeyPress 5 to 7, MotionNotify 15 and 16). It was always
    /// None. Red before the fix: the press in the grandchild names no child.
    #[test]
    fn a_core_events_child_is_the_event_windows_child_toward_the_source() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let toplevel = client.next;
        let child = client.next + 2;
        let grandchild = client.next + 4;
        client.next += 6;
        // Keys, ButtonPress and ButtonRelease on the toplevel only.
        client.stream
            .write_all(&create_window_request(client.order, toplevel, 20, 0, 16, 16))
            .unwrap();
        client.stream
            .write_all(&change_window_event_mask_request(client.order, toplevel, 3 | (1 << 2) | (1 << 3)))
            .unwrap();
        client.stream
            .write_all(&create_window_request_with_parent(client.order, child, toplevel, 4, 4, 8, 8))
            .unwrap();
        client.stream
            .write_all(&create_window_request_with_parent(client.order, grandchild, child, 2, 2, 4, 4))
            .unwrap();
        for window in [toplevel, child, grandchild] {
            client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        }
        client.settle();
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let field = |event: &[u8; 32], at: usize| u32::from_le_bytes([event[at], event[at + 1], event[at + 2], event[at + 3]]);

        // In the grandchild (root 26..30, 6..10): the event window is the
        // toplevel and its child toward the source is the child.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 27, 7);
        client.fake_input(4, 1);
        let press = client.next_event(4);
        assert_eq!((field(&press, 12), field(&press, 16)), (toplevel, child), "press in the grandchild");
        client.fake_input(5, 1);
        let _ = client.next_event(5);
        client.fake_input(2, 38);
        let key = client.next_event(2);
        assert_eq!((field(&key, 12), field(&key, 16)), (toplevel, child), "key with the pointer in the grandchild");
        client.fake_input(3, 38);
        let _ = client.next_event(3);

        // In the child (root 24..32, 4..12) outside the grandchild: the child
        // is the source and the subwindow.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 25, 5);
        client.fake_input(4, 1);
        let press = client.next_event(4);
        assert_eq!((field(&press, 12), field(&press, 16)), (toplevel, child), "press in the child");
        client.fake_input(5, 1);
        let _ = client.next_event(5);

        // In the toplevel itself: no subwindow.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 21, 1);
        client.fake_input(4, 1);
        let press = client.next_event(4);
        assert_eq!((field(&press, 12), field(&press, 16)), (toplevel, 0), "press in the toplevel");
        client.fake_input(5, 1);
        let _ = client.next_event(5);
        client.barrier();
    }

    /// A press nobody selected on the source propagates up to the first
    /// window where the client selected it, the root included, and stops at
    /// a do-not-propagate mask or at the first selector (XTS Xlib11
    /// ButtonPress 7, ButtonRelease 4). The owner's walk stopped at its own
    /// toplevel, so a client that selected on the root heard nothing from
    /// its own windows. Red before the fix: the first press reaches nobody.
    #[test]
    fn a_press_propagates_to_the_root_and_stops_at_do_not_propagate() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let toplevel = client.next;
        let child = client.next + 2;
        let grandchild = client.next + 4;
        client.next += 6;
        client.stream
            .write_all(&create_window_request(client.order, toplevel, 20, 0, 16, 16))
            .unwrap();
        client.stream
            .write_all(&create_window_request_with_parent(client.order, child, toplevel, 2, 2, 12, 12))
            .unwrap();
        client.stream
            .write_all(&create_window_request_with_parent(client.order, grandchild, child, 2, 2, 8, 8))
            .unwrap();
        for window in [toplevel, child, grandchild] {
            client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        }
        // ButtonPress on the root only.
        client.stream
            .write_all(&change_window_event_mask_request(client.order, X_SETUP_DEFAULT_ROOT, 1 << 2))
            .unwrap();
        client.settle();
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let field = |event: &[u8; 32], at: usize| u32::from_le_bytes([event[at], event[at + 1], event[at + 2], event[at + 3]]);
        let at = |event: &[u8; 32], offset: usize| i16::from_le_bytes([event[offset], event[offset + 1]]);
        let dnp = |client: &mut XtestClient, window: u32, mask: u32| {
            let mut out = vec![2, 0];
            push_u16(&mut out, client.order, 4);
            push_u32(&mut out, client.order, window);
            push_u32(&mut out, client.order, 1 << 12);
            push_u32(&mut out, client.order, mask);
            client.stream.write_all(&out).unwrap();
        };

        // In the grandchild (root 24..32, 4..12): the press climbs to the root.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 27, 7);
        client.fake_input(4, 1);
        let press = client.next_event(4);
        assert_eq!((field(&press, 12), field(&press, 16)), (X_SETUP_DEFAULT_ROOT, toplevel), "the press on the root names the toplevel as child");
        assert_eq!((at(&press, 20), at(&press, 22), at(&press, 24), at(&press, 26)), (27, 7, 27, 7), "root coordinates on the root");
        client.fake_input(5, 1);
        client.assert_quiet("nobody selected the release");

        // The root stops selecting, the toplevel selects, and the child
        // refuses to propagate presses: the press reaches nobody.
        client.stream
            .write_all(&change_window_event_mask_request(client.order, X_SETUP_DEFAULT_ROOT, 0))
            .unwrap();
        client.stream
            .write_all(&change_window_event_mask_request(client.order, toplevel, 1 << 2))
            .unwrap();
        dnp(&mut client, child, 1 << 2);
        client.barrier();
        client.fake_input(4, 1);
        client.assert_quiet("the child's do-not-propagate mask stops the press");
        client.fake_input(5, 1);

        // The child selects and the toplevel refuses to propagate: the press
        // stops at the child, the first selector, and the toplevel hears
        // nothing.
        client.stream
            .write_all(&change_window_event_mask_request(client.order, child, 1 << 2))
            .unwrap();
        dnp(&mut client, toplevel, 1 << 2);
        dnp(&mut client, child, 0);
        client.barrier();
        client.fake_input(4, 1);
        let press = client.next_event(4);
        assert_eq!((field(&press, 12), field(&press, 16)), (child, grandchild), "the press on the child names the grandchild");
        client.fake_input(5, 1);
        client.assert_quiet("the toplevel heard nothing");
    }

    /// An injected wheel button is a button to the core protocol: its press
    /// and release are ButtonPress and ButtonRelease with its detail, and
    /// motion while it is held carries Button4Mask and answers to
    /// Button4Motion (XTS Xlib11 MotionNotify 6 and 7). XTEST dropped it
    /// as a wheel step it could not carry. Red before the fix: no press
    /// arrives.
    #[test]
    fn an_injected_wheel_button_is_held_like_any_button() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let window = client.next;
        client.next += 2;
        // ButtonPress, ButtonRelease and Button4Motion.
        client.stream
            .write_all(&create_window_request(client.order, window, 20, 0, 16, 16))
            .unwrap();
        client.stream
            .write_all(&change_window_event_mask_request(client.order, window, (1 << 2) | (1 << 3) | (1 << 11)))
            .unwrap();
        client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        client.settle();
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let state = |event: &[u8; 32]| u16::from_le_bytes([event[28], event[29]]);

        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 25, 5);
        client.fake_input(4, 4);
        let press = client.next_event(4);
        assert_eq!((press[1], state(&press)), (4, 0), "button 4 pressed with nothing held");
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 26, 6);
        let motion = client.next_event(6);
        assert_eq!(state(&motion), 1 << 11, "motion with button 4 held carries Button4Mask");
        client.fake_input(5, 4);
        let release = client.next_event(5);
        assert_eq!((release[1], state(&release)), (4, 1 << 11), "button 4 released, still in the state");
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 27, 7);
        client.assert_quiet("plain motion is not selected");
    }

    /// A pointer move generates the protocol's crossings: leaves from the
    /// window left up to the common ancestor, then enters down to the
    /// window entered, each with its detail (Ancestor, Virtual, Inferior,
    /// Nonlinear, NonlinearVirtual), its child toward the pointer and
    /// coordinates in its own window; a move out of every window of the
    /// client is a move to the root (XTS Xlib11 EnterNotify 3, 4, 7 to 9,
    /// 12, 13; LeaveNotify 4, 5, 8 to 10, 14, 15). The crossing was one
    /// EnterNotify of detail Nonlinear on the window a motion was reported
    /// on, and nothing when the pointer left. Red before the fix: the first
    /// move into the grandchild produces one record where four are owed.
    #[test]
    fn a_pointer_move_generates_the_protocols_crossings() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let toplevel = client.next;
        let child = client.next + 2;
        let grandchild = client.next + 4;
        let other = client.next + 6;
        client.next += 8;
        let root = X_SETUP_DEFAULT_ROOT;
        // EnterWindow and LeaveWindow everywhere, the root included.
        client.stream
            .write_all(&create_window_request(client.order, toplevel, 20, 0, 16, 16))
            .unwrap();
        client.stream
            .write_all(&create_window_request_with_parent(client.order, child, toplevel, 4, 4, 8, 8))
            .unwrap();
        client.stream
            .write_all(&create_window_request_with_parent(client.order, grandchild, child, 2, 2, 4, 4))
            .unwrap();
        client.stream
            .write_all(&create_window_request(client.order, other, 40, 0, 16, 16))
            .unwrap();
        for window in [toplevel, child, grandchild, other, root] {
            client.stream
                .write_all(&change_window_event_mask_request(client.order, window, (1 << 4) | (1 << 5)))
                .unwrap();
        }
        for window in [toplevel, child, grandchild, other] {
            client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        }
        client.settle();
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let field = |event: &[u8; 32], at: usize| u32::from_le_bytes([event[at], event[at + 1], event[at + 2], event[at + 3]]);
        let at = |event: &[u8; 32], offset: usize| i16::from_le_bytes([event[offset], event[offset + 1]]);
        // (type, detail, window, child) of every crossing up to the barrier.
        let crossings = |client: &mut XtestClient| {
            let mut request = vec![43, 0];
            push_u16(&mut request, client.order, 1);
            client.stream.write_all(&request).unwrap();
            let mut seen = Vec::new();
            loop {
                let record = read_x_record(&mut client.stream);
                match record[0] & 0x7f {
                    1 => break seen,
                    7 | 8 => seen.push((record[0] & 0x7f, record[1], field(&record, 12), field(&record, 16), at(&record, 24), at(&record, 26))),
                    6 => {}
                    other => panic!("unexpected record {other}: {record:?}"),
                }
            }
        };
        const ANCESTOR: u8 = 0;
        const VIRTUAL: u8 = 1;
        const INFERIOR: u8 = 2;
        const NONLINEAR: u8 = 3;
        const NONLINEAR_VIRTUAL: u8 = 4;

        // From the root into the grandchild (root 26..30, 6..10): the root
        // is left toward the toplevel, the windows between are entered
        // virtually, the grandchild as Ancestor; coordinates in each window.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 27, 7);
        assert_eq!(
            crossings(&mut client),
            vec![
                (8, INFERIOR, root, toplevel, 27, 7),
                (7, VIRTUAL, toplevel, child, 7, 7),
                (7, VIRTUAL, child, grandchild, 3, 3),
                (7, ANCESTOR, grandchild, 0, 1, 1),
            ],
            "root to grandchild"
        );
        // Up to the toplevel: the grandchild is left as Ancestor, the child
        // virtually, the toplevel entered as Inferior toward the child.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 21, 1);
        assert_eq!(
            crossings(&mut client),
            vec![
                (8, ANCESTOR, grandchild, 0, -5, -5),
                (8, VIRTUAL, child, grandchild, -3, -3),
                (7, INFERIOR, toplevel, child, 1, 1),
            ],
            "grandchild to toplevel"
        );
        // Out to the root: no window of the client is under the pointer.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 5, 5);
        assert_eq!(
            crossings(&mut client),
            vec![(8, ANCESTOR, toplevel, 0, -15, 5), (7, INFERIOR, root, toplevel, 5, 5)],
            "toplevel to root"
        );
        // Into the other toplevel, then across to the grandchild: siblings
        // under the root, so the root is the common ancestor and hears
        // nothing, and the windows between are crossed as NonlinearVirtual.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 45, 5);
        assert_eq!(
            crossings(&mut client),
            vec![(8, INFERIOR, root, other, 45, 5), (7, ANCESTOR, other, 0, 5, 5)],
            "root to the other toplevel"
        );
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 27, 7);
        assert_eq!(
            crossings(&mut client),
            vec![
                (8, NONLINEAR, other, 0, -13, 7),
                (7, NONLINEAR_VIRTUAL, toplevel, child, 7, 7),
                (7, NONLINEAR_VIRTUAL, child, grandchild, 3, 3),
                (7, NONLINEAR, grandchild, 0, 1, 1),
            ],
            "the other toplevel to the grandchild"
        );
    }

    fn warp_pointer_request(
        order: XByteOrder,
        source: u32,
        destination: u32,
        src_x: i16,
        src_y: i16,
        src_width: u16,
        src_height: u16,
        dst_x: i16,
        dst_y: i16,
    ) -> Vec<u8> {
        let mut out = vec![41, 0];
        push_u16(&mut out, order, 6);
        push_u32(&mut out, order, source);
        push_u32(&mut out, order, destination);
        push_u16(&mut out, order, src_x as u16);
        push_u16(&mut out, order, src_y as u16);
        push_u16(&mut out, order, src_width);
        push_u16(&mut out, order, src_height);
        push_u16(&mut out, order, dst_x as u16);
        push_u16(&mut out, order, dst_y as u16);
        out
    }
}
