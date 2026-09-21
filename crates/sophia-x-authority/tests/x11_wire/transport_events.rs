
#[cfg(unix)]
#[test]
fn x11_request_reader_receives_bounded_scm_rights_with_the_request_header() {
    use std::os::unix::net::UnixStream;
    use std::fs::File;
    use std::io::IoSlice;
    use std::mem::MaybeUninit;
    use std::os::fd::AsFd;

    let (sender, mut receiver) = UnixStream::pair().unwrap();
    let request =
        extension_query_version_request(XByteOrder::LittleEndian, X_DRI3_MAJOR_OPCODE, 1, 2);
    let file = File::open("/dev/null").unwrap();
    let borrowed = [file.as_fd()];
    let mut space = [MaybeUninit::uninit();
        rustix::cmsg_space!(ScmRights(sophia_protocol::DMA_BUF_MAX_PLANES))];
    let mut ancillary = rustix::net::SendAncillaryBuffer::new(&mut space);
    assert!(ancillary.push(rustix::net::SendAncillaryMessage::ScmRights(&borrowed)));
    let sent = rustix::net::sendmsg(
        sender,
        &[IoSlice::new(&request)],
        &mut ancillary,
        rustix::net::SendFlags::empty(),
    )
    .unwrap();
    assert_eq!(sent, request.len());

    let received = read_x11_core_request(&mut receiver, XByteOrder::LittleEndian)
        .unwrap()
        .unwrap();
    assert_eq!(received.major_opcode, X_DRI3_MAJOR_OPCODE);
    assert_eq!(received.bytes, request);
    assert_eq!(received.fds.len(), 1);
}

#[cfg(unix)]
#[test]
fn x11_output_record_sends_bounded_scm_rights_with_the_first_bytes() {
    use std::os::unix::net::UnixStream;
    use std::fs::File;
    use std::io::IoSliceMut;
    use std::mem::MaybeUninit;
    use std::os::fd::OwnedFd;

    for fd_count in [1, sophia_protocol::DMA_BUF_MAX_PLANES] {
        let (mut sender, receiver) = UnixStream::pair().unwrap();
        let payload = vec![0x5a; X_CLIENT_OUTPUT_RECORD_LEN];
        let fds = (0..fd_count)
            .map(|_| OwnedFd::from(File::open("/dev/null").unwrap()))
            .collect();
        let record = X11SocketOutputRecord::new(payload.clone(), fds).unwrap();
        assert_eq!(record.bytes(), payload);
        assert_eq!(record.fd_count(), fd_count);

        write_x11_socket_output_record(&mut sender, record).unwrap();

        let mut bytes = [0; X_CLIENT_OUTPUT_RECORD_LEN];
        let mut iov = [IoSliceMut::new(&mut bytes)];
        let mut ancillary_space = [MaybeUninit::uninit();
            rustix::cmsg_space!(ScmRights(sophia_protocol::DMA_BUF_MAX_PLANES))];
        let mut ancillary = rustix::net::RecvAncillaryBuffer::new(&mut ancillary_space);
        let received = rustix::net::recvmsg(
            receiver,
            &mut iov,
            &mut ancillary,
            rustix::net::RecvFlags::CMSG_CLOEXEC,
        )
        .unwrap();
        assert_eq!(received.bytes, payload.len());
        assert_eq!(bytes, payload.as_slice());
        let received_fds = ancillary
            .drain()
            .flat_map(|message| match message {
                rustix::net::RecvAncillaryMessage::ScmRights(fds) => fds.collect::<Vec<_>>(),
                _ => Vec::new(),
            })
            .collect::<Vec<_>>();
        assert_eq!(received_fds.len(), fd_count);
        for fd in received_fds {
            File::from(fd).metadata().unwrap();
        }
    }
}

#[cfg(unix)]
#[test]
fn x11_output_record_rejects_empty_bytes_and_excess_descriptors() {
    use std::fs::File;
    use std::os::fd::OwnedFd;

    assert!(X11SocketOutputRecord::new(Vec::new(), Vec::new()).is_err());
    let fds = (0..=sophia_protocol::DMA_BUF_MAX_PLANES)
        .map(|_| OwnedFd::from(File::open("/dev/null").unwrap()))
        .collect();
    let error = X11SocketOutputRecord::new(vec![0], fds).unwrap_err();
    assert!(error.to_string().contains("maximum is"));
}

#[cfg(unix)]
#[test]
fn x11_output_record_preserves_byte_only_output() {
    use std::os::unix::net::UnixStream;

    let (mut sender, mut receiver) = UnixStream::pair().unwrap();
    let payload = vec![0xa5; X_CLIENT_OUTPUT_RECORD_LEN];
    let record = X11SocketOutputRecord::try_from(payload.clone()).unwrap();
    write_x11_socket_output_record(&mut sender, record).unwrap();
    let mut observed = vec![0; payload.len()];
    fill_from_socket(&mut receiver, &mut observed);
    assert_eq!(observed, payload);
}

#[test]
fn mit_shm_completion_uses_the_advertised_extension_event_layout() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let event = encode_x_client_event(
            byte_order,
            XClientEvent::ShmCompletion {
                sequence: 0x1234,
                drawable: XResourceId::new(0x220701, 1),
                segment: XResourceId::new(0x440001, 1),
                offset: 128,
            },
        );
        assert_eq!(event[0], X_MIT_SHM_FIRST_EVENT);
        assert_eq!(read_u16(byte_order, &event[2..4]), 0x1234);
        assert_eq!(read_u32(byte_order, &event[4..8]), 0x220701);
        assert_eq!(
            read_u16(byte_order, &event[8..10]),
            u16::from(X_MIT_SHM_PUT_IMAGE_MINOR_OPCODE)
        );
        assert_eq!(event[10], X_MIT_SHM_MAJOR_OPCODE);
        assert_eq!(read_u32(byte_order, &event[12..16]), 0x440001);
        assert_eq!(read_u32(byte_order, &event[16..20]), 128);
    }
}

#[test]
fn present_complete_and_idle_notifications_use_xge_packed_layouts() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let configure = encode_x_client_event(
            byte_order,
            XClientEvent::PresentConfigureNotify {
                sequence: 0x1233,
                event_id: XResourceId::new(0x220900, 1),
                window: XResourceId::new(0x220901, 1),
                x: -12,
                y: 34,
                width: 960,
                height: 640,
                pixmap_width: 960,
                pixmap_height: 640,
                pixmap_flags: 0,
            },
        );
        assert_eq!(configure.len(), 40);
        assert_eq!(configure[0], 35);
        assert_eq!(configure[1], X_PRESENT_MAJOR_OPCODE);
        assert_eq!(read_u32(byte_order, &configure[4..8]), 2);
        assert_eq!(read_u16(byte_order, &configure[8..10]), 0);
        assert_eq!(read_u32(byte_order, &configure[12..16]), 0x220900);
        assert_eq!(read_u32(byte_order, &configure[16..20]), 0x220901);
        assert_eq!(read_u16(byte_order, &configure[24..26]), 960);
        assert_eq!(read_u16(byte_order, &configure[26..28]), 640);
        assert_eq!(read_u16(byte_order, &configure[32..34]), 960);
        assert_eq!(read_u16(byte_order, &configure[34..36]), 640);
        assert_eq!(read_u32(byte_order, &configure[36..40]), 0);

        let complete = encode_x_client_event(
            byte_order,
            XClientEvent::PresentCompleteNotify {
                sequence: 0x1234,
                event_id: XResourceId::new(0x220900, 1),
                window: XResourceId::new(0x220901, 1),
                serial: 77,
                ust: 123_456,
                msc: 42,
                kind: 0,
                mode: 1,
            },
        );
        assert_eq!(complete.len(), 40);
        assert_eq!(complete[0], 35);
        assert_eq!(complete[1], X_PRESENT_MAJOR_OPCODE);
        assert_eq!(read_u32(byte_order, &complete[4..8]), 2);
        assert_eq!(read_u16(byte_order, &complete[8..10]), 1);
        assert_eq!(complete[10], 0);
        assert_eq!(complete[11], 1);
        assert_eq!(read_u32(byte_order, &complete[12..16]), 0x220900);
        assert_eq!(read_u32(byte_order, &complete[16..20]), 0x220901);
        assert_eq!(read_u32(byte_order, &complete[20..24]), 77);
        assert_eq!(read_u64(byte_order, &complete[24..32]), 123_456);
        assert_eq!(read_u64(byte_order, &complete[32..40]), 42);

        let idle = encode_x_client_event(
            byte_order,
            XClientEvent::PresentIdleNotify {
                sequence: 0x1235,
                event_id: XResourceId::new(0x220900, 1),
                window: XResourceId::new(0x220901, 1),
                serial: 77,
                pixmap: XResourceId::new(0x220902, 1),
                idle_fence: Some(XResourceId::new(0x220903, 1)),
            },
        );
        assert_eq!(idle.len(), 32);
        assert_eq!(idle[0], 35);
        assert_eq!(idle[1], X_PRESENT_MAJOR_OPCODE);
        assert_eq!(read_u32(byte_order, &idle[4..8]), 0);
        assert_eq!(read_u16(byte_order, &idle[8..10]), 2);
        assert_eq!(read_u32(byte_order, &idle[24..28]), 0x220902);
        assert_eq!(read_u32(byte_order, &idle[28..32]), 0x220903);
    }
}

#[test]
fn visibility_notify_uses_the_core_x11_layout() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let event = encode_x_client_event(
            byte_order,
            XClientEvent::VisibilityNotify {
                sequence: 0x1234,
                window: XResourceId::new(0x220901, 1),
                state: 0,
            },
        );
        assert_eq!(event.len(), X_CLIENT_OUTPUT_RECORD_LEN);
        assert_eq!(event[0], 15);
        assert_eq!(read_u16(byte_order, &event[2..4]), 0x1234);
        assert_eq!(read_u32(byte_order, &event[4..8]), 0x220901);
        assert_eq!(event[8], 0);
    }
}

#[test]
fn selection_events_use_core_x11_layout_and_preserve_send_event() {
    let owner = XResourceId::new(0x200001, 1);
    let requestor = XResourceId::new(0x400001, 1);
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let clear = encode_x_client_event(
            byte_order,
            XClientEvent::SelectionClear {
                sequence: 9,
                time: 11,
                owner,
                selection: 12,
            },
        );
        assert_eq!(clear[0], 29);
        assert_eq!(read_u32(byte_order, &clear[4..8]), 11);
        assert_eq!(read_u32(byte_order, &clear[8..12]), 0x200001);
        assert_eq!(read_u32(byte_order, &clear[12..16]), 12);

        let request = encode_x_client_event(
            byte_order,
            XClientEvent::SelectionRequest {
                sequence: 10,
                time: 13,
                owner,
                requestor,
                selection: 14,
                target: 15,
                property: 16,
            },
        );
        assert_eq!(request[0], 30);
        assert_eq!(read_u32(byte_order, &request[8..12]), 0x200001);
        assert_eq!(read_u32(byte_order, &request[12..16]), 0x400001);
        assert_eq!(read_u32(byte_order, &request[16..20]), 14);
        assert_eq!(read_u32(byte_order, &request[20..24]), 15);
        assert_eq!(read_u32(byte_order, &request[24..28]), 16);

        for (synthetic, expected_type) in [(false, 31), (true, 31 | 0x80)] {
            let notify = encode_x_client_event(
                byte_order,
                XClientEvent::SelectionNotify {
                    sequence: 17,
                    synthetic,
                    time: 18,
                    requestor,
                    selection: 19,
                    target: 20,
                    property: 21,
                },
            );
            assert_eq!(notify[0], expected_type);
            assert_eq!(read_u16(byte_order, &notify[2..4]), 17);
            assert_eq!(read_u32(byte_order, &notify[8..12]), 0x400001);
            assert_eq!(read_u32(byte_order, &notify[12..16]), 19);
            assert_eq!(read_u32(byte_order, &notify[16..20]), 20);
            assert_eq!(read_u32(byte_order, &notify[20..24]), 21);
        }
    }
}

#[test]
fn send_event_accepts_selection_notify_and_rejects_input_events() {
    let namespace = NamespaceId::from_raw(44);
    let byte_order = XByteOrder::LittleEndian;
    let mut request = vec![0; 44];
    request[0] = 25;
    request[2..4].copy_from_slice(&11u16.to_le_bytes());
    request[4..8].copy_from_slice(&0x200001u32.to_le_bytes());
    request[12] = 31;
    request[16..20].copy_from_slice(&17u32.to_le_bytes());
    request[20..24].copy_from_slice(&0x200001u32.to_le_bytes());
    request[24..28].copy_from_slice(&18u32.to_le_bytes());
    request[28..32].copy_from_slice(&19u32.to_le_bytes());
    request[32..36].copy_from_slice(&20u32.to_le_bytes());

    assert_eq!(
        decode_x11_core_request(context(namespace, 1, byte_order), &request).unwrap(),
        XWireRequest::SendSelectionNotify {
            destination: XResourceId::new(0x200001, 1),
            event_mask: 0,
            event: XClientEvent::SelectionNotify {
                sequence: 0,
                synthetic: true,
                time: 17,
                requestor: XResourceId::new(0x200001, 1),
                selection: 18,
                target: 19,
                property: 20,
            },
        }
    );

    request[8..12].copy_from_slice(&0x0018_0000u32.to_le_bytes());
    request[12] = 18;
    let mut unmap = [0; 32];
    unmap.copy_from_slice(&request[12..44]);
    assert_eq!(
        decode_x11_core_request(context(namespace, 1, byte_order), &request).unwrap(),
        XWireRequest::SendSelectionNotify {
            destination: XResourceId::new(0x200001, 1),
            event_mask: 0x0018_0000,
            event: XClientEvent::ClientMessage {
                sequence: 0,
                bytes: unmap,
            },
        }
    );

    request[12] = 0xf0;
    let mut extension_event = [0; 32];
    extension_event.copy_from_slice(&request[12..44]);
    assert_eq!(
        decode_x11_core_request(context(namespace, 1, byte_order), &request).unwrap(),
        XWireRequest::SendSelectionNotify {
            destination: XResourceId::new(0x200001, 1),
            event_mask: 0x0018_0000,
            event: XClientEvent::ClientMessage {
                sequence: 0,
                bytes: extension_event,
            },
        }
    );

    request[12] = 2;
    assert_eq!(
        decode_x11_core_request(context(namespace, 1, byte_order), &request),
        Err(XWireParseError::InvalidEventType(2))
    );
}

#[test]
fn xi2_decoder_accepts_query_pointer_and_ungrab_device() {
    let namespace = NamespaceId::from_raw(44);
    let mut request = vec![135, 40, 3, 0, 1, 0, 0x20, 0, 2, 0, 0, 0];

    assert_eq!(
        decode_x11_core_request(context(namespace, 1, XByteOrder::LittleEndian), &request).unwrap(),
        XWireRequest::XiQueryPointer {
            window: XResourceId::new(0x200001, 1),
            device_id: 2,
        }
    );

    request[1] = 52;
    request[4..8].copy_from_slice(&7u32.to_le_bytes());
    assert_eq!(
        decode_x11_core_request(context(namespace, 2, XByteOrder::LittleEndian), &request).unwrap(),
        XWireRequest::XiUngrabDevice {
            device_id: 2,
            time: 7,
        }
    );
}

#[test]
fn xi2_decoder_bounds_grab_device_event_mask() {
    let namespace = NamespaceId::from_raw(45);
    let mut request = vec![0_u8; 32];
    request[0] = X_INPUT_MAJOR_OPCODE;
    request[1] = X_INPUT_GRAB_DEVICE_MINOR_OPCODE;
    request[2..4].copy_from_slice(&8_u16.to_le_bytes());
    request[4..8].copy_from_slice(&0x200001_u32.to_le_bytes());
    request[8..12].copy_from_slice(&7_u32.to_le_bytes());
    request[12..16].copy_from_slice(&0x300001_u32.to_le_bytes());
    request[16..18].copy_from_slice(&2_u16.to_le_bytes());
    request[18] = 1;
    request[19] = 1;
    request[20] = 1;
    request[22..24].copy_from_slice(&2_u16.to_le_bytes());
    request[24..28].copy_from_slice(&0x00c0_u32.to_le_bytes());
    request[28..32].copy_from_slice(&1_u32.to_le_bytes());

    assert_eq!(
        decode_x11_core_request(context(namespace, 1, XByteOrder::LittleEndian), &request).unwrap(),
        XWireRequest::XiGrabDevice {
            window: XResourceId::new(0x200001, 1),
            time: 7,
            cursor: Some(XResourceId::new(0x300001, 1)),
            device_id: 2,
            pointer_mode: 1,
            keyboard_mode: 1,
            owner_events: true,
            event_mask: vec![0x00c0, 1],
        }
    );

    request[22..24].copy_from_slice(&9_u16.to_le_bytes());
    assert_eq!(
        decode_x11_core_request(context(namespace, 2, XByteOrder::LittleEndian), &request),
        Err(XWireParseError::InvalidValue(9))
    );
}

#[test]
fn legacy_xinput_device_bell_is_a_bounded_noop() {
    let namespace = NamespaceId::from_raw(45);
    let request = vec![
        X_INPUT_MAJOR_OPCODE,
        X_INPUT_DEVICE_BELL_MINOR_OPCODE,
        2,
        0,
        2,
        0,
        0,
        0,
    ];
    let decoded =
        decode_x11_core_request(context(namespace, 1, XByteOrder::LittleEndian), &request).unwrap();
    assert_eq!(decoded, XWireRequest::XiDeviceBell);

    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let dispatched = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, X_INPUT_MAJOR_OPCODE),
        decoded,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(dispatched.outputs.is_empty());

    assert!(
        decode_x11_core_request(
            context(namespace, 2, XByteOrder::LittleEndian),
            &request[..4],
        )
        .is_err()
    );
}

#[test]
fn core_keyboard_control_and_bell_requests_are_bounded() {
    let namespace = NamespaceId::from_raw(46);
    let keyboard_control = decode_x11_core_request(
        context(namespace, 1, XByteOrder::LittleEndian),
        &[103, 0, 1, 0],
    )
    .unwrap();
    assert_eq!(keyboard_control, XWireRequest::GetKeyboardControl);

    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let reply = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 103),
        keyboard_control,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = reply.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0].len(), 52);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][4..8]), 5);

    let bell = decode_x11_core_request(
        context(namespace, 2, XByteOrder::LittleEndian),
        &[104, 0, 1, 0],
    )
    .unwrap();
    assert_eq!(bell, XWireRequest::Bell);
}

fn warp_request(
    source: u32,
    destination: u32,
    src: (i16, i16, u16, u16),
    dst: (i16, i16),
) -> Vec<u8> {
    let mut bytes = vec![41u8, 0, 6, 0];
    bytes.extend_from_slice(&source.to_le_bytes());
    bytes.extend_from_slice(&destination.to_le_bytes());
    bytes.extend_from_slice(&src.0.to_le_bytes());
    bytes.extend_from_slice(&src.1.to_le_bytes());
    bytes.extend_from_slice(&src.2.to_le_bytes());
    bytes.extend_from_slice(&src.3.to_le_bytes());
    bytes.extend_from_slice(&dst.0.to_le_bytes());
    bytes.extend_from_slice(&dst.1.to_le_bytes());
    bytes
}

/// Dispatches one already-built request and returns what the client sees.
fn xts_repair_outputs(
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
    properties: &mut XPropertyTable,
    namespace: NamespaceId,
    sequence: u16,
    opcode: u8,
    request: &[u8],
) -> Vec<Vec<u8>> {
    let decoded = decode_x11_core_request(
        context(namespace, u64::from(sequence), XByteOrder::LittleEndian),
        request,
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, sequence, XByteOrder::LittleEndian, opcode),
        decoded,
        runtime,
        atoms,
        properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian)
}

#[test]
fn x11_change_property_refuses_an_atom_that_names_nothing() {
    let namespace = NamespaceId::from_raw(51);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let root = X_SETUP_DEFAULT_ROOT;

    // The property atom names nothing: the error reports that atom.
    let encoded = xts_repair_outputs(
        &mut runtime, &mut atoms, &mut properties, namespace, 1, 18,
        &change_property_request(
            XByteOrder::LittleEndian, XPropertyMode::Replace, root, 0xFFFF_FFFF, 19, 32, &[],
        ),
    );
    assert_eq!(encoded.len(), 1);
    assert_eq!(encoded[0][0], 0, "an error, not a reply");
    assert_eq!(encoded[0][1], 5, "BadAtom");
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[0][4..8]),
        0xFFFF_FFFF,
        "the error names the atom it refused"
    );

    // The type atom is checked too, and after the property atom.
    let encoded = xts_repair_outputs(
        &mut runtime, &mut atoms, &mut properties, namespace, 2, 18,
        &change_property_request(
            XByteOrder::LittleEndian, XPropertyMode::Replace, root, 39, 0xFFFF_FFFE, 32, &[],
        ),
    );
    assert_eq!(encoded[0][1], 5, "BadAtom");
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[0][4..8]),
        0xFFFF_FFFE
    );

    // Two predefined atoms are accepted, so the guard refuses only the invalid.
    let encoded = xts_repair_outputs(
        &mut runtime, &mut atoms, &mut properties, namespace, 3, 18,
        &change_property_request(
            XByteOrder::LittleEndian, XPropertyMode::Replace, root, 39, 19, 32, &[1, 0, 0, 0],
        ),
    );
    assert!(
        encoded.iter().all(|output| output[0] != 0),
        "a property named by valid atoms is stored"
    );
}

#[test]
fn x11_change_property_append_of_another_type_is_a_match_error() {
    let namespace = NamespaceId::from_raw(52);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let root = X_SETUP_DEFAULT_ROOT;

    // Seed the property as one type and format.
    xts_repair_outputs(
        &mut runtime, &mut atoms, &mut properties, namespace, 1, 18,
        &change_property_request(
            XByteOrder::LittleEndian, XPropertyMode::Replace, root, 39, 19, 32, &[1, 0, 0, 0],
        ),
    );

    // Appending a different type is the protocol's Match error, not a Value
    // error: both atoms are valid and the format is in range, and it is the
    // pair that does not agree.
    let encoded = xts_repair_outputs(
        &mut runtime, &mut atoms, &mut properties, namespace, 2, 18,
        &change_property_request(
            XByteOrder::LittleEndian, XPropertyMode::Append, root, 39, 5, 32, &[2, 0, 0, 0],
        ),
    );
    assert_eq!(encoded.len(), 1);
    assert_eq!(encoded[0][0], 0, "an error, not a reply");
    assert_eq!(encoded[0][1], 8, "BadMatch");

    // Prepending a different format is the same error.
    let encoded = xts_repair_outputs(
        &mut runtime, &mut atoms, &mut properties, namespace, 3, 18,
        &change_property_request(
            XByteOrder::LittleEndian, XPropertyMode::Prepend, root, 39, 19, 16, &[2, 0],
        ),
    );
    assert_eq!(encoded[0][1], 8, "BadMatch");
}

#[test]
fn x11_get_selection_owner_refuses_an_atom_that_names_nothing() {
    let namespace = NamespaceId::from_raw(53);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let encoded = xts_repair_outputs(
        &mut runtime, &mut atoms, &mut properties, namespace, 1, 23,
        &resource_request(XByteOrder::LittleEndian, 23, 0xFFFF_FFFF),
    );
    assert_eq!(encoded.len(), 1);
    assert_eq!(encoded[0][0], 0, "an error, not a reply");
    assert_eq!(encoded[0][1], 5, "BadAtom");
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[0][4..8]),
        0xFFFF_FFFF
    );

    // A valid atom nobody owns is still a reply: unowned and unnamed are
    // different answers and the client can reach both.
    let encoded = xts_repair_outputs(
        &mut runtime, &mut atoms, &mut properties, namespace, 2, 23,
        &resource_request(XByteOrder::LittleEndian, 23, 7),
    );
    assert_eq!(encoded[0][0], 1, "a reply");
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][8..12]), 0);
}

#[test]
fn x11_destroying_the_root_succeeds_and_destroys_nothing() {
    let namespace = NamespaceId::from_raw(54);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let encoded = xts_repair_outputs(
        &mut runtime, &mut atoms, &mut properties, namespace, 1, 4,
        &resource_request(XByteOrder::LittleEndian, 4, X_SETUP_DEFAULT_ROOT),
    );
    assert!(
        encoded.is_empty(),
        "the root is a real window with no parent: the request succeeds and does nothing"
    );
}

#[test]
fn x11_warp_pointer_decodes_every_field_and_refuses_a_request_of_the_wrong_length() {
    let namespace = NamespaceId::from_raw(48);
    let decoded = decode_x11_core_request(
        context(namespace, 1, XByteOrder::LittleEndian),
        &warp_request(0x200040, 0x200041, (3, 5, 17, 19), (-7, 11)),
    )
    .unwrap();
    assert_eq!(
        decoded,
        XWireRequest::WarpPointer {
            source: XResourceId::new(0x200040, 1),
            destination: XResourceId::new(0x200041, 1),
            src_x: 3,
            src_y: 5,
            src_width: 17,
            src_height: 19,
            dst_x: -7,
            dst_y: 11,
        }
    );

    // Both windows absent is the unconditional warp, and zero must survive
    // as zero rather than becoming a resource nobody named.
    let unconditional = decode_x11_core_request(
        context(namespace, 2, XByteOrder::LittleEndian),
        &warp_request(0, 0, (0, 0, 0, 0), (1, 2)),
    )
    .unwrap();
    assert_eq!(
        unconditional,
        XWireRequest::WarpPointer {
            source: XResourceId::new(0, 1),
            destination: XResourceId::new(0, 1),
            src_x: 0,
            src_y: 0,
            src_width: 0,
            src_height: 0,
            dst_x: 1,
            dst_y: 2,
        }
    );

    let mut overlong = warp_request(0, 0, (0, 0, 0, 0), (0, 0));
    overlong.extend_from_slice(&[0, 0, 0, 0]);
    assert!(
        decode_x11_core_request(context(namespace, 3, XByteOrder::LittleEndian), &overlong)
            .is_err(),
        "a warp longer than its six words is a length failure, not a warp"
    );
}

#[test]
fn x11_warp_pointer_refuses_a_window_nobody_created() {
    let namespace = NamespaceId::from_raw(49);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let request = decode_x11_core_request(
        context(namespace, 1, XByteOrder::LittleEndian),
        &warp_request(0, 0x200099, (0, 0, 0, 0), (4, 4)),
    )
    .unwrap();
    let encoded = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 41),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);

    assert_eq!(encoded.len(), 1, "a warp to nowhere is one error");
    assert_eq!(encoded[0][0], 0, "an error, not a reply");
    assert_eq!(encoded[0][1], 3, "BadWindow");
    assert_eq!(encoded[0][10], 41, "major opcode");
}

#[test]
fn x11_warp_pointer_moves_the_pointer_and_query_pointer_agrees() {
    let namespace = NamespaceId::from_raw(50);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let mut topology = sophia_protocol::OutputTopologySnapshot::deterministic();
    topology.generation = 9;
    runtime.update_output_topology(topology).unwrap();

    let warp = |runtime: &mut XAuthorityRuntime,
                atoms: &mut XAtomTable,
                properties: &mut XPropertyTable,
                sequence: u16,
                request: Vec<u8>| {
        let decoded = decode_x11_core_request(
            context(namespace, u64::from(sequence), XByteOrder::LittleEndian),
            &request,
        )
        .unwrap();
        dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, 41),
            decoded,
            runtime,
            atoms,
            properties,
        )
        .encoded_outputs(XByteOrder::LittleEndian)
    };
    // Read the position the way a client does, through QueryPointer on the
    // root, so what is asserted is what a client would actually see.
    let position = |runtime: &mut XAuthorityRuntime,
                    atoms: &mut XAtomTable,
                    properties: &mut XPropertyTable,
                    sequence: u16| {
        let mut request = vec![38u8, 0, 2, 0];
        request.extend_from_slice(&X_SETUP_DEFAULT_ROOT.to_le_bytes());
        let decoded = decode_x11_core_request(
            context(namespace, u64::from(sequence), XByteOrder::LittleEndian),
            &request,
        )
        .unwrap();
        let encoded = dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, 38),
            decoded,
            runtime,
            atoms,
            properties,
        )
        .encoded_outputs(XByteOrder::LittleEndian);
        assert_eq!(encoded.len(), 1, "QueryPointer answers once");
        assert_eq!(encoded[0][0], 1, "a reply, not an error");
        (
            i16::from_le_bytes([encoded[0][16], encoded[0][17]]),
            i16::from_le_bytes([encoded[0][18], encoded[0][19]]),
        )
    };

    // A warp to the root origin plus an offset is unconditional and silent,
    // and it is what gives the pointer its first position.
    let outputs = warp(
        &mut runtime,
        &mut atoms,
        &mut properties,
        1,
        warp_request(0, X_SETUP_DEFAULT_ROOT, (0, 0, 0, 0), (40, 25)),
    );
    assert!(outputs.is_empty(), "an accepted warp says nothing");
    assert_eq!(position(&mut runtime, &mut atoms, &mut properties, 101), (40, 25));

    // With no destination window the offset is from where the pointer is.
    let outputs = warp(
        &mut runtime,
        &mut atoms,
        &mut properties,
        2,
        warp_request(0, 0, (0, 0, 0, 0), (-10, 5)),
    );
    assert!(outputs.is_empty());
    assert_eq!(position(&mut runtime, &mut atoms, &mut properties, 102), (30, 30));

    // The pointer cannot be warped off the screen; it stops at the edge.
    let outputs = warp(
        &mut runtime,
        &mut atoms,
        &mut properties,
        3,
        warp_request(0, 0, (0, 0, 0, 0), (i16::MIN, i16::MIN)),
    );
    assert!(outputs.is_empty());
    assert_eq!(position(&mut runtime, &mut atoms, &mut properties, 103), (0, 0));

    // A source window the pointer is not inside makes the warp conditional
    // and it does not happen. The root holds the pointer, so a rectangle of
    // the root that excludes it is the honest way to say "not here".
    let outputs = warp(
        &mut runtime,
        &mut atoms,
        &mut properties,
        4,
        warp_request(X_SETUP_DEFAULT_ROOT, 0, (500, 500, 10, 10), (17, 19)),
    );
    assert!(outputs.is_empty(), "a warp that does not fire is not an error");
    assert_eq!(
        position(&mut runtime, &mut atoms, &mut properties, 104),
        (0, 0),
        "the pointer stayed where it was"
    );

    // The same warp with a rectangle that does contain the pointer fires.
    let outputs = warp(
        &mut runtime,
        &mut atoms,
        &mut properties,
        5,
        warp_request(X_SETUP_DEFAULT_ROOT, 0, (0, 0, 10, 10), (17, 19)),
    );
    assert!(outputs.is_empty());
    assert_eq!(position(&mut runtime, &mut atoms, &mut properties, 105), (17, 19));
}

#[test]
fn x11_force_screen_saver_accepts_both_modes_and_refuses_any_other() {
    let namespace = NamespaceId::from_raw(47);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    // Reset and Activate. This host blanks nothing, so the whole observable
    // is that neither is refused: every XTS test's startup resets the screen
    // saver, and a refusal there ends the test before its first assertion.
    for (sequence, mode) in [(1u16, 0u8), (2, 1)] {
        let request = decode_x11_core_request(
            context(namespace, u64::from(sequence), XByteOrder::LittleEndian),
            &[115, mode, 1, 0],
        )
        .unwrap();
        assert_eq!(request, XWireRequest::ForceScreenSaver { mode });
        let accepted = dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, 115),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        assert!(
            accepted
                .encoded_outputs(XByteOrder::LittleEndian)
                .is_empty(),
            "mode {mode} should be accepted silently"
        );
    }

    let refused = decode_x11_core_request(
        context(namespace, 3, XByteOrder::LittleEndian),
        &[115, 2, 1, 0],
    )
    .unwrap();
    assert_eq!(refused, XWireRequest::ForceScreenSaver { mode: 2 });
    let encoded = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 115),
        refused,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded.len(), 1, "an undefined mode is one error and nothing else");
    assert_eq!(encoded[0][0], 0, "an error, not a reply");
    assert_eq!(encoded[0][1], 2, "BadValue");
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[0][4..8]),
        2,
        "the error names the mode it refused"
    );
    assert_eq!(encoded[0][10], 115, "major opcode");

    // A request longer than its one word is a decode failure, not a mode.
    assert!(
        decode_x11_core_request(
            context(namespace, 4, XByteOrder::LittleEndian),
            &[115, 0, 2, 0, 0, 0, 0, 0],
        )
        .is_err()
    );
}

#[test]
fn x11_setup_parser_accepts_little_endian_auth_fields() {
    let bytes = setup_request(
        XByteOrder::LittleEndian,
        11,
        0,
        b"MIT-MAGIC-COOKIE-1",
        b"0123456789abcdef",
    );

    let request = parse_x11_setup_request(&bytes).unwrap();

    assert_eq!(request.byte_order, XByteOrder::LittleEndian);
    assert_eq!(request.major_version, 11);
    assert_eq!(request.minor_version, 0);
    assert_eq!(request.authorization_protocol_name, b"MIT-MAGIC-COOKIE-1");
    assert_eq!(request.authorization_data, b"0123456789abcdef");
    assert_eq!(
        x11_setup_request_total_len(&bytes[..12]).unwrap(),
        bytes.len()
    );
}

#[test]
fn x11_setup_parser_accepts_big_endian_empty_auth() {
    let bytes = setup_request(XByteOrder::BigEndian, 11, 0, b"", b"");

    let request = parse_x11_setup_request(&bytes).unwrap();

    assert_eq!(request.byte_order, XByteOrder::BigEndian);
    assert_eq!(request.major_version, 11);
    assert!(request.authorization_protocol_name.is_empty());
    assert!(request.authorization_data.is_empty());
}

#[test]
fn x11_setup_parser_rejects_malformed_inputs() {
    assert_eq!(
        parse_x11_setup_request(&[b'l'; 4]),
        Err(XSetupParseError::Truncated {
            needed: 12,
            actual: 4
        })
    );
    assert_eq!(
        parse_x11_setup_request(&setup_request(XByteOrder::LittleEndian, 12, 0, b"", b"")),
        Err(XSetupParseError::UnsupportedMajorVersion(12))
    );
    assert_eq!(
        parse_x11_setup_request(&[b'x', 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        Err(XSetupParseError::InvalidByteOrder(b'x'))
    );

    let mut overlarge = setup_request(XByteOrder::LittleEndian, 11, 0, b"", b"");
    overlarge[6..8].copy_from_slice(&1025u16.to_le_bytes());
    assert_eq!(
        parse_x11_setup_request(&overlarge),
        Err(XSetupParseError::AuthFieldTooLarge {
            field: "authorization_protocol_name",
            len: 1025,
            max: X_SETUP_MAX_AUTH_FIELD_LEN,
        })
    );

    let mut truncated = setup_request(XByteOrder::LittleEndian, 11, 0, b"AUTH", b"DATA");
    truncated.pop();
    assert!(matches!(
        parse_x11_setup_request(&truncated),
        Err(XSetupParseError::Truncated { .. })
    ));
}

#[test]
fn x11_setup_success_reply_encodes_resource_id_facts() {
    let reply = encode_x11_setup_success(
        XByteOrder::LittleEndian,
        &XSetupSuccess {
            major_version: 11,
            minor_version: 0,
            release: 7,
            resource_id_base: 0x0020_0000,
            resource_id_mask: 0x001f_ffff,
            max_request_units: 4096,
            vendor: b"Sophia".to_vec(),
            roots: 0,
            formats: 0,
            root_size: Size {
                width: 1280,
                height: 720,
            },
        },
    )
    .unwrap();

    assert_eq!(reply[0], 1);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &reply[2..4]), 11);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &reply[4..6]), 0);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &reply[8..12]), 7);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &reply[12..16]),
        0x0020_0000
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &reply[16..20]),
        0x001f_ffff
    );
    assert_eq!(read_u16(XByteOrder::LittleEndian, &reply[24..26]), 6);
    assert_eq!(&reply[40..46], b"Sophia");
}

#[test]
fn x11_setup_success_reply_advertises_exact_true_color_visuals_in_both_orders() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let reply =
            encode_x11_setup_success(byte_order, &XSetupSuccess::client_compatible()).unwrap();

        assert_eq!(reply[0], 1);
        assert_eq!(reply[28], 1);
        assert_eq!(reply[29], 7);
        for (offset, expected) in [
            (48, [1, 1, 32]),
            (56, [4, 4, 32]),
            (64, [8, 8, 32]),
            (72, [15, 16, 32]),
            (80, [16, 16, 32]),
            (88, [24, 32, 32]),
            (96, [32, 32, 32]),
        ] {
            assert_eq!(&reply[offset..offset + 3], &expected);
        }
        assert_eq!(read_u32(byte_order, &reply[104..108]), X_SETUP_DEFAULT_ROOT);
        assert_eq!(
            read_u32(byte_order, &reply[108..112]),
            X_SETUP_DEFAULT_COLORMAP
        );
        assert_eq!(read_u32(byte_order, &reply[136..140]), X_SETUP_DEFAULT_VISUAL);
        assert_eq!(reply[142], 24);
        assert_eq!(reply[143], 7);
        for (offset, depth) in [(144, 1), (152, 4), (160, 8), (168, 15), (176, 16)] {
            assert_eq!(reply[offset], depth);
            assert_eq!(read_u16(byte_order, &reply[offset + 2..offset + 4]), 0);
        }

        for (offset, visual, depth) in [
            (192, X_SETUP_DEFAULT_VISUAL, 24),
            (224, X_SETUP_ARGB_VISUAL, 32),
        ] {
            assert_eq!(read_u32(byte_order, &reply[offset..offset + 4]), visual);
            assert_eq!(reply[offset + 4], 4);
            assert_eq!(reply[offset + 5], 8);
            assert_eq!(read_u16(byte_order, &reply[offset + 6..offset + 8]), 256);
            assert_eq!(
                read_u32(byte_order, &reply[offset + 8..offset + 12]),
                X_TRUE_COLOR_RED_MASK
            );
            assert_eq!(
                read_u32(byte_order, &reply[offset + 12..offset + 16]),
                X_TRUE_COLOR_GREEN_MASK
            );
            assert_eq!(
                read_u32(byte_order, &reply[offset + 16..offset + 20]),
                X_TRUE_COLOR_BLUE_MASK
            );
            assert_eq!(reply[offset - 8], depth);
        }
    }
}

#[test]
fn glx_vendor_string_is_nul_terminated_even_at_four_bytes() {
    let encoded = encode_x_client_output(
        XByteOrder::LittleEndian,
        XClientOutput::Reply(XClientReply::GlxString {
            sequence: 9,
            value: "mesa".to_owned(),
        }),
    );
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[12..16]), 5);
    assert_eq!(&encoded[32..37], b"mesa\0");
    assert_eq!(encoded.len(), 40);
}

#[test]
fn glx_config_requests_decode_in_both_byte_orders() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        for (minor, expected) in [
            (
                X_GLX_GET_VISUAL_CONFIGS_MINOR_OPCODE,
                XWireRequest::GlxGetVisualConfigs { screen: 0 },
            ),
            (
                X_GLX_GET_FB_CONFIGS_MINOR_OPCODE,
                XWireRequest::GlxGetFbConfigs { screen: 0 },
            ),
        ] {
            let mut request = vec![X_GLX_MAJOR_OPCODE, minor, 0, 0, 0, 0, 0, 0];
            match byte_order {
                XByteOrder::LittleEndian => request[2..4].copy_from_slice(&2u16.to_le_bytes()),
                XByteOrder::BigEndian => request[2..4].copy_from_slice(&2u16.to_be_bytes()),
            }
            assert_eq!(
                decode_x11_core_request(context(NamespaceId::from_raw(1), 1, byte_order), &request)
                    .unwrap(),
                expected
            );
        }
    }
}

#[test]
fn glx_fb_config_reply_uses_tagged_attribute_pairs() {
    let configs = vec![vec![(0x8013, 3), (0x800b, X_SETUP_ARGB_VISUAL)]];
    let encoded = encode_x_client_output(
        XByteOrder::BigEndian,
        XClientOutput::Reply(XClientReply::GlxFbConfigs {
            sequence: 11,
            configs,
        }),
    );
    assert_eq!(read_u32(XByteOrder::BigEndian, &encoded[8..12]), 1);
    assert_eq!(read_u32(XByteOrder::BigEndian, &encoded[12..16]), 2);
    assert_eq!(read_u32(XByteOrder::BigEndian, &encoded[32..36]), 0x8013);
    assert_eq!(read_u32(XByteOrder::BigEndian, &encoded[36..40]), 3);
    assert_eq!(
        read_u32(XByteOrder::BigEndian, &encoded[44..48]),
        X_SETUP_ARGB_VISUAL
    );
}

#[test]
fn legacy_glx_context_uses_a_visual_and_normalizes_to_its_fbconfig() {
    let byte_order = XByteOrder::LittleEndian;
    let namespace = NamespaceId::from_raw(1);
    let context_id = XResourceId::new(0x0020_000b, 1);
    let mut request = vec![X_GLX_MAJOR_OPCODE, X_GLX_CREATE_CONTEXT_MINOR_OPCODE];
    push_u16(&mut request, byte_order, 6);
    push_u32(&mut request, byte_order, context_id.local.raw() as u32);
    push_u32(&mut request, byte_order, X_SETUP_DEFAULT_VISUAL);
    push_u32(&mut request, byte_order, 0);
    push_u32(&mut request, byte_order, 0);
    request.extend_from_slice(&[1, 0, 0, 0]);

    let decoded =
        decode_x11_core_request(context(namespace, 1, byte_order), &request).unwrap();
    assert_eq!(
        decoded,
        XWireRequest::GlxCreateContext {
            context: context_id,
            config: XGlxContextConfig::Visual(X_SETUP_DEFAULT_VISUAL),
            screen: 0,
            share: None,
            direct: true,
        }
    );

    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let drawable = XResourceId::new(0x0020_000c, 1);
    runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(1),
        namespace,
        kind: XAuthorityRequestKind::CreateWindow {
            window: drawable,
            surface: SurfaceId::new(90, 1),
            geometry: Rect {
                x: 0,
                y: 0,
                width: 500,
                height: 500,
            },
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    });
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 12, byte_order, X_GLX_MAJOR_OPCODE),
        decoded,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(result.outputs.is_empty());
    assert_eq!(runtime.glx_context(namespace, context_id), Ok((1, true)));

    let mut make_current = vec![X_GLX_MAJOR_OPCODE, X_GLX_MAKE_CURRENT_MINOR_OPCODE];
    push_u16(&mut make_current, byte_order, 4);
    push_u32(&mut make_current, byte_order, drawable.local.raw() as u32);
    push_u32(&mut make_current, byte_order, context_id.local.raw() as u32);
    push_u32(&mut make_current, byte_order, 0);
    let decoded =
        decode_x11_core_request(context(namespace, 2, byte_order), &make_current).unwrap();
    assert_eq!(
        decoded,
        XWireRequest::GlxMakeCurrent {
            drawable: Some(drawable),
            context: Some(context_id),
            old_context_tag: 0,
        }
    );
    let encoded = dispatch_x11_wire_request(
        dispatch_context(namespace, 13, byte_order, X_GLX_MAJOR_OPCODE),
        decoded,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(byte_order)
    .remove(0);
    assert_eq!(read_u32(byte_order, &encoded[8..12]), 1);
}

#[test]
fn kitty_glx_context_attribs_layout_decodes_the_28_byte_header() {
    let byte_order = XByteOrder::LittleEndian;
    let mut request = vec![
        X_GLX_MAJOR_OPCODE,
        X_GLX_CREATE_CONTEXT_ATTRIBS_ARB_MINOR_OPCODE,
    ];
    push_u16(&mut request, byte_order, 13);
    push_u32(&mut request, byte_order, 0x0020_000b);
    push_u32(&mut request, byte_order, 3);
    push_u32(&mut request, byte_order, 0);
    push_u32(&mut request, byte_order, 0);
    request.extend_from_slice(&[1, 0, 0, 0]);
    push_u32(&mut request, byte_order, 3);
    for (attribute, value) in [(0x2091, 3), (0x2092, 1), (0x9126, 1)] {
        push_u32(&mut request, byte_order, attribute);
        push_u32(&mut request, byte_order, value);
    }
    assert_eq!(request.len(), 52);
    assert_eq!(
        decode_x11_core_request(context(NamespaceId::from_raw(1), 1, byte_order), &request)
            .unwrap(),
        XWireRequest::GlxCreateContext {
            context: XResourceId::new(0x0020_000b, 1),
            config: XGlxContextConfig::FbConfig(3),
            screen: 0,
            share: None,
            direct: true,
        }
    );
}

#[test]
fn kitty_fbconfig_catalog_has_argb_blue_aux_and_srgb_attributes() {
    let namespace = NamespaceId::from_raw(1);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let encoded = dispatch_x11_wire_request(
        dispatch_context(namespace, 12, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        XWireRequest::GlxGetFbConfigs { screen: 0 },
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian)
    .remove(0);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[8..12]), 3);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[12..16]), 29);
    let pair = |config: usize, attribute: usize| 32 + (config * 29 + attribute) * 8;
    let aux = pair(0, 10);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[aux..aux + 4]),
        7
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[aux + 4..aux + 8]),
        0
    );
    let blue = pair(0, 13);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[blue..blue + 4]),
        10
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[blue + 4..blue + 8]),
        8
    );
    let depth = pair(0, 15);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[depth..depth + 4]),
        12
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[depth + 4..depth + 8]),
        24
    );
    let srgb = pair(2, 25);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[srgb..srgb + 4]),
        0x20b2
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[srgb + 4..srgb + 8]),
        1
    );
}

#[test]
fn legacy_glx_visual_catalog_has_rgba_double_buffer_and_depth() {
    let namespace = NamespaceId::from_raw(1);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let encoded = dispatch_x11_wire_request(
        dispatch_context(namespace, 12, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        XWireRequest::GlxGetVisualConfigs { screen: 0 },
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian)
    .remove(0);

    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[8..12]), 2);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[12..16]), 18);
    let field = |config: usize, attribute: usize| 32 + (config * 18 + attribute) * 4;
    assert_eq!(
        read_u32(
            XByteOrder::LittleEndian,
            &encoded[field(0, 2)..field(0, 2) + 4]
        ),
        1
    );
    assert_eq!(
        read_u32(
            XByteOrder::LittleEndian,
            &encoded[field(0, 11)..field(0, 11) + 4]
        ),
        1
    );
    assert_eq!(
        read_u32(
            XByteOrder::LittleEndian,
            &encoded[field(0, 14)..field(0, 14) + 4]
        ),
        24
    );
}

#[test]
fn sync_counter_and_kitty_teardown_requests_decode() {
    let namespace = NamespaceId::from_raw(1);
    let mut initialize = vec![
        X_SYNC_MAJOR_OPCODE,
        X_SYNC_INITIALIZE_MINOR_OPCODE,
        2,
        0,
        3,
        1,
        0,
        0,
    ];
    initialize[2..4].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        decode_x11_core_request(context(namespace, 1, XByteOrder::LittleEndian), &initialize)
            .unwrap(),
        XWireRequest::SyncInitialize {
            desired_major: 3,
            desired_minor: 1
        }
    );
    let counter = 0x0020_0010u32;
    for (minor, value, expected) in [
        (
            X_SYNC_CREATE_COUNTER_MINOR_OPCODE,
            -2i64,
            XWireRequest::SyncCreateCounter {
                counter: XResourceId::new(u64::from(counter), 1),
                initial_value: -2,
            },
        ),
        (
            X_SYNC_SET_COUNTER_MINOR_OPCODE,
            17,
            XWireRequest::SyncSetCounter {
                counter: XResourceId::new(u64::from(counter), 1),
                value: 17,
            },
        ),
        (
            X_SYNC_CHANGE_COUNTER_MINOR_OPCODE,
            -3,
            XWireRequest::SyncChangeCounter {
                counter: XResourceId::new(u64::from(counter), 1),
                delta: -3,
            },
        ),
    ] {
        let mut request = vec![X_SYNC_MAJOR_OPCODE, minor, 4, 0];
        request.extend_from_slice(&counter.to_le_bytes());
        request.extend_from_slice(&((value >> 32) as u32).to_le_bytes());
        request.extend_from_slice(&(value as u32).to_le_bytes());
        assert_eq!(
            decode_x11_core_request(context(namespace, 2, XByteOrder::LittleEndian), &request)
                .unwrap(),
            expected
        );
    }
    for (minor, expected) in [
        (
            X_SYNC_QUERY_COUNTER_MINOR_OPCODE,
            XWireRequest::SyncQueryCounter {
                counter: XResourceId::new(u64::from(counter), 1),
            },
        ),
        (
            X_SYNC_DESTROY_COUNTER_MINOR_OPCODE,
            XWireRequest::SyncDestroyCounter {
                counter: XResourceId::new(u64::from(counter), 1),
            },
        ),
    ] {
        let mut request = vec![X_SYNC_MAJOR_OPCODE, minor, 2, 0];
        request.extend_from_slice(&counter.to_le_bytes());
        assert_eq!(
            decode_x11_core_request(context(namespace, 3, XByteOrder::LittleEndian), &request)
                .unwrap(),
            expected
        );
    }
    for (major, minor, expected) in [
        (
            X_SYNC_MAJOR_OPCODE,
            X_SYNC_DESTROY_FENCE_MINOR_OPCODE,
            XWireRequest::SyncDestroyFence {
                fence: XResourceId::new(0x0020_000f, 1),
            },
        ),
        (
            79,
            0,
            XWireRequest::FreeColormap {
                colormap: XResourceId::new(0x0020_0008, 1),
            },
        ),
    ] {
        let mut request = vec![major, minor, 2, 0];
        request.extend_from_slice(&0x0020_000fu32.to_le_bytes());
        if major == 79 {
            request[4..8].copy_from_slice(&0x0020_0008u32.to_le_bytes());
        }
        assert_eq!(
            decode_x11_core_request(context(namespace, 2, XByteOrder::LittleEndian), &request)
                .unwrap(),
            expected
        );
    }
}
