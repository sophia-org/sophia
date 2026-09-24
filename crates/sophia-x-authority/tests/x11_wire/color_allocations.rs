// Per-client, per-channel TrueColor references over the wire. These checks
// distinguish channel accounting from a set of whole pixels, and prove that
// failed FreeColors still releases valid components in the same request.
#[cfg(unix)]
mod color_allocations {
    use super::*;
    use std::{
        io::Write,
        num::NonZeroUsize,
        sync::Arc,
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    fn request(order: XByteOrder, opcode: u8, words: &[u32]) -> Vec<u8> {
        let mut bytes = vec![opcode, 0];
        push_u16(&mut bytes, order, 1 + words.len() as u16);
        for word in words {
            push_u32(&mut bytes, order, *word);
        }
        bytes
    }

    fn alloc(client: &mut std::os::unix::net::UnixStream, order: XByteOrder, map: u32, rgb: u32) {
        let mut bytes = vec![84, 0];
        push_u16(&mut bytes, order, 4);
        push_u32(&mut bytes, order, map);
        for shift in [16, 8, 0] {
            push_u16(&mut bytes, order, u16::from((rgb >> shift) as u8) * 257);
        }
        push_u16(&mut bytes, order, 0);
        client.write_all(&bytes).unwrap();
        let record = read_x_record(client);
        assert_eq!(record[0], 1, "allocation: {record:?}");
        assert_eq!(read_u32(order, &record[16..20]) & 0x00ff_ffff, rgb);
    }

    fn free(
        client: &mut std::os::unix::net::UnixStream,
        order: XByteOrder,
        map: u32,
        mask: u32,
        pixels: &[u32],
        error: Option<u8>,
    ) {
        let mut words = vec![map, mask];
        words.extend_from_slice(pixels);
        client.write_all(&request(order, 88, &words)).unwrap();
        client.write_all(&request(order, 43, &[])).unwrap();
        if let Some(error) = error {
            let record = read_x_record(client);
            assert_eq!(
                (record[0], record[1], record[10]),
                (0, error, 88),
                "{record:?}"
            );
        }
        assert_eq!(
            read_x_record(client)[0],
            1,
            "GetInputFocus fences FreeColors"
        );
    }

    #[test]
    fn color_references_are_per_client_per_channel_counted_and_moved_by_copy() {
        for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let path = std::env::temp_dir().join(format!(
                "sophia-color-refs-{}-{}.sock",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let namespace = NamespaceContext::new(
                NamespaceId::from_raw(212),
                NamespaceProfile::ClassicShared,
                NamespaceCapabilities::NONE,
            )
            .unwrap();
            let config = XServerFrontendConfig::new_with_namespace_context(&path, namespace)
                .unwrap()
                .with_admission_policy(Arc::new(TestXAdmissionPolicy::new(namespace, false)))
                .with_max_concurrent_clients(NonZeroUsize::new(2).unwrap());
            let server = thread::spawn(move || {
                let mut frontend = XServerFrontend::bind(config).unwrap();
                let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(64).unwrap());
                for _ in 0..2 {
                    frontend.serve_next_concurrently_routed(&broker).unwrap();
                }
                frontend.wait_for_clients().unwrap();
            });
            wait_for_socket(&path);
            let connect = || {
                let mut client = connect_x_socket(&path);
                client
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                client
                    .write_all(&setup_request(order, 11, 0, b"", b""))
                    .unwrap();
                let base = read_setup_resource_id_base(&mut client, order);
                (client, base)
            };
            let (mut a, base) = connect();
            let (mut b, _) = connect();
            let map = X_SETUP_DEFAULT_COLORMAP;
            alloc(&mut a, order, map, 0x123456);
            free(&mut b, order, map, 0, &[0x123456], Some(10));
            alloc(&mut a, order, map, 0x123456);
            free(&mut a, order, map, 0, &[0x123456, 0x123456], None);
            free(&mut a, order, map, 0, &[0x123456], Some(10));

            // No allocation of either recombined pixel was made. Each
            // component nevertheless has one reference to release.
            alloc(&mut a, order, map, 0x123456);
            alloc(&mut a, order, map, 0x789abc);
            free(&mut a, order, map, 0, &[0x129abc, 0x783456], None);

            alloc(&mut a, order, map, 0x123456);
            free(&mut a, order, map, 0, &[0xff3456], Some(10));
            // Green and blue were released despite the bad red entry.
            free(&mut a, order, map, 0, &[0x123456], Some(10));

            alloc(&mut a, order, map, 0x123456);
            alloc(&mut b, order, map, 0x123456);
            let copied = base + 1;
            a.write_all(&request(order, 80, &[copied, map])).unwrap();
            free(&mut a, order, map, 0, &[0x123456], Some(10));
            free(&mut a, order, copied, 0, &[0x123456], None);
            free(&mut b, order, map, 0, &[0x123456], None);

            // A plane bit expands its channel only; other channels are
            // released once, not once per whole-pixel combination.
            alloc(&mut a, order, map, 0x123456);
            alloc(&mut a, order, map, 0x133456);
            free(&mut a, order, map, 0x010000, &[0x123456], None);
            free(&mut a, order, map, 0, &[0x123456], Some(10));
            free(&mut a, order, map, u32::MAX, &[], None);
            free(&mut a, order, map, 0, &[0x01000000], Some(2));

            // LookupColor computes a value without allocating it;
            // AllocNamedColor creates the same references as AllocColor.
            for opcode in [92, 85] {
                let mut name = vec![opcode, 0];
                push_u16(&mut name, order, 4);
                push_u32(&mut name, order, map);
                push_u16(&mut name, order, 3);
                push_u16(&mut name, order, 0);
                name.extend_from_slice(b"red\0");
                a.write_all(&name).unwrap();
                assert_eq!(read_x_record(&mut a)[0], 1);
                free(
                    &mut a,
                    order,
                    map,
                    0,
                    &[0xff0000],
                    (opcode == 92).then_some(10),
                );
            }

            let argb_map = base + 2;
            a.write_all(&request(
                order,
                78,
                &[argb_map, X_SETUP_DEFAULT_ROOT, X_SETUP_ARGB_VISUAL],
            ))
            .unwrap();
            alloc(&mut a, order, argb_map, 0x123456);
            free(&mut a, order, argb_map, 0xff000000, &[0xff123456], None);
            drop(a);
            drop(b);
            server.join().unwrap();
            let _ = std::fs::remove_file(path);
        }
    }
}
