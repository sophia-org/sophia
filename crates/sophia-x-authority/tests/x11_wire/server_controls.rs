// Advisory server controls over a real socket: pointer control, the screen
// saver, keyboard control, motion history, and the host access list.
//
// WHAT THESE PROVE. What a client sets it reads back, validated as the
// protocol validates it, and this authority acts on none of it: the Engine
// owns pointer acceleration, the session owns key repeat, nothing here
// blanks a screen. GetMotionEvents replies no events (no history is kept,
// which the protocol allows), ListHosts an empty list with access control
// enabled, and ChangeHosts and SetAccessControl are BadAccess: admission is
// by namespace and peer credentials, and no client may change a list that
// decides nothing. Before t166 every one of these opcodes was BadRequest,
// which xterm's error handler turns into an exit.

#[cfg(unix)]
mod server_controls {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    const BAD_VALUE: u8 = 2;
    const BAD_WINDOW: u8 = 3;
    const BAD_MATCH: u8 = 8;
    const BAD_ACCESS: u8 = 10;
    const BAD_LENGTH: u8 = 16;

    fn served(name: &str, byte_order: XByteOrder) -> (std::os::unix::net::UnixStream, std::path::PathBuf, thread::JoinHandle<()>) {
        let socket_path = std::env::temp_dir().join(format!(
            "sophia-x-server-controls-{name}-{}-{}.sock",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let namespace = NamespaceContext::new(
            NamespaceId::from_raw(1166),
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
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        (client, socket_path, server)
    }

    fn request(byte_order: XByteOrder, opcode: u8, detail: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![opcode, detail];
        push_u16(&mut out, byte_order, 1 + (body.len() / 4) as u16);
        out.extend_from_slice(body);
        out
    }

    fn words(byte_order: XByteOrder, values: &[u32]) -> Vec<u8> {
        let mut out = Vec::new();
        for value in values {
            push_u32(&mut out, byte_order, *value);
        }
        out
    }

    fn expect_error(byte_order: XByteOrder, record: &[u8; 32], code: u8, opcode: u8, what: &str) {
        assert_eq!((record[0], record[1], record[10]), (0, code, opcode), "{byte_order:?}: {what}: {record:?}");
    }

    fn reply(byte_order: XByteOrder, client: &mut std::os::unix::net::UnixStream, what: &str) -> [u8; 32] {
        let record = read_x_record(client);
        assert_eq!(record[0], 1, "{byte_order:?}: {what} replies: {record:?}");
        record
    }

    #[test]
    fn what_a_client_sets_it_reads_back_and_a_bad_value_is_named() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (mut client, socket_path, server) = served("controls", byte_order);
            // Pointer control: the defaults, then a change, then -1 restores.
            client.write_all(&request(byte_order, 106, 0, &[])).unwrap();
            let record = reply(byte_order, &mut client, "GetPointerControl");
            assert_eq!(
                (read_u16(byte_order, &record[8..10]), read_u16(byte_order, &record[10..12]), read_u16(byte_order, &record[12..14])),
                (2, 1, 4),
                "{byte_order:?}: the reference defaults"
            );
            let mut change = Vec::new();
            for value in [7i16, 3, 9] {
                change.extend_from_slice(&match byte_order {
                    XByteOrder::LittleEndian => value.to_le_bytes(),
                    XByteOrder::BigEndian => value.to_be_bytes(),
                });
            }
            change.extend_from_slice(&[1, 1]);
            client.write_all(&request(byte_order, 105, 0, &change)).unwrap();
            client.write_all(&request(byte_order, 106, 0, &[])).unwrap();
            let record = reply(byte_order, &mut client, "GetPointerControl after change");
            assert_eq!(
                (read_u16(byte_order, &record[8..10]), read_u16(byte_order, &record[10..12]), read_u16(byte_order, &record[12..14])),
                (7, 3, 9),
                "{byte_order:?}: what was set is read back"
            );
            // A zero denominator with acceleration asked for is BadValue.
            let mut zero = Vec::new();
            for value in [1i16, 0, 4] {
                zero.extend_from_slice(&match byte_order {
                    XByteOrder::LittleEndian => value.to_le_bytes(),
                    XByteOrder::BigEndian => value.to_be_bytes(),
                });
            }
            zero.extend_from_slice(&[1, 0]);
            client.write_all(&request(byte_order, 105, 0, &zero)).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_VALUE, 105, "zero denominator");

            // Screen saver: defaults, a change, the too-short form.
            client.write_all(&request(byte_order, 108, 0, &[])).unwrap();
            let record = reply(byte_order, &mut client, "GetScreenSaver");
            assert_eq!((read_u16(byte_order, &record[8..10]), read_u16(byte_order, &record[10..12]), record[12], record[13]), (600, 600, 1, 1));
            let mut saver = Vec::new();
            for value in [120i16, 30] {
                saver.extend_from_slice(&match byte_order {
                    XByteOrder::LittleEndian => value.to_le_bytes(),
                    XByteOrder::BigEndian => value.to_be_bytes(),
                });
            }
            saver.extend_from_slice(&[0, 2, 0, 0]);
            client.write_all(&request(byte_order, 107, 0, &saver)).unwrap();
            client.write_all(&request(byte_order, 108, 0, &[])).unwrap();
            let record = reply(byte_order, &mut client, "GetScreenSaver after change");
            assert_eq!((read_u16(byte_order, &record[8..10]), read_u16(byte_order, &record[10..12]), record[12], record[13]), (120, 30, 0, 1), "{byte_order:?}: Default keeps a mode");
            client.write_all(&request(byte_order, 107, 0, &saver[..4])).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_LENGTH, 107, "short SetScreenSaver");
            let mut bad_mode = saver.clone();
            // prefer-blanking sits after the two timings.
            bad_mode[4] = 3;
            client.write_all(&request(byte_order, 107, 0, &bad_mode)).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_VALUE, 107, "mode 3");

            // Keyboard control: bell percent and pitch set and read back; an
            // unused mask bit is BadValue carrying the mask; a led without a
            // mode is BadMatch; a percent past 100 is BadValue.
            client.write_all(&request(byte_order, 102, 0, &words(byte_order, &[0x02 | 0x04, 75, 880]))).unwrap();
            client.write_all(&request(byte_order, 103, 0, &[])).unwrap();
            let record = read_x_record(&mut client);
            assert_eq!(record[0], 1, "{byte_order:?}: GetKeyboardControl replies: {record:?}");
            let mut rest = [0u8; 20];
            fill_from_socket(&mut client, &mut rest);
            assert_eq!((record[13], read_u16(byte_order, &record[14..16])), (75, 880), "{byte_order:?}: what was set is read back");
            assert_eq!(record[1], 1, "{byte_order:?}: global auto-repeat still on");
            client.write_all(&request(byte_order, 102, 0, &words(byte_order, &[0x100, 0]))).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_VALUE, 102, "unused mask bit");
            client.write_all(&request(byte_order, 102, 0, &words(byte_order, &[0x10, 3]))).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_MATCH, 102, "led without a mode");
            client.write_all(&request(byte_order, 102, 0, &words(byte_order, &[0x02, 101]))).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_VALUE, 102, "percent past 100");

            drop(client);
            server.join().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    #[test]
    fn motion_history_is_empty_and_the_host_list_is_locked() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (mut client, socket_path, server) = served("hosts", byte_order);
            client.write_all(&request(byte_order, 39, 0, &words(byte_order, &[X_SETUP_DEFAULT_ROOT, 0, 0]))).unwrap();
            let record = reply(byte_order, &mut client, "GetMotionEvents");
            assert_eq!(read_u32(byte_order, &record[8..12]), 0, "{byte_order:?}: no events");
            assert_eq!(read_u32(byte_order, &record[4..8]), 0, "{byte_order:?}: nothing follows");
            client.write_all(&request(byte_order, 39, 0, &words(byte_order, &[0x7ff0_0001, 0, 0]))).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_WINDOW, 39, "unknown window");
            client.write_all(&request(byte_order, 39, 0, &words(byte_order, &[X_SETUP_DEFAULT_ROOT, 0]))).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_LENGTH, 39, "short GetMotionEvents");

            client.write_all(&request(byte_order, 110, 0, &[])).unwrap();
            let record = reply(byte_order, &mut client, "ListHosts");
            assert_eq!(record[1], 1, "{byte_order:?}: access control enabled");
            assert_eq!(read_u16(byte_order, &record[8..10]), 0, "{byte_order:?}: no hosts");
            assert_eq!(read_u32(byte_order, &record[4..8]), 0, "{byte_order:?}: nothing follows");
            // ChangeHosts: an Internet address, framed exactly; then BadAccess.
            let mut hosts = vec![0u8, 0];
            push_u16(&mut hosts, byte_order, 4);
            hosts.extend_from_slice(&[127, 0, 0, 1]);
            client.write_all(&request(byte_order, 109, 0, &hosts)).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_ACCESS, 109, "ChangeHosts");
            client.write_all(&request(byte_order, 109, 2, &hosts)).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_VALUE, 109, "ChangeHosts mode 2");
            client.write_all(&request(byte_order, 109, 0, &hosts[..4])).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_LENGTH, 109, "short ChangeHosts");
            client.write_all(&request(byte_order, 111, 1, &[])).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_ACCESS, 111, "SetAccessControl");
            client.write_all(&request(byte_order, 111, 2, &[])).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_VALUE, 111, "SetAccessControl mode 2");
            // SetFontPath with no paths, one unit long, is BadLength before
            // the refusal by design.
            client.write_all(&request(byte_order, 51, 0, &words(byte_order, &[0, 0]))).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_LENGTH, 51, "long SetFontPath");
            client.write_all(&request(byte_order, 51, 0, &words(byte_order, &[0]))).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_ACCESS, 51, "SetFontPath by design");

            drop(client);
            server.join().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }
}
