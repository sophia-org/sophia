// Core LookupColor goes through the real socket decoder, authority and encoder.

fn lookup_color_request(order: XByteOrder, colormap: u32, name: &str) -> Vec<u8> {
    let mut request = alloc_named_color_request(order, colormap, name);
    request[0] = 92;
    request
}

#[test]
fn lookup_color_socket_replies_and_errors_preserve_sequence() {
    use std::{io::Write, sync::Arc, thread, time::{SystemTime, UNIX_EPOCH}};
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let path = std::env::temp_dir().join(format!("sophia-lookup-{}-{}.sock",
            std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let namespace = NamespaceContext::new(NamespaceId::from_raw(834),
            NamespaceProfile::ClassicShared, NamespaceCapabilities::NONE).unwrap();
        let config = XServerFrontendConfig::new_with_namespace_context(&path, namespace).unwrap()
            .with_admission_policy(Arc::new(TestXAdmissionPolicy::new(namespace, false)));
        let server = thread::spawn(move || {
            XServerFrontend::bind(config).unwrap().serve_next().unwrap();
        });
        wait_for_socket(&path);
        let mut client = connect_x_socket(&path);
        client.write_all(&setup_request(order, 11, 0, b"", b"")).unwrap();
        read_setup_success(&mut client, order);
        for (index, (map, name, error)) in [
            (X_SETUP_DEFAULT_COLORMAP, "LiGhT Gray", 0),
            (X_SETUP_DEFAULT_COLORMAP, "not-a-retained-color", 15),
            (0x765432, "white", 12),
            (X_SETUP_DEFAULT_COLORMAP, "white", 0),
        ].into_iter().enumerate() {
            client.write_all(&lookup_color_request(order, map, name)).unwrap();
            let reply = read_x_reply(&mut client, order);
            assert_eq!(read_u16(order, &reply[2..4]), index as u16 + 1);
            assert_eq!(reply.len(), 32);
            if error != 0 {
                assert_eq!((reply[0], reply[1], reply[10]), (0, error, 92));
            } else {
                assert_eq!(reply[0], 1);
                assert_eq!(read_u32(order, &reply[4..8]), 0);
                let value = if index == 0 { 0xd3d3 } else { 0xffff };
                for offset in [8, 10, 12, 14, 16, 18] {
                    assert_eq!(read_u16(order, &reply[offset..offset + 2]), value);
                }
                assert_eq!(&reply[20..], &[0; 12]);
            }
        }
        drop(client);
        server.join().unwrap();
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn lookup_color_checks_lengths_and_latin1_without_allocating() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let ns = NamespaceId::from_raw(835);
        let ctx = context(ns, 1, order);
        let mut request = lookup_color_request(order, X_SETUP_DEFAULT_COLORMAP, "red");
        request[12] = 0xe9;
        assert!(matches!(decode_x11_core_request(ctx, &request).unwrap(),
            XWireRequest::Core(sophia_x_authority::XCoreRequest::LookupColor { name, .. }) if name == "éed"));
        request.pop();
        assert!(decode_x11_core_request(ctx, &request).is_err());
        let oversized = lookup_color_request(order, X_SETUP_DEFAULT_COLORMAP, &"x".repeat(257));
        assert!(decode_x11_core_request(ctx, &oversized).is_err());
        let mut runtime = XAuthorityRuntime::new();
        let mut atoms = XAtomTable::new();
        let mut properties = XPropertyTable::new();
        // A private colormap in a different namespace cannot be queried.
        let created = decode_x11_core_request(ctx, &create_colormap_request(order,
            0x200001, X_SETUP_DEFAULT_ROOT, X_SETUP_DEFAULT_VISUAL)).unwrap();
        dispatch_x11_wire_request(dispatch_context(ns, 1, order, 78), created,
            &mut runtime, &mut atoms, &mut properties);
        let other = NamespaceId::from_raw(836);
        let lookup = decode_x11_core_request(context(other, 2, order),
            &lookup_color_request(order, 0x200001, "white")).unwrap();
        let reply = dispatch_x11_wire_request(dispatch_context(other, 2, order, 92), lookup,
            &mut runtime, &mut atoms, &mut properties).encoded_outputs(order);
        assert_eq!((reply[0][0], reply[0][1]), (0, 12));
    }
}
