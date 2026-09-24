// The colormap requests on a static visual, over a real socket.
//
// WHAT THESE PROVE. Every colormap request is framed as the protocol frames
// it, so a request one unit long or short is BadLength before it is
// anything else; CopyColormapAndFree makes a new colormap on the source's
// visual (a static visual has no allocations to move) and refuses a reused
// or foreign id as BadIDChoice; ListInstalledColormaps replies the one
// installed colormap, the default. Before t169 one shared minimum length
// let the long and short forms through to the semantic answer, and both
// requests were BadAlloc and BadRequest. XTS5 pAllocColorCells 2,
// pAllocColorPlanes 2, pCopyColormapAndFree 1/2/4/5, pFreeColors 2,
// pInstallColormap 2, pListInstalledColormaps 1/2, pStoreColors 3,
// pStoreNamedColor 3, pUninstallColormap 2.

#[cfg(unix)]
mod colormap_static {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    const BAD_WINDOW: u8 = 3;
    const BAD_ACCESS: u8 = 10;
    const BAD_ALLOC: u8 = 11;
    const BAD_ID_CHOICE: u8 = 14;
    const BAD_LENGTH: u8 = 16;

    fn served(name: &str, byte_order: XByteOrder) -> (std::os::unix::net::UnixStream, std::path::PathBuf, thread::JoinHandle<()>) {
        let socket_path = std::env::temp_dir().join(format!(
            "sophia-x-colormap-static-{name}-{}-{}.sock",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let namespace = NamespaceContext::new(
            NamespaceId::from_raw(1169),
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
        // An answer that never comes must fail the test, not hang it: on
        // the decoders before t169 a request one unit long was accepted
        // in silence.
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        (client, socket_path, server)
    }

    /// A request of `opcode` with `words` four-byte fields after the header.
    fn request(byte_order: XByteOrder, opcode: u8, detail: u8, words: &[u32]) -> Vec<u8> {
        let mut out = vec![opcode, detail];
        push_u16(&mut out, byte_order, 1 + words.len() as u16);
        for word in words {
            push_u32(&mut out, byte_order, *word);
        }
        out
    }

    fn expect_error(byte_order: XByteOrder, record: &[u8; 32], code: u8, opcode: u8, what: &str) {
        assert_eq!(
            (record[0], record[1], record[10]),
            (0, code, opcode),
            "{byte_order:?}: {what}: {record:?}"
        );
    }

    #[test]
    fn a_colormap_request_one_unit_off_is_bad_length_before_anything_else() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (mut client, socket_path, server) = served("framing", byte_order);
            let default = X_SETUP_DEFAULT_COLORMAP;
            // One unit long: the shared minimum used to let these through
            // to BadAlloc, BadAccess, BadColor or silence.
            let long: [(u8, &str, Vec<u32>); 8] = [
                (81, "InstallColormap", vec![default, 0]),
                (82, "UninstallColormap", vec![default, 0]),
                (83, "ListInstalledColormaps", vec![X_SETUP_DEFAULT_ROOT, 0]),
                (86, "AllocColorCells", vec![default, 0, 0]),
                (87, "AllocColorPlanes", vec![default, 0, 0, 0]),
                (80, "CopyColormapAndFree", vec![X_SETUP_DEFAULT_RESOURCE_ID_BASE + 1, default, 0]),
                (89, "StoreColors", vec![default, 0]),
                (90, "StoreNamedColor", vec![default, 0, 0, 0]),
            ];
            for (opcode, name, words) in long {
                client.write_all(&request(byte_order, opcode, 0, &words)).unwrap();
                let record = read_x_record(&mut client);
                expect_error(byte_order, &record, BAD_LENGTH, opcode, name);
            }
            // One unit short.
            client.write_all(&request(byte_order, 88, 0, &[default])).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_LENGTH, 88, "FreeColors");
            client.write_all(&request(byte_order, 86, 0, &[default])).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_LENGTH, 86, "AllocColorCells short");
            // Framed right, the static answers stand: cells and planes
            // cannot be allocated, read-only cells cannot be stored into,
            // and freeing what was never allocated is not an error.
            client.write_all(&request(byte_order, 86, 0, &[default, 1])).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_ALLOC, 86, "AllocColorCells");
            client.write_all(&request(byte_order, 89, 0, &[default, 0, 0, 0])).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_ACCESS, 89, "StoreColors");
            client.write_all(&request(byte_order, 88, 0, &[default, 0, 1])).unwrap();
            client.write_all(&request(byte_order, 43, 0, &[])).unwrap();
            let record = read_x_record(&mut client);
            assert_eq!(record[0], 1, "{byte_order:?}: FreeColors answered nothing; GetInputFocus replied");
            drop(client);
            server.join().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    #[test]
    fn copy_colormap_and_free_creates_on_the_sources_visual_and_installed_is_the_default() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (mut client, socket_path, server) = served("copy", byte_order);
            let default = X_SETUP_DEFAULT_COLORMAP;
            let copy = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 1;
            // Sequence 1: the copy answers nothing. Sequence 2: it is a
            // colormap AllocColor accepts.
            client.write_all(&request(byte_order, 80, 0, &[copy, default])).unwrap();
            let mut alloc = request(byte_order, 84, 0, &[copy]);
            for component in [0x8000u16, 0x4000, 0x2000] {
                push_u16(&mut alloc, byte_order, component);
            }
            push_u16(&mut alloc, byte_order, 0);
            alloc[2..4].copy_from_slice(&match byte_order {
                XByteOrder::LittleEndian => 4u16.to_le_bytes(),
                XByteOrder::BigEndian => 4u16.to_be_bytes(),
            });
            client.write_all(&alloc).unwrap();
            let record = read_x_record(&mut client);
            assert_eq!(record[0], 1, "{byte_order:?}: the copied colormap allocates: {record:?}");
            assert_eq!(read_u16(byte_order, &record[2..4]), 2);
            // The same id again is BadIDChoice; an id outside the range too.
            client.write_all(&request(byte_order, 80, 0, &[copy, default])).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_ID_CHOICE, 80, "reused id");
            client.write_all(&request(byte_order, 80, 0, &[0x7ff0_0001, default])).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_ID_CHOICE, 80, "foreign id");
            // ListInstalledColormaps on the root: exactly the default.
            client.write_all(&request(byte_order, 83, 0, &[X_SETUP_DEFAULT_ROOT])).unwrap();
            let record = read_x_record(&mut client);
            assert_eq!(record[0], 1, "{byte_order:?}: ListInstalledColormaps replies: {record:?}");
            assert_eq!(read_u32(byte_order, &record[4..8]), 1, "{byte_order:?}: one list word");
            assert_eq!(read_u16(byte_order, &record[8..10]), 1, "{byte_order:?}: one colormap");
            let mut list = [0u8; 4];
            fill_from_socket(&mut client, &mut list);
            assert_eq!(read_u32(byte_order, &list), default, "{byte_order:?}: the default is the installed one");
            client.write_all(&request(byte_order, 83, 0, &[0x7ff0_0001])).unwrap();
            expect_error(byte_order, &read_x_record(&mut client), BAD_WINDOW, 83, "unknown window");
            drop(client);
            server.join().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }
}
