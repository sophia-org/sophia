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

        fn fake_input(&mut self, event_type: u8, detail: u8) {
            let mut body = [0u8; 32];
            body[0] = event_type;
            body[1] = detail;
            self.stream
                .write_all(&xtest_request(self.order, X_TEST_FAKE_INPUT_MINOR_OPCODE, &body))
                .unwrap();
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
            let namespaces = [901, 902].map(|id| {
                NamespaceContext::new(
                    NamespaceId::from_raw(id),
                    NamespaceProfile::Confined,
                    NamespaceCapabilities::NONE,
                )
                .unwrap()
            });
            let config = XServerFrontendConfig::new(&path, NamespaceId::from_raw(901))
                .unwrap()
                .with_max_concurrent_clients(NonZeroUsize::new(4).unwrap())
                .with_admission_policy(Arc::new(SequencedXAdmissionPolicy {
                    namespaces,
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
}
