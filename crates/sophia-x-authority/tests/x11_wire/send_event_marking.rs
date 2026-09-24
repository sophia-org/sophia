// SendEvent marks what it delivers.
//
// WHAT THIS PROVES. An event a client sends through SendEvent arrives with
// bit 7 of its type set, whatever the client put in its template: that is
// how a recipient tells a sent event from one the server generated, and
// what Xt and every toolkit read as `send_event`. Before t168 the
// ClientMessage form was copied verbatim, so a template with a clear bit
// arrived as if the server had generated it (XTS5 pSendEvent 1: "Expected
// MSB set in event type ClientMessage; got 0"). SelectionNotify already
// carried the bit.

#[cfg(unix)]
#[test]
fn a_sent_client_message_arrives_with_bit_seven_set() {
    use std::io::Write;
    use std::sync::Arc;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    const CLIENT_MESSAGE: u8 = 33;

    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let socket_path = std::env::temp_dir().join(format!(
            "sophia-x-send-event-marking-{}-{}.sock",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let namespace = NamespaceContext::new(
            NamespaceId::from_raw(1168),
            NamespaceProfile::ClassicShared,
            NamespaceCapabilities::NONE,
        )
        .unwrap();
        let policy = Arc::new(TestXAdmissionPolicy::new(namespace, false));
        let config = XServerFrontendConfig::new_with_namespace_context(&socket_path, namespace)
            .unwrap()
            .with_admission_policy(policy);
        let server = thread::spawn(move || {
            let mut frontend = XServerFrontend::bind(config).unwrap();
            frontend.serve_next().unwrap();
        });
        wait_for_socket(&socket_path);
        let mut client = connect_x_socket(&socket_path);
        client
            .write_all(&setup_request(byte_order, 11, 0, b"", b""))
            .unwrap();
        read_setup_success(&mut client, byte_order);

        // Sequence 1: a window of our own to address.
        let window = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 1;
        client
            .write_all(&create_window_request(byte_order, window, 0, 0, 64, 48))
            .unwrap();
        // Sequence 2: SendEvent, propagate false, no mask, a ClientMessage
        // whose template has bit 7 clear, as Xlib's XSendEvent leaves it.
        let mut request = vec![25, 0];
        push_u16(&mut request, byte_order, 11);
        push_u32(&mut request, byte_order, window);
        push_u32(&mut request, byte_order, 0);
        let mut template = [0u8; 32];
        template[0] = CLIENT_MESSAGE;
        template[1] = 32;
        request.extend_from_slice(&template);
        client.write_all(&request).unwrap();

        let record = read_x_record(&mut client);
        assert_eq!(
            record[0],
            CLIENT_MESSAGE | 0x80,
            "{byte_order:?}: a sent event is marked as sent: {record:?}"
        );
        assert_eq!(record[1], 32, "{byte_order:?}: the rest of the template is delivered as sent");
        assert_eq!(read_u16(byte_order, &record[2..4]), 2, "{byte_order:?}: stamped with the request's sequence");

        drop(client);
        server.join().unwrap();
        let _ = std::fs::remove_file(&socket_path);
    }
}
