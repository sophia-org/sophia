use super::*;

struct SocketFixture {
    path: std::path::PathBuf,
    server: Option<std::thread::JoinHandle<Result<(), X11SetupSocketError>>>,
}

impl SocketFixture {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("srr-{}-{tag}.sock", std::process::id()));
        let namespace = NamespaceContext::new(
            NS,
            NamespaceProfile::ClassicShared,
            NamespaceCapabilities::NONE,
        )
        .unwrap();
        let policy = std::sync::Arc::new(TestXAdmissionPolicy::new(namespace, false));
        let config = XServerFrontendConfig::new_with_namespace_context(&path, namespace)
            .unwrap()
            .with_admission_policy(policy);
        let mut frontend = XServerFrontend::bind_exclusive(config).unwrap();
        let server = Some(std::thread::spawn(move || frontend.serve_next()));
        Self { path, server }
    }
}

impl Drop for SocketFixture {
    fn drop(&mut self) {
        // The client must be dropped first. Do not hide an original failure
        // with a second panic from a server that observed a broken connection.
        if !std::thread::panicking() {
            self.server.take().unwrap().join().unwrap().unwrap();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

const OUTPUT: u32 = 0x2000_0001;
const NS: NamespaceId = NamespaceId::from_raw(843);

#[test]
fn randr_property_error_range_does_not_overlap_other_extensions() {
    let ranges = [
        (X_INPUT_FIRST_ERROR, 5),
        (X_RENDER_FIRST_ERROR, 5),
        (X_GLX_FIRST_ERROR, X_GLX_ERROR_COUNT),
        (X_RANDR_FIRST_ERROR, X_RANDR_ERROR_COUNT),
    ];
    let mut occupied = std::collections::BTreeSet::new();
    for (base, count) in ranges {
        for code in base..base.checked_add(count).unwrap() {
            assert!(
                occupied.insert(code),
                "extension error ranges must be disjoint"
            );
        }
    }
}

fn property_request(order: XByteOrder, minor: u8, output: u32, atom: Option<u32>) -> Vec<u8> {
    let mut bytes = vec![X_RANDR_MAJOR_OPCODE, minor];
    push_u16(&mut bytes, order, if atom.is_some() { 3 } else { 2 });
    push_u32(&mut bytes, order, output);
    if let Some(atom) = atom {
        push_u32(&mut bytes, order, atom);
    }
    bytes
}

fn answer(order: XByteOrder, runtime: &mut XAuthorityRuntime, bytes: &[u8]) -> Vec<u8> {
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let outputs = match decode_x11_core_request(context(NS, 71, order), bytes) {
        Ok(request) => {
            let result = dispatch_x11_wire_request(
                dispatch_context(NS, 71, order, X_RANDR_MAJOR_OPCODE),
                request,
                runtime,
                &mut atoms,
                &mut properties,
            );
            assert!(result.metadata_candidates.is_empty());
            result.encoded_outputs(order)
        }
        Err(error) => vec![
            encode_x_client_error(
                order,
                x_error_from_wire_parse(&error, 71, bytes[0], bytes[1].into()),
            )
            .to_vec(),
        ],
    };
    assert_eq!(outputs.len(), 1);
    assert_eq!(read_u16(order, &outputs[0][2..4]), 71);
    outputs.into_iter().next().unwrap()
}

#[test]
fn randr_property_listing_metadata_and_value_agree_in_both_orders() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let mut runtime = XAuthorityRuntime::new();
        let topology = runtime.output_topology().clone();
        let atoms = XAtomTable::new();
        let property = atoms.atom(X_ATOM_NAME_RANDR_NON_DESKTOP).unwrap();
        let list = property_request(order, 10, OUTPUT, None);
        for _ in 0..2 {
            let reply = answer(order, &mut runtime, &list);
            assert_eq!(
                reply[0], 1,
                "RANDR property enumeration must return a reply"
            );
            assert_eq!(reply.len(), 36);
            assert_eq!(read_u32(order, &reply[4..8]), 1);
            assert_eq!(read_u16(order, &reply[8..10]), 1);
            assert!(reply[10..32].iter().all(|b| *b == 0));
            assert_eq!(read_u32(order, &reply[32..36]), property);
            let meta = answer(
                order,
                &mut runtime,
                &property_request(order, 11, OUTPUT, Some(property)),
            );
            assert_eq!(meta[0], 1);
            assert_eq!(meta.len(), 40);
            assert_eq!(read_u32(order, &meta[4..8]), 2);
            assert_eq!(&meta[8..11], &[0, 1, 1]);
            assert!(meta[11..32].iter().all(|b| *b == 0));
            assert_eq!(read_u32(order, &meta[32..36]), 0);
            assert_eq!(read_u32(order, &meta[36..40]), 1);
            let get = randr_get_output_property_request(order, OUTPUT, property, 1);
            let value = answer(order, &mut runtime, &get);
            assert_eq!(value[0..2], [1, 32]);
            assert_eq!(read_u32(order, &value[8..12]), X_ATOM_INTEGER);
            assert_eq!(read_u32(order, &value[16..20]), 1);
            assert_eq!(read_u32(order, &value[32..36]), 0);
        }
        assert_eq!(
            runtime.output_topology(),
            &topology,
            "reads cannot change topology"
        );
    }
}

#[test]
fn randr_property_errors_preserve_exact_request_correlation() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let mut runtime = XAuthorityRuntime::new();
        let atoms = XAtomTable::new();
        let property = atoms.atom(X_ATOM_NAME_RANDR_NON_DESKTOP).unwrap();
        let edid = atoms.atom(X_ATOM_NAME_RANDR_EDID).unwrap();
        for (minor, output, atom, expected, bad) in [
            (10, 0, None, X_RANDR_FIRST_ERROR, 0),
            (
                11,
                0xdeadbeef,
                Some(property),
                X_RANDR_FIRST_ERROR,
                0xdeadbeef,
            ),
            (11, OUTPUT, Some(0), 5, 0),
            (11, OUTPUT, Some(u32::MAX), 5, u32::MAX),
            (11, OUTPUT, Some(edid), 15, edid),
        ] {
            let error = answer(
                order,
                &mut runtime,
                &property_request(order, minor, output, atom),
            );
            assert_eq!(error.len(), 32);
            assert_eq!(&error[..2], &[0, expected]);
            assert_eq!(read_u32(order, &error[4..8]), bad);
            assert_eq!(read_u16(order, &error[8..10]), u16::from(minor));
            assert_eq!(error[10], X_RANDR_MAJOR_OPCODE);
        }
        let missing = answer(
            order,
            &mut runtime,
            &randr_get_output_property_request(order, OUTPUT, edid, 1),
        );
        assert_eq!(missing[0..2], [1, 0]);
        assert!(missing[4..].iter().all(|b| *b == 0));
        let mut get = randr_get_output_property_request(order, OUTPUT, property, 1);
        get[24] = 1;
        assert_eq!(
            answer(order, &mut runtime, &get)[0..2],
            [0, 10],
            "immutable deletion is refused"
        );
        get[24] = 0;
        get[25] = 1;
        assert_eq!(answer(order, &mut runtime, &get)[0..2], [1, 32]);
        let invalid_type = match order {
            XByteOrder::LittleEndian => u32::MAX.to_le_bytes(),
            XByteOrder::BigEndian => u32::MAX.to_be_bytes(),
        };
        get[12..16].copy_from_slice(&invalid_type);
        let error = answer(order, &mut runtime, &get);
        assert_eq!(error[0..2], [0, 5]);
        assert_eq!(read_u32(order, &error[4..8]), u32::MAX);
    }
}

#[test]
fn randr_property_reads_bound_offsets_lengths_and_types() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let mut runtime = XAuthorityRuntime::new();
        let property = XAtomTable::new()
            .atom(X_ATOM_NAME_RANDR_NON_DESKTOP)
            .unwrap();
        for (offset, length, requested_type, code, bytes_after, items) in [
            (0, 0, 0, 1, 4, 0),
            (0, u32::MAX, 0, 1, 0, 1),
            (1, 1, 0, 1, 0, 0),
            (2, 1, 0, 0, 0, 0),
            (u32::MAX, 1, 0, 0, 0, 0),
            (0, 1, X_ATOM_CARDINAL, 1, 4, 0),
        ] {
            let mut request = vec![X_RANDR_MAJOR_OPCODE, 15];
            push_u16(&mut request, order, 7);
            for v in [OUTPUT, property, requested_type, offset, length] {
                push_u32(&mut request, order, v);
            }
            request.extend_from_slice(&[0; 4]);
            let reply = answer(order, &mut runtime, &request);
            assert_eq!(reply[0], code);
            if code == 1 {
                assert_eq!(read_u32(order, &reply[12..16]), bytes_after);
                assert_eq!(read_u32(order, &reply[16..20]), items);
                assert_eq!(reply.len(), 32 + items as usize * 4);
            } else {
                assert_eq!(reply[1], 2);
                assert_eq!(read_u32(order, &reply[4..8]), offset);
            }
        }
    }
}

#[test]
fn randr_property_lengths_and_booleans_are_validated() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let mut runtime = XAuthorityRuntime::new();
        for request in [
            property_request(order, 10, OUTPUT, None),
            property_request(order, 11, OUTPUT, Some(19)),
        ] {
            for len in 4..request.len() {
                assert_eq!(answer(order, &mut runtime, &request[..len])[0..2], [0, 16]);
            }
            let mut extra = request.clone();
            extra.extend_from_slice(&[0; 4]);
            let units = (extra.len() / 4) as u16;
            extra[2..4].copy_from_slice(&match order {
                XByteOrder::LittleEndian => units.to_le_bytes(),
                XByteOrder::BigEndian => units.to_be_bytes(),
            });
            assert_eq!(answer(order, &mut runtime, &extra)[0..2], [0, 16]);
        }
        for position in [24, 25] {
            let mut get = randr_get_output_property_request(order, OUTPUT, 19, 1);
            get[position] = 2;
            assert_eq!(answer(order, &mut runtime, &get)[0..2], [0, 2]);
        }
    }
}

#[test]
fn randr_property_requests_revalidate_removed_outputs() {
    let order = XByteOrder::LittleEndian;
    let mut runtime = XAuthorityRuntime::new();
    let mut topology = runtime.output_topology().clone();
    topology.generation += 1;
    topology.outputs[0].output = OutputId::from_raw(2);
    topology.primary = OutputId::from_raw(2);
    runtime.update_output_topology(topology).unwrap();
    for request in [
        property_request(order, 10, OUTPUT, None),
        property_request(order, 11, OUTPUT, Some(19)),
        randr_get_output_property_request(order, OUTPUT, 19, 1),
    ] {
        assert_eq!(
            answer(order, &mut runtime, &request)[0..2],
            [0, X_RANDR_FIRST_ERROR]
        );
    }
    assert_eq!(
        answer(
            order,
            &mut runtime,
            &property_request(order, 10, OUTPUT + 1, None)
        )[0],
        1
    );
}

#[test]
fn randr_property_socket_recovers_after_errors_in_both_orders() {
    use std::io::{Read, Write};
    for (tag, order) in [
        ("le", XByteOrder::LittleEndian),
        ("be", XByteOrder::BigEndian),
    ] {
        let fixture = SocketFixture::new(tag);
        let mut client = connect_x_socket(&fixture.path);
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        client
            .write_all(&setup_request(order, 11, 0, b"", b""))
            .unwrap();
        read_setup_success(&mut client, order);
        let exchange = |client: &mut std::os::unix::net::UnixStream, request: &[u8]| {
            client.write_all(request).unwrap();
            let mut reply = vec![0; 32];
            client.read_exact(&mut reply).unwrap();
            if reply[0] == 1 {
                let size = read_u32(order, &reply[4..8]) as usize * 4;
                assert!(size <= 64);
                reply.resize(32 + size, 0);
                client.read_exact(&mut reply[32..]).unwrap();
            }
            reply
        };
        let ext = exchange(&mut client, &query_extension_request(order, "RANDR"));
        assert_eq!(ext[11], X_RANDR_FIRST_ERROR);
        let listed = exchange(&mut client, &property_request(order, 10, OUTPUT, None));
        assert_eq!(listed[0], 1, "served RANDR enumeration must succeed");
        let atom = read_u32(order, &listed[32..36]);
        let bad = exchange(&mut client, &property_request(order, 10, 0, None));
        assert_eq!(bad[..2], [0, ext[11]]);
        assert_eq!(read_u16(order, &bad[2..4]), 3);
        assert_eq!(read_u16(order, &bad[8..10]), 10);
        let meta = exchange(
            &mut client,
            &property_request(order, 11, OUTPUT, Some(atom)),
        );
        assert_eq!(meta[0], 1);
        assert_eq!(read_u16(order, &meta[2..4]), 4);
        let value = exchange(
            &mut client,
            &randr_get_output_property_request(order, OUTPUT, atom, 1),
        );
        assert_eq!(value[0..2], [1, 32]);
        assert_eq!(read_u32(order, &value[8..12]), X_ATOM_INTEGER);
        assert_eq!(read_u32(order, &value[32..36]), 0);
        drop(client);
    }
}

#[test]
fn randr_properties_independent_xcb_client() {
    use std::process::Command;
    let binary = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("randr-properties-probe-{}", std::process::id()));
    let compiled = Command::new("/usr/bin/cc")
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror"])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/randr_properties.c"
        ))
        .args(["-lxcb", "-lxcb-randr", "-o"])
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let fixture = SocketFixture::new("xcb");
    let result = Command::new(&binary).arg(&fixture.path).output().unwrap();
    assert!(
        result.status.success(),
        "independent RANDR client: {result:?}"
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("error recovery PASS"));
    std::fs::remove_file(binary).unwrap();
}
