// The frontend's transport: transaction emitter batches and backpressure,
// the route broker, the socket configuration and cookie, DRI3 descriptors
// over the socket. Included from x11_wire.rs beside resources_frontend.rs
// (t026).

#[test]
fn x11_dispatch_sophia_present_emits_xpixmap_surface_transaction() {
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let create = decode_x11_core_request(
        context(namespace, 621, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, 0x220121, 10, 20, 640, 480),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let present = decode_x11_core_request(
        context(namespace, 622, XByteOrder::LittleEndian),
        &sophia_present_pixmap_request(
            XByteOrder::LittleEndian,
            0x220121,
            0x990,
            (3, 5, 32, 24),
            1,
            250,
        ),
    )
    .unwrap();
    let present = dispatch_x11_wire_request(
        dispatch_context(
            namespace,
            2,
            XByteOrder::LittleEndian,
            X_SOPHIA_PRESENT_MAJOR_OPCODE,
        ),
        present,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    assert!(present.outputs.is_empty());
    let response = present.response.unwrap();
    assert_eq!(response.transactions.len(), 1);
    assert_eq!(
        response.transactions[0].surface,
        SurfaceId::new(0x220121, 1)
    );
    assert_eq!(
        response.transactions[0].target_buffer(),
        BufferSource::XPixmap { pixmap: 0x990 }
    );
    assert_eq!(
        response.transactions[0].damage,
        Region::single(Rect {
            x: 3,
            y: 5,
            width: 32,
            height: 24,
        })
    );
}

#[test]
fn x_authority_transaction_emitter_sends_bounded_batches() {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let result = present_dispatch_result(TransactionId::from_raw(610));

    let emitted = try_emit_x_authority_transactions(&sender, &result)
        .unwrap()
        .unwrap();
    let received = receiver.try_recv().unwrap();

    assert_eq!(emitted.transaction, TransactionId::from_raw(610));
    assert_eq!(emitted.transactions.len(), 1);
    assert_eq!(received, emitted);
}

#[test]
fn x_authority_transaction_emitter_reports_backpressure() {
    let (sender, _receiver) = std::sync::mpsc::sync_channel(0);
    let result = present_dispatch_result(TransactionId::from_raw(611));

    assert_eq!(
        try_emit_x_authority_transactions(&sender, &result),
        Err(XAuthorityTransportError::Backpressure {
            transaction: TransactionId::from_raw(611)
        })
    );
}
#[test]
fn protocol_router_remains_usable_after_route_broker_moves_or_drops() {
    use std::num::NonZeroUsize;

    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(4).unwrap());
    let router = broker.protocol_router();
    let second = router.clone();
    drop(broker);

    assert_eq!(
        router.route_present_complete(
            TransactionId::from_raw(91),
            10,
            20,
            XPresentCompletionMode::Flip,
        ),
        Ok(false)
    );
    assert_eq!(
        second.route_present_idle(TransactionId::from_raw(91)),
        Ok(false)
    );
}

#[cfg(unix)]
#[test]
fn x_server_frontend_config_requires_a_socket_path_and_namespace() {
    assert!(XServerFrontendConfig::new("", NamespaceId::from_raw(1)).is_err());
    assert!(XServerFrontendConfig::new("/tmp/sophia-x11.sock", NamespaceId::INVALID).is_err());

    let config =
        XServerFrontendConfig::new("/tmp/sophia-x11.sock", NamespaceId::from_raw(812)).unwrap();
    assert_eq!(
        config.socket_path(),
        std::path::Path::new("/tmp/sophia-x11.sock")
    );
    assert_eq!(config.namespace(), NamespaceId::from_raw(812));
    assert_eq!(
        config.namespace_context().profile,
        NamespaceProfile::ClassicShared
    );
    assert_eq!(config.max_concurrent_clients().get(), 16);
}

#[cfg(unix)]
#[test]
fn x_server_frontend_config_accepts_a_session_namespace_context() {
    let namespace = NamespaceContext::new(
        NamespaceId::from_raw(821),
        NamespaceProfile::Confined,
        NamespaceCapabilities::NONE
            .with_request(NamespacePortalCapability::Clipboard)
            .with_publish(NamespacePortalCapability::Clipboard),
    )
    .unwrap();

    let config = XServerFrontendConfig::new_with_namespace_context(
        "/tmp/sophia-x11-confined.sock",
        namespace,
    )
    .unwrap();

    assert_eq!(config.namespace(), namespace.id);
    assert_eq!(config.namespace_context(), namespace);
}

#[cfg(unix)]
#[test]
fn x_server_frontend_dri3_open_sends_backend_owned_render_device_fd() {
    use std::fs::File;
    use std::io::{IoSliceMut, Write};
    use std::mem::MaybeUninit;
    use std::os::fd::OwnedFd;
    use std::sync::Arc;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestRenderDeviceProvider;

    impl XServerFrontendRenderDeviceProvider for TestRenderDeviceProvider {
        fn open_render_device_fd(&self) -> Result<OwnedFd, XServerFrontendRenderDeviceError> {
            File::open("/dev/null")
                .map(OwnedFd::from)
                .map_err(|_| XServerFrontendRenderDeviceError::Unavailable)
        }
    }

    let path = std::env::temp_dir().join(format!(
        "sophia-x-server-dri3-open-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config = XServerFrontendConfig::new(&path, NamespaceId::from_raw(822))
        .unwrap()
        .with_render_device_provider(Arc::new(TestRenderDeviceProvider));
    let mut frontend = XServerFrontend::bind(config).unwrap();
    let server = thread::spawn(move || frontend.serve_next());

    wait_for_socket(&path);
    let mut stream = connect_x_socket(&path);
    stream
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    read_setup_success(&mut stream, XByteOrder::LittleEndian);
    stream
        .write_all(&dri3_open_request(
            XByteOrder::LittleEndian,
            X_SETUP_DEFAULT_ROOT,
            0,
        ))
        .unwrap();

    let mut reply = [0; X_CLIENT_OUTPUT_RECORD_LEN];
    let mut iov = [IoSliceMut::new(&mut reply)];
    let mut ancillary_space = [MaybeUninit::uninit();
        rustix::cmsg_space!(ScmRights(sophia_protocol::DMA_BUF_MAX_PLANES))];
    let mut ancillary = rustix::net::RecvAncillaryBuffer::new(&mut ancillary_space);
    let received = rustix::net::recvmsg(
        &stream,
        &mut iov,
        &mut ancillary,
        rustix::net::RecvFlags::CMSG_CLOEXEC,
    )
    .unwrap();
    assert_eq!(received.bytes, X_CLIENT_OUTPUT_RECORD_LEN);
    assert_eq!(reply[0], 1);
    assert_eq!(reply[1], 1);
    let received_fds = ancillary
        .drain()
        .flat_map(|message| match message {
            rustix::net::RecvAncillaryMessage::ScmRights(fds) => fds.collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect::<Vec<_>>();
    assert_eq!(received_fds.len(), 1);
    File::from(received_fds.into_iter().next().unwrap())
        .metadata()
        .unwrap();

    drop(stream);
    server.join().unwrap().unwrap();
    std::fs::remove_file(path).unwrap();
}

#[cfg(unix)]
#[test]
fn x_server_frontend_assigns_batched_scm_rights_to_fd_bearing_requests() {
    use std::fs::File;
    use std::io::{IoSlice, Write};
    use std::mem::MaybeUninit;
    use std::net::Shutdown;
    use std::os::fd::AsFd;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let path = std::env::temp_dir().join(format!(
        "sophia-x-server-batched-rights-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config = XServerFrontendConfig::new(&path, NamespaceId::from_raw(823)).unwrap();
    let mut frontend = XServerFrontend::bind(config).unwrap();
    let server = thread::spawn(move || frontend.serve_next());

    wait_for_socket(&path);
    let mut stream = connect_x_socket(&path);
    stream
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    read_setup_success(&mut stream, XByteOrder::LittleEndian);

    let mut requests = xfixes_create_region_request(XByteOrder::LittleEndian, 0x220810, &[]);
    requests.extend_from_slice(&dri3_pixmap_from_buffer_request(
        XByteOrder::LittleEndian,
        0x220811,
        X_SETUP_DEFAULT_ROOT,
        64 * 48 * 4,
        64,
        48,
        256,
        24,
        32,
    ));
    requests.extend_from_slice(&dri3_fence_from_fd_request(
        XByteOrder::LittleEndian,
        X_SETUP_DEFAULT_ROOT,
        0x220812,
        false,
    ));
    let pixmap_fd = File::open("/dev/null").unwrap();
    let fence_fd = File::open("/dev/null").unwrap();
    let borrowed = [pixmap_fd.as_fd(), fence_fd.as_fd()];
    let mut space = [MaybeUninit::uninit();
        rustix::cmsg_space!(ScmRights(sophia_protocol::DMA_BUF_MAX_PLANES))];
    let mut ancillary = rustix::net::SendAncillaryBuffer::new(&mut space);
    assert!(ancillary.push(rustix::net::SendAncillaryMessage::ScmRights(&borrowed)));
    let sent = rustix::net::sendmsg(
        &stream,
        &[IoSlice::new(&requests)],
        &mut ancillary,
        rustix::net::SendFlags::empty(),
    )
    .unwrap();
    assert_eq!(sent, requests.len());
    stream.shutdown(Shutdown::Write).unwrap();

    server.join().unwrap().unwrap();
    std::fs::remove_file(path).unwrap();
}

#[cfg(unix)]
#[test]
fn declared_zero_dri3_buffer_size_is_taken_from_the_descriptor_and_still_bounded() {
    use std::fs::OpenOptions;
    use std::io::{IoSlice, Read, Seek, SeekFrom, Write};
    use std::mem::MaybeUninit;
    use std::net::Shutdown;
    use std::os::fd::AsFd;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    // A tiled ARGB buffer: its rows need stride * height bytes, and its
    // allocation is larger than that.
    const WIDTH: u16 = 640;
    const HEIGHT: u16 = 360;
    const STRIDE: u16 = 3072;
    const DEPTH: u8 = 32;
    const ROW_BYTES: u32 = STRIDE as u32 * HEIGHT as u32;
    const ALLOCATION: u32 = 1_572_864;
    const PIXMAP: u32 = 0x220811;
    // The descriptor is sent with a non-zero offset, which resolving the size
    // must leave exactly where it was.
    const OFFSET: u64 = 17;

    fn unique(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "sophia-x-dri3-size-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn get_geometry_request(drawable: u32) -> Vec<u8> {
        let mut out = vec![0u8; 8];
        out[0] = 14;
        out[2..4].copy_from_slice(&2u16.to_le_bytes());
        out[4..8].copy_from_slice(&drawable.to_le_bytes());
        out
    }

    // Drives one import over a real socket with a real SCM_RIGHTS descriptor.
    // An admitted import is proven by querying the pixmap back, not by the
    // absence of an error.
    fn import(declared_size: u32, backing_bytes: u64, expected_admitted: bool, tag: &str) {
        let socket = unique(&format!("{tag}.sock"));
        let config = XServerFrontendConfig::new(&socket, NamespaceId::from_raw(823)).unwrap();
        let mut frontend = XServerFrontend::bind(config).unwrap();
        let server = thread::spawn(move || frontend.serve_next());

        wait_for_socket(&socket);
        let mut stream = connect_x_socket(&socket);
        stream
            .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
            .unwrap();
        read_setup_success(&mut stream, XByteOrder::LittleEndian);

        let backing_path = unique(&format!("{tag}.buffer"));
        let mut backing = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&backing_path)
            .unwrap();
        backing.set_len(backing_bytes).unwrap();
        backing.seek(SeekFrom::Start(OFFSET)).unwrap();

        let mut requests = dri3_pixmap_from_buffer_request(
            XByteOrder::LittleEndian,
            PIXMAP,
            X_SETUP_DEFAULT_ROOT,
            declared_size,
            WIDTH,
            HEIGHT,
            STRIDE,
            DEPTH,
            32,
        );
        if expected_admitted {
            requests.extend_from_slice(&get_geometry_request(PIXMAP));
        }

        let borrowed = [backing.as_fd()];
        let mut space = [MaybeUninit::uninit();
            rustix::cmsg_space!(ScmRights(sophia_protocol::DMA_BUF_MAX_PLANES))];
        let mut ancillary = rustix::net::SendAncillaryBuffer::new(&mut space);
        assert!(ancillary.push(rustix::net::SendAncillaryMessage::ScmRights(&borrowed)));
        let sent = rustix::net::sendmsg(
            &stream,
            &[IoSlice::new(&requests)],
            &mut ancillary,
            rustix::net::SendFlags::empty(),
        )
        .unwrap();
        assert_eq!(sent, requests.len());
        stream.shutdown(Shutdown::Write).unwrap();

        let mut answer = Vec::new();
        stream.read_to_end(&mut answer).unwrap();
        server.join().unwrap().unwrap();

        assert_eq!(
            answer.len() % 32,
            0,
            "{tag}: answer must be whole 32-byte records, got {} bytes",
            answer.len(),
        );
        let records = answer.chunks_exact(32).collect::<Vec<_>>();
        let errors = records
            .iter()
            .filter(|record| record[0] == 0)
            .map(|record| record[1])
            .collect::<Vec<_>>();
        let replies = records
            .iter()
            .filter(|record| record[0] == 1)
            .collect::<Vec<_>>();

        if expected_admitted {
            assert_eq!(errors, Vec::<u8>::new(), "{tag}: import must be admitted");
            assert_eq!(replies.len(), 1, "{tag}: the pixmap must answer a query");
            let reply = replies[0];
            assert_eq!(reply[1], DEPTH, "{tag}: queried depth");
            assert_eq!(
                u16::from_le_bytes([reply[16], reply[17]]),
                WIDTH,
                "{tag}: queried width",
            );
            assert_eq!(
                u16::from_le_bytes([reply[18], reply[19]]),
                HEIGHT,
                "{tag}: queried height",
            );
        } else {
            assert_eq!(
                errors,
                vec![XErrorCode::BadWindow.wire_code()],
                "{tag}: import must be refused",
            );
            assert!(
                replies.is_empty(),
                "{tag}: a refused import answers nothing"
            );
        }

        assert_eq!(
            backing.stream_position().unwrap(),
            OFFSET,
            "{tag}: resolving the size must not move the offset shared with the client",
        );

        std::fs::remove_file(socket).unwrap();
        std::fs::remove_file(backing_path).unwrap();
    }

    // A zero declared size is answered by the descriptor.
    import(0, u64::from(ALLOCATION), true, "zero-ok");
    // The bound still holds against the descriptor's own size.
    import(0, u64::from(ROW_BYTES) - 1, false, "zero-short");
    // A descriptor that reports no size leaves the zero standing.
    import(0, 0, false, "zero-empty");
    // An explicit claim is taken as given, and an undersized one is refused
    // even though the descriptor would have covered the rows.
    import(ROW_BYTES - 1, u64::from(ALLOCATION), false, "nonzero-short");
    // An explicit adequate claim is unaffected.
    import(ALLOCATION, u64::from(ALLOCATION), true, "nonzero-ok");
}

#[cfg(unix)]
#[test]
fn x_server_frontend_binds_an_owner_only_socket_and_preserves_regular_files() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    let path = std::env::temp_dir().join(format!(
        "sophia-x-server-frontend-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config = XServerFrontendConfig::new(&path, NamespaceId::from_raw(813)).unwrap();
    let frontend = XServerFrontend::bind(config).unwrap();
    assert_eq!(frontend.config().socket_path(), path.as_path());
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    drop(frontend);
    std::fs::remove_file(&path).unwrap();

    std::fs::write(&path, b"do not replace regular files").unwrap();
    let config = XServerFrontendConfig::new(&path, NamespaceId::from_raw(814)).unwrap();
    let error = match XServerFrontend::bind(config) {
        Ok(_) => panic!("frontend must not replace a regular file"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("refusing to replace non-socket"));
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"do not replace regular files"
    );
    std::fs::remove_file(&path).unwrap();
}

#[cfg(unix)]
#[test]
fn x_server_frontend_rejects_bad_cookie_then_accepts_the_configured_cookie() {
    use std::io::Write;
    use std::sync::Arc;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x-server-frontend-cookie-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let cookie = [0x3c; 16];
    let namespace = NamespaceContext::new(
        NamespaceId::from_raw(815),
        NamespaceProfile::ClassicShared,
        NamespaceCapabilities::NONE,
    )
    .unwrap();
    let policy = Arc::new(TestXAdmissionPolicy::new(namespace, false));
    let config = XServerFrontendConfig::new_with_namespace_context(&socket_path, namespace)
        .unwrap()
        .with_setup_authorization(XServerFrontendSetupAuthorization::MitMagicCookie(cookie))
        .with_admission_policy(policy.clone());
    assert_eq!(
        format!("{:?}", config.setup_authorization()),
        "MitMagicCookie([redacted])"
    );
    let server = thread::spawn(move || {
        let mut frontend = XServerFrontend::bind(config).unwrap();
        frontend.serve_next().unwrap();
        frontend.serve_next().unwrap();
    });

    wait_for_socket(&socket_path);
    let mut rejected = connect_x_socket(&socket_path);
    rejected
        .write_all(&setup_request(
            XByteOrder::LittleEndian,
            11,
            0,
            b"MIT-MAGIC-COOKIE-1",
            b"wrong-cookie-data",
        ))
        .unwrap();
    let mut rejected_prefix = [0; X_SETUP_REPLY_PREFIX_LEN];
    fill_from_socket(&mut rejected, &mut rejected_prefix);
    assert_eq!(rejected_prefix[0], 0);
    let rejected_body_len =
        usize::from(read_u16(XByteOrder::LittleEndian, &rejected_prefix[6..8])) * 4;
    let mut rejected_body = vec![0; rejected_body_len];
    fill_from_socket(&mut rejected, &mut rejected_body);
    assert!(String::from_utf8_lossy(&rejected_body).contains("authorization failed"));
    drop(rejected);

    let mut accepted = connect_x_socket(&socket_path);
    accepted
        .write_all(&setup_request(
            XByteOrder::LittleEndian,
            11,
            0,
            b"MIT-MAGIC-COOKIE-1",
            &cookie,
        ))
        .unwrap();
    read_setup_success(&mut accepted, XByteOrder::LittleEndian);
    drop(accepted);

    server.join().unwrap();
    let requests = policy.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].setup_authentication,
        ClientAuthenticationMethod::MitMagicCookie1
    );
    assert_eq!(policy.revoked.lock().unwrap().len(), 1);
    std::fs::remove_file(&socket_path).unwrap();
}
