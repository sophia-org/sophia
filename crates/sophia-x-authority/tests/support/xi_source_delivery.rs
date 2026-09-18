#[test]
fn xi_source_and_master_selection_reach_the_wire_without_duplicate_masters() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        for xi_ancestor in [false, true] {
            for (kind, event_type, core_mask) in [
                (
                    XAuthorityPointerEventKind::Axis {
                        button: 4,
                        pressed: true,
                        horizontal_position_v120: None,
                        vertical_position_v120: Some(-120),
                    },
                    6,
                    1 << 2,
                ),
                (
                    XAuthorityPointerEventKind::Button {
                        button: 1,
                        pressed: true,
                    },
                    4,
                    1 << 2,
                ),
                (
                    XAuthorityPointerEventKind::Button {
                        button: 1,
                        pressed: false,
                    },
                    5,
                    1 << 3,
                ),
            ] {
                for (selected_device, mut expected_devices) in [
                    (0, vec![128, 2]),
                    (1, vec![2]),
                    (2, vec![2]),
                    (128, vec![128]),
                ] {
                    let namespace = NamespaceId::from_raw(1);
                    let client = XServerFrontendClientId::from_raw(1);
                    let surface = SurfaceId::new(1, 1);
                    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
                    let window = XResourceId::new(0x200001, 1);
                    let mut authority = crate::XInputAuthorityState::default();
                    authority.select_xi_events(
                        namespace,
                        client.raw(),
                        if xi_ancestor { root } else { window },
                        &[(selected_device, vec![1 << event_type])],
                    );
                    let mut selections = XCoreEventSelectionState::default();
                    selections.register(
                        window,
                        root,
                        Rect {
                            x: 10,
                            y: 20,
                            width: 400,
                            height: 300,
                        },
                    );
                    selections.update(window, Some(core_mask), None);
                    if xi_ancestor {
                        expected_devices.retain(|device| *device != 2);
                    }
                    let (socket, mut peer) = UnixStream::pair().unwrap();
                    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                    let (sender, receiver) = channel();
                    let writer = spawn_x11_input_event_writer(
                        X11InputWriterState {
                            stream: Arc::new(Mutex::new(socket)),
                            output_control_pending: Arc::new(AtomicUsize::new(0)),
                            output_wire: Arc::new(X11WirePermission::open()),
                            byte_order: order,
                            sequence: Arc::new(AtomicU16::new(1)),
                            focused_surface_window: Arc::new(AtomicU64::new(window.local.raw())),
                            core_event_selections: Arc::new(Mutex::new(selections)),
                            xkb_state_details: Arc::new(AtomicU16::new(0)),
                            xkb_modifiers: Arc::new(AtomicU16::new(0)),
                            surface_windows: Arc::new(Mutex::new(BTreeMap::from([(
                                surface, window,
                            )]))),
                            input_authority: Some(Arc::new(Mutex::new(authority))),
                            standalone_query_authority: None,
                            namespace,
                            client,
                        },
                        X11InputEventReceiver::Plain(receiver),
                    )
                    .unwrap();
                    sender
                        .send(XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                            kind,
                            surface,
                            root_x: 30,
                            root_y: 40,
                            event_x: 20,
                            event_y: 20,
                            state: 0,
                            time_msec: 1,
                        }))
                        .unwrap();
                    drop(sender);
                    writer.thread.join().unwrap().unwrap();
                    let mut bytes = Vec::new();
                    peer.read_to_end(&mut bytes).unwrap();
                    let read_u16 = |bytes: &[u8]| match order {
                        XByteOrder::LittleEndian => u16::from_le_bytes(bytes.try_into().unwrap()),
                        XByteOrder::BigEndian => u16::from_be_bytes(bytes.try_into().unwrap()),
                    };
                    let read_u32 = |bytes: &[u8]| match order {
                        XByteOrder::LittleEndian => u32::from_le_bytes(bytes.try_into().unwrap()),
                        XByteOrder::BigEndian => u32::from_be_bytes(bytes.try_into().unwrap()),
                    };
                    let mut devices = Vec::new();
                    let mut core_packets = 0;
                    let mut offset = 0;
                    while offset < bytes.len() {
                        let packet = &bytes[offset..];
                        let len = if packet[0] == 35 {
                            assert_eq!(read_u16(&packet[8..10]), event_type);
                            devices.push(read_u16(&packet[10..12]));
                            assert_eq!(read_u16(&packet[52..54]), 128);
                            assert_eq!(
                                read_u32(&packet[24..28]),
                                if xi_ancestor {
                                    root.local.raw() as u32
                                } else {
                                    0x200001
                                }
                            );
                            if event_type == 6 {
                                assert_eq!(decode_xi_fp3232(order, &packet[84..92]), (-120, 0));
                            }
                            32 + read_u32(&packet[4..8]) as usize * 4
                        } else {
                            core_packets += 1;
                            32
                        };
                        offset += len;
                    }
                    assert_eq!(
                        core_packets,
                        usize::from(!expected_devices.contains(&2)),
                        "core and master XI are alternatives"
                    );
                    assert_eq!(
                        devices, expected_devices,
                        "selection={selected_device} order={order:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn xi_all_master_masks_do_not_subscribe_a_source_or_another_namespace() {
    let mut authority = crate::XInputAuthorityState::default();
    let namespace = NamespaceId::from_raw(1);
    let window = XResourceId::new(0x200001, 1);
    authority.select_xi_events(namespace, 1, window, &[(1, vec![1 << 6])]);
    assert!(authority.xi_event_selected(namespace, 1, window, 2, 6));
    assert!(!authority.xi_event_selected(namespace, 1, window, 128, 6));
    authority.select_xi_events(namespace, 1, window, &[(0, vec![1 << 6])]);
    assert!(authority.xi_event_selected(namespace, 1, window, 128, 6));
    assert!(!authority.xi_event_selected(NamespaceId::from_raw(2), 1, window, 128, 6));
    assert!(!authority.xi_event_selected(namespace, 2, window, 128, 6));
}
