// BIG-REQUESTS framing over a real socket.
//
// WHAT THESE PROVE. The authority advertises BIG-REQUESTS and answers
// BigReqEnable, so a client of X11R6 lineage -- XTS5's own Xlib among them
// -- sends a request longer than 65535 units with a zero length field and
// a 32-bit length after it. Once a connection has enabled the extension,
// the reader frames that: a request within the maximum is handed on as the
// ordinary request it carries, and one beyond the maximum is read to its
// end, dropped, and answered with exactly one BadLength, as the reference
// server does. Before t174 the zero-length header was a four-byte request
// and the 262 KB body some 65 000 opcode-0 requests, each answered, where
// XTS5's TOO_LONG purpose wants one BadLength and then nothing.
//
// Until the connection enables the extension a zero length field means
// what the core protocol says: a four-byte request. That is the control.

#[cfg(unix)]
mod big_requests {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const BAD_LENGTH: u8 = 16;
    const INTERN_ATOM: u8 = 16;
    const GET_INPUT_FOCUS: u8 = 43;
    const ATOM_NAME: &str = "SOPHIA_BIG_REQUEST";

    fn served_client(
        name: &str,
        byte_order: XByteOrder,
    ) -> (std::os::unix::net::UnixStream, std::path::PathBuf, thread::JoinHandle<()>) {
        let socket_path = std::env::temp_dir().join(format!(
            "sophia-x-big-requests-{name}-{}-{}.sock",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let namespace = NamespaceContext::new(
            NamespaceId::from_raw(1174),
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
        (client, socket_path, server)
    }

    fn big_req_enable(byte_order: XByteOrder) -> Vec<u8> {
        let mut out = vec![X_BIG_REQUESTS_MAJOR_OPCODE, 0];
        push_u16(&mut out, byte_order, 1);
        out
    }

    fn get_input_focus(byte_order: XByteOrder) -> Vec<u8> {
        let mut out = vec![GET_INPUT_FOCUS, 0];
        push_u16(&mut out, byte_order, 1);
        out
    }

    /// An ordinary request re-framed with the extended length encoding: a
    /// zero length field, then the length in units as a 32-bit field, then
    /// the rest of the request unchanged.
    fn extended(byte_order: XByteOrder, ordinary: &[u8]) -> Vec<u8> {
        let units = u32::try_from(ordinary.len() / 4).unwrap();
        let mut out = vec![ordinary[0], ordinary[1]];
        push_u16(&mut out, byte_order, 0);
        push_u32(&mut out, byte_order, units + 1);
        out.extend_from_slice(&ordinary[4..]);
        out
    }

    fn sequence(byte_order: XByteOrder, record: &[u8; 32]) -> u16 {
        read_u16(byte_order, &record[2..4])
    }

    fn enable(client: &mut std::os::unix::net::UnixStream, byte_order: XByteOrder) {
        client.write_all(&big_req_enable(byte_order)).unwrap();
        let record = read_x_record(client);
        assert_eq!(record[0], 1, "{byte_order:?}: BigReqEnable is answered with its reply");
        assert_eq!(sequence(byte_order, &record), 1);
        assert!(
            read_u32(byte_order, &record[8..12]) >= u32::from(X_SETUP_DEFAULT_MAX_REQUEST_UNITS),
            "{byte_order:?}: the extension's maximum is no less than the setup's"
        );
    }

    #[test]
    fn an_extended_request_beyond_the_maximum_is_answered_with_one_bad_length() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (mut client, socket_path, server) = served_client("beyond", byte_order);
            enable(&mut client, byte_order);

            // XTS5's TOO_LONG: one unit past the maximum, the whole frame
            // sent in one write. 262 144 bytes, of which the reader must
            // consume every one without answering any of them.
            let units = u32::from(X_SETUP_DEFAULT_MAX_REQUEST_UNITS) + 1;
            let mut frame = vec![GET_INPUT_FOCUS, 0];
            push_u16(&mut frame, byte_order, 0);
            push_u32(&mut frame, byte_order, units);
            frame.resize(units as usize * 4, 0);
            client
                .set_write_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            client
                .write_all(&frame)
                .expect("the frame is read to its end by a server that does not answer it piecemeal");
            client.write_all(&get_input_focus(byte_order)).unwrap();

            let record = read_x_record(&mut client);
            assert_eq!(
                (record[0], record[1]),
                (0, BAD_LENGTH),
                "{byte_order:?}: a request beyond the maximum is BadLength"
            );
            assert_eq!(sequence(byte_order, &record), 2, "{byte_order:?}: for the frame's own sequence");
            assert_eq!(record[10], GET_INPUT_FOCUS, "{byte_order:?}: naming the frame's opcode");
            let record = read_x_record(&mut client);
            assert_eq!(
                record[0], 1,
                "{byte_order:?}: the request after the frame is answered next, nothing between"
            );
            assert_eq!(sequence(byte_order, &record), 3);

            drop(client);
            server.join().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    #[test]
    fn an_extended_request_within_the_maximum_is_served_as_the_request_it_frames() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (mut client, socket_path, server) = served_client("within", byte_order);
            enable(&mut client, byte_order);

            let ordinary = intern_atom_request(byte_order, false, ATOM_NAME);
            client.write_all(&extended(byte_order, &ordinary)).unwrap();
            client.write_all(&ordinary).unwrap();

            let framed = read_x_record(&mut client);
            assert_eq!(
                framed[0], 1,
                "{byte_order:?}: the extended frame is the InternAtom it carries: {framed:?}"
            );
            assert_eq!(sequence(byte_order, &framed), 2);
            let atom = read_u32(byte_order, &framed[8..12]);
            assert_ne!(atom, 0, "{byte_order:?}: the name was interned");
            let plain = read_x_record(&mut client);
            assert_eq!(plain[0], 1);
            assert_eq!(sequence(byte_order, &plain), 3);
            assert_eq!(
                read_u32(byte_order, &plain[8..12]),
                atom,
                "{byte_order:?}: the same name, framed either way, is the same atom"
            );

            drop(client);
            server.join().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    /// THE CONTROL. Framing is what BigReqEnable buys; a connection that never
    /// asked keeps the core protocol's reading of a zero length field.
    #[test]
    fn a_zero_length_field_is_a_four_byte_request_until_the_extension_is_enabled() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (mut client, socket_path, server) = served_client("control", byte_order);

            // A zero-length InternAtom header, then GetInputFocus. Framed as
            // extended, the header would swallow the four bytes after it as
            // a length and the connection would wait for a body.
            let mut zero = vec![INTERN_ATOM, 0];
            push_u16(&mut zero, byte_order, 0);
            client.write_all(&zero).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();

            let record = read_x_record(&mut client);
            assert_eq!(
                (record[0], record[1]),
                (0, BAD_LENGTH),
                "{byte_order:?}: a four-byte InternAtom is too short: {record:?}"
            );
            assert_eq!(sequence(byte_order, &record), 1);
            let record = read_x_record(&mut client);
            assert_eq!(record[0], 1, "{byte_order:?}: GetInputFocus was the next request");
            assert_eq!(sequence(byte_order, &record), 2);

            drop(client);
            server.join().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }
}
