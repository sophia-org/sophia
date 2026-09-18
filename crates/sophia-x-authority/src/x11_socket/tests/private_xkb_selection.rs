mod private_xkb_selection_tests {
    use super::*;

    fn word(bytes: &mut [u8], offset: usize, value: u16, big: bool) {
        bytes[offset..offset + 2].copy_from_slice(&if big {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        });
    }

    fn barrier(socket: &mut UnixStream, big: bool) {
        let mut request = [43, 0, 0, 0];
        word(&mut request, 2, 1, big);
        std::io::Write::write_all(socket, &request).unwrap();
        let mut reply = [0; 32];
        std::io::Read::read_exact(socket, &mut reply).unwrap();
        assert_eq!(
            reply[0], 1,
            "the exact preceding request must have succeeded"
        );
    }

    #[test]
    fn actual_xkb_select_events_publishes_guarded_details_in_both_byte_orders() {
        for big in [false, true] {
            let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
            let private = private_for_roles(&service_keeper);
            let registry = private.broker.registry.clone();
            let state = Arc::new(X11CoreSocketServerState::new());
            state
                .runtime
                .lock()
                .unwrap()
                .set_input_authority(registry.input_authority.clone());
            let context = admitted(XServerFrontendClientId(7810));
            let (mut client, mut server) = UnixStream::pair().unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let server_state = state.clone();
            let worker = std::thread::spawn(move || {
                serve_x11_core_socket_client_with_trace_observer_and_input(
                    &mut server,
                    context.namespace.id,
                    &server_state,
                    X11ClientConnectionInputs {
                        input_receiver: None,
                        control_channels: None,
                        client_routing: Some(registry),
                    },
                    X11ClientAdmissionContext {
                        authorization: &XServerFrontendSetupAuthorization::default(),
                        admission_policy: Some(Arc::new(LifecycleSetupPolicy(context))),
                        worker_admission: None,
                    },
                    |_| Ok(None),
                )
            });
            let mut setup = [0; 12];
            setup[0] = if big { b'B' } else { b'l' };
            word(&mut setup, 2, 11, big);
            std::io::Write::write_all(&mut client, &setup).unwrap();
            let mut prefix = [0; 8];
            std::io::Read::read_exact(&mut client, &mut prefix).unwrap();
            assert_eq!(prefix[0], 1);
            let count = if big {
                u16::from_be_bytes([prefix[6], prefix[7]])
            } else {
                u16::from_le_bytes([prefix[6], prefix[7]])
            };
            let mut body = vec![0; usize::from(count) * 4];
            std::io::Read::read_exact(&mut client, &mut body).unwrap();
            barrier(&mut client, big);
            let selections = {
                let clients = private.broker.registry.clients.lock().unwrap();
                assert_eq!(clients.len(), 1);
                clients
                    .values()
                    .next()
                    .unwrap()
                    .connection_state
                    .get()
                    .unwrap()
                    .selections
                    .clone()
            };
            let initial_revision = selections.lock().unwrap().applied_revision.unwrap();
            let requests = [
                (4, 0, 0, Some((1, 1)), 1),
                (4, 0, 0, Some((2, 2)), 3),
                (0, 4, 0, None, 0),
                (0, 0, 4, None, u16::MAX),
            ];
            for (index, (affect, clear, all, detail, expected)) in requests.into_iter().enumerate()
            {
                let mut packet = vec![0; if detail.is_some() { 20 } else { 16 }];
                packet[0] = crate::X_KEYBOARD_MAJOR_OPCODE;
                packet[1] = crate::X_KEYBOARD_SELECT_EVENTS_MINOR_OPCODE;
                let units = (packet.len() / 4) as u16;
                word(&mut packet, 2, units, big);
                word(&mut packet, 4, 3, big);
                word(&mut packet, 6, affect, big);
                word(&mut packet, 8, clear, big);
                word(&mut packet, 10, all, big);
                if let Some((affect, detail)) = detail {
                    word(&mut packet, 16, affect, big);
                    word(&mut packet, 18, detail, big);
                }
                std::io::Write::write_all(&mut client, &packet).unwrap();
                barrier(&mut client, big);
                let selected = selections.lock().unwrap();
                assert_eq!(selected.xkb_state_details, expected);
                assert_eq!(
                    selected.applied_revision,
                    Some(initial_revision + index as u64 + 1)
                );
            }
            client.shutdown(std::net::Shutdown::Both).unwrap();
            worker.join().unwrap().unwrap();
            lifecycle_drain(&private.terminal.lifecycle);
        }
    }

    #[test]
    fn xkb_selection_updates_preserve_the_ordinary_projection_and_unknown_revision() {
        let mut selections = XCoreEventSelectionState::default();
        let ordinary = AtomicU16::new(0);
        selections.select_xkb_state_notifications(&ordinary, 4, 0, 0, Some((1, 1)));
        assert_eq!(ordinary.load(Ordering::Acquire), 1);
        selections.select_xkb_state_notifications(&ordinary, 4, 0, 0, Some((3, 2)));
        assert_eq!(ordinary.load(Ordering::Acquire), 2);
        assert_eq!(selections.xkb_state_details, 2);
        // Staged interruption: a following update must not make an unreadable
        // applied selection valid merely because the ordinary atomic updated.
        selections.begin_applied_mutation();
        selections.select_xkb_state_notifications(&ordinary, 0, 0, 4, None);
        assert_eq!(ordinary.load(Ordering::Acquire), u16::MAX);
        assert_eq!(selections.applied_revision, None);
    }
}
