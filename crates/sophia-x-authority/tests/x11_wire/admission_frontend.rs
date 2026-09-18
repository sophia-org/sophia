#[cfg(unix)]
struct TestXAdmissionPolicy {
    namespace: NamespaceContext,
    deny: bool,
    next_client: std::sync::atomic::AtomicU64,
    requests: std::sync::Mutex<Vec<XServerFrontendAdmissionRequest>>,
    revoked: std::sync::Mutex<Vec<ClientAdmissionContext>>,
}

#[cfg(unix)]
impl TestXAdmissionPolicy {
    fn new(namespace: NamespaceContext, deny: bool) -> Self {
        Self {
            namespace,
            deny,
            next_client: std::sync::atomic::AtomicU64::new(1),
            requests: std::sync::Mutex::new(Vec::new()),
            revoked: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(unix)]
impl XServerFrontendAdmissionPolicy for TestXAdmissionPolicy {
    fn admit(
        &self,
        request: XServerFrontendAdmissionRequest,
    ) -> Result<ClientAdmissionContext, XServerFrontendAdmissionError> {
        self.requests.lock().unwrap().push(request);
        if self.deny {
            return Err(XServerFrontendAdmissionError::Denied);
        }
        let client = self
            .next_client
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(ClientAdmissionContext::new(
            ClientAdmissionId::from_raw(client),
            self.namespace,
            ClientAuthProvenance::new(ClientAuthenticationMethod::PeerCredentials, 7).unwrap(),
        )
        .unwrap())
    }

    fn revoke(&self, context: ClientAdmissionContext) -> Result<(), XServerFrontendAdmissionError> {
        self.revoked.lock().unwrap().push(context);
        Ok(())
    }
}

#[cfg(unix)]
struct SequencedXAdmissionPolicy {
    namespaces: [NamespaceContext; 2],
    next_client: std::sync::atomic::AtomicU64,
    revoked: std::sync::Mutex<Vec<ClientAdmissionContext>>,
}

#[cfg(unix)]
impl XServerFrontendAdmissionPolicy for SequencedXAdmissionPolicy {
    fn admit(
        &self,
        request: XServerFrontendAdmissionRequest,
    ) -> Result<ClientAdmissionContext, XServerFrontendAdmissionError> {
        let index = self
            .next_client
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let namespace = self
            .namespaces
            .get(usize::try_from(index).map_err(|_| XServerFrontendAdmissionError::Unavailable)?)
            .copied()
            .ok_or(XServerFrontendAdmissionError::Unavailable)?;
        ClientAdmissionContext::new(
            ClientAdmissionId::from_raw(index + 1),
            namespace,
            ClientAuthProvenance::new(request.setup_authentication, 9).unwrap(),
        )
        .ok_or(XServerFrontendAdmissionError::Unavailable)
    }

    fn revoke(&self, context: ClientAdmissionContext) -> Result<(), XServerFrontendAdmissionError> {
        self.revoked.lock().unwrap().push(context);
        Ok(())
    }
}

#[cfg(unix)]
#[test]
fn x_server_frontend_reports_admission_denial_as_x11_setup_failure() {
    use std::io::Write;
    use std::sync::Arc;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x-server-admission-denial-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let namespace = NamespaceContext::new(
        NamespaceId::from_raw(825),
        NamespaceProfile::ClassicShared,
        NamespaceCapabilities::NONE,
    )
    .unwrap();
    let policy = Arc::new(TestXAdmissionPolicy::new(namespace, true));
    let config = XServerFrontendConfig::new_with_namespace_context(&socket_path, namespace)
        .unwrap()
        .with_admission_policy(policy.clone());
    let server = thread::spawn(move || {
        let mut frontend = XServerFrontend::bind(config).unwrap();
        frontend.serve_next().unwrap();
    });

    wait_for_socket(&socket_path);
    let mut client = connect_x_socket(&socket_path);
    client
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    let mut prefix = [0; X_SETUP_REPLY_PREFIX_LEN];
    fill_from_socket(&mut client, &mut prefix);
    assert_eq!(prefix[0], 0);
    let body_len = usize::from(read_u16(XByteOrder::LittleEndian, &prefix[6..8])) * 4;
    let mut body = vec![0; body_len];
    fill_from_socket(&mut client, &mut body);
    assert!(String::from_utf8_lossy(&body).contains("admission failed"));
    drop(client);

    server.join().unwrap();
    let requests = policy.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].setup_authentication,
        ClientAuthenticationMethod::TrustedLocal
    );
    assert!(requests[0].peer_credentials.is_some());
    assert!(policy.revoked.lock().unwrap().is_empty());
    std::fs::remove_file(&socket_path).unwrap();
}

#[cfg(unix)]
#[test]
fn x_server_frontend_revokes_distinct_admissions_for_concurrent_clients() {
    use std::io::Write;
    use std::num::NonZeroUsize;
    use std::sync::Arc;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x-server-admission-concurrency-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let namespace = NamespaceContext::new(
        NamespaceId::from_raw(826),
        NamespaceProfile::ClassicShared,
        NamespaceCapabilities::NONE,
    )
    .unwrap();
    let policy = Arc::new(TestXAdmissionPolicy::new(namespace, false));
    let config = XServerFrontendConfig::new_with_namespace_context(&socket_path, namespace)
        .unwrap()
        .with_admission_policy(policy.clone())
        .with_max_concurrent_clients(NonZeroUsize::new(2).unwrap());
    let server = thread::spawn(move || {
        let mut frontend = XServerFrontend::bind(config).unwrap();
        frontend.serve_next_concurrently().unwrap();
        frontend.serve_next_concurrently().unwrap();
        frontend.wait_for_clients().unwrap();
        frontend.active_client_count()
    });

    wait_for_socket(&socket_path);
    let mut first = connect_x_socket(&socket_path);
    first
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    read_setup_success(&mut first, XByteOrder::LittleEndian);
    let mut second = connect_x_socket(&socket_path);
    second
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    read_setup_success(&mut second, XByteOrder::LittleEndian);

    assert_eq!(policy.requests.lock().unwrap().len(), 2);
    drop(first);
    drop(second);
    assert_eq!(server.join().unwrap(), 0);

    let revoked = policy.revoked.lock().unwrap();
    assert_eq!(revoked.len(), 2);
    assert_ne!(revoked[0].client_id, revoked[1].client_id);
    assert!(revoked.iter().all(|context| context.namespace == namespace));
    std::fs::remove_file(&socket_path).unwrap();
}

#[cfg(unix)]
#[test]
fn x_server_frontend_confined_clients_reject_cross_namespace_window_property_and_selection_access()
{
    use std::io::Write;
    use std::num::NonZeroUsize;
    use std::sync::Arc;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x-server-confined-namespace-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let first_namespace = NamespaceContext::new(
        NamespaceId::from_raw(828),
        NamespaceProfile::Confined,
        NamespaceCapabilities::NONE,
    )
    .unwrap();
    let second_namespace = NamespaceContext::new(
        NamespaceId::from_raw(829),
        NamespaceProfile::Confined,
        NamespaceCapabilities::NONE,
    )
    .unwrap();
    let policy = Arc::new(SequencedXAdmissionPolicy {
        namespaces: [first_namespace, second_namespace],
        next_client: std::sync::atomic::AtomicU64::new(0),
        revoked: std::sync::Mutex::new(Vec::new()),
    });
    let metadata_candidates = Arc::new(std::sync::Mutex::new(0usize));
    let config = XServerFrontendConfig::new_with_namespace_context(&socket_path, first_namespace)
        .unwrap()
        .with_admission_policy(policy.clone())
        .with_max_concurrent_clients(NonZeroUsize::new(2).unwrap());
    let server_metadata_candidates = metadata_candidates.clone();
    let server = thread::spawn(move || {
        let mut frontend = XServerFrontend::bind(config).unwrap();
        let observer: Arc<X11CoreTraceObserver> = Arc::new(move |trace| {
            let mut count = server_metadata_candidates.lock().unwrap();
            *count = count.saturating_add(trace.result.metadata_candidates.len());
            Ok(None)
        });
        frontend
            .serve_next_concurrently_traced(observer.clone())
            .unwrap();
        frontend.serve_next_concurrently_traced(observer).unwrap();
        frontend.wait_for_clients().unwrap();
    });

    wait_for_socket(&socket_path);
    let mut first = connect_x_socket(&socket_path);
    first
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    assert_eq!(
        read_setup_resource_id_base(&mut first, XByteOrder::LittleEndian),
        X_SETUP_DEFAULT_RESOURCE_ID_BASE
    );
    let first_window = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 1;
    first
        .write_all(&create_window_request(
            XByteOrder::LittleEndian,
            first_window,
            0,
            0,
            160,
            90,
        ))
        .unwrap();

    // `second` asks about a window `first` created. Without a barrier the two
    // requests race and the answer is BadWindow -- correct, and about a different
    // question than the access control this test is checking.
    sync_x_connection(&mut first, XByteOrder::LittleEndian, first_window);
    let mut second = connect_x_socket(&socket_path);
    second
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    assert_eq!(
        read_setup_resource_id_base(&mut second, XByteOrder::LittleEndian),
        0x0040_0000
    );
    second
        .write_all(&resource_request(XByteOrder::LittleEndian, 8, first_window))
        .unwrap();
    let mut error = [0; 32];
    fill_from_socket(&mut second, &mut error);
    assert_eq!(error[0], 0);
    assert_eq!(error[1], XErrorCode::BadAccess.wire_code());

    second
        .write_all(&change_window_event_mask_request(
            XByteOrder::LittleEndian,
            first_window,
            (1 << 0) | (1 << 1),
        ))
        .unwrap();
    fill_from_socket(&mut second, &mut error);
    assert_eq!(error[0], 0);
    assert_eq!(error[1], XErrorCode::BadAccess.wire_code());

    second
        .write_all(&change_property_request(
            XByteOrder::LittleEndian,
            XPropertyMode::Replace,
            first_window,
            X_ATOM_WM_NAME,
            X_ATOM_STRING,
            8,
            b"foreign title",
        ))
        .unwrap();
    fill_from_socket(&mut second, &mut error);
    assert_eq!(error[0], 0);
    assert_eq!(error[1], XErrorCode::BadAccess.wire_code());

    second
        .write_all(&set_selection_owner_request(
            XByteOrder::LittleEndian,
            first_window,
            X_ATOM_PRIMARY,
            1,
        ))
        .unwrap();
    fill_from_socket(&mut second, &mut error);
    assert_eq!(error[0], 0);
    assert_eq!(error[1], XErrorCode::BadAccess.wire_code());

    second
        .write_all(&convert_selection_request(
            XByteOrder::LittleEndian,
            first_window,
            X_ATOM_PRIMARY,
            X_ATOM_STRING,
            X_ATOM_WM_NAME,
            2,
        ))
        .unwrap();
    fill_from_socket(&mut second, &mut error);
    assert_eq!(error[0], 31);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &error[20..24]), 0);

    drop(first);
    drop(second);
    server.join().unwrap();
    let revoked = policy.revoked.lock().unwrap();
    assert_eq!(revoked.len(), 2);
    assert!(
        revoked
            .iter()
            .all(|context| context.namespace.profile == NamespaceProfile::Confined)
    );
    assert_ne!(revoked[0].namespace.id, revoked[1].namespace.id);
    assert_eq!(*metadata_candidates.lock().unwrap(), 0);
    std::fs::remove_file(&socket_path).unwrap();
}

#[cfg(unix)]
#[test]
fn x_server_frontend_revokes_admission_after_dispatch_failure() {
    use std::io::Write;
    use std::sync::Arc;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x-server-admission-error-cleanup-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let namespace = NamespaceContext::new(
        NamespaceId::from_raw(827),
        NamespaceProfile::ClassicShared,
        NamespaceCapabilities::NONE,
    )
    .unwrap();
    let policy = Arc::new(TestXAdmissionPolicy::new(namespace, false));
    let config = XServerFrontendConfig::new_with_namespace_context(&socket_path, namespace)
        .unwrap()
        .with_admission_policy(policy.clone());
    let server = thread::spawn(move || {
        let mut frontend = XServerFrontend::bind(config).unwrap();
        let error = frontend
            .serve_next_traced(|_| Err(X11SetupSocketError::new("injected observer failure")))
            .unwrap_err();
        (error.to_string(), frontend.active_client_count())
    });

    wait_for_socket(&socket_path);
    let mut client = connect_x_socket(&socket_path);
    client
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    read_setup_success(&mut client, XByteOrder::LittleEndian);
    client
        .write_all(&intern_atom_request(
            XByteOrder::LittleEndian,
            false,
            "FORCE_OBSERVER_FAILURE",
        ))
        .unwrap();

    let (error, active_clients) = server.join().unwrap();
    assert_eq!(error, "injected observer failure");
    assert_eq!(active_clients, 0);
    assert_eq!(policy.revoked.lock().unwrap().len(), 1);
    drop(client);
    std::fs::remove_file(&socket_path).unwrap();
}

#[cfg(unix)]
#[test]
fn x_server_frontend_assigns_disjoint_setup_resource_ranges_to_clients() {
    use std::io::Write;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x-server-frontend-resource-ranges-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config = XServerFrontendConfig::new(&socket_path, NamespaceId::from_raw(816)).unwrap();
    let server = thread::spawn(move || {
        let mut frontend = XServerFrontend::bind(config).unwrap();
        frontend.serve_next().unwrap();
        frontend.serve_next().unwrap();
    });

    wait_for_socket(&socket_path);
    let mut first = connect_x_socket(&socket_path);
    first
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    let first_base = read_setup_resource_id_base(&mut first, XByteOrder::LittleEndian);
    drop(first);

    let mut second = connect_x_socket(&socket_path);
    second
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    let second_base = read_setup_resource_id_base(&mut second, XByteOrder::LittleEndian);
    drop(second);

    assert_eq!(first_base, X_SETUP_DEFAULT_RESOURCE_ID_BASE);
    assert_eq!(second_base, 0x0040_0000);
    assert_eq!(
        second_base - first_base,
        X_SETUP_DEFAULT_RESOURCE_ID_MASK + 1
    );
    server.join().unwrap();
    std::fs::remove_file(&socket_path).unwrap();
}

#[cfg(unix)]
#[test]
fn x_server_frontend_routes_selection_notify_to_the_requestor_client() {
    use std::io::{Read, Write};
    use std::net::Shutdown;
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const TARGET: u32 = X_ATOM_STRING;
    const PROPERTY: u32 = X_ATOM_WM_NAME;

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x-server-frontend-selection-route-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config = XServerFrontendConfig::new(&socket_path, NamespaceId::from_raw(817))
        .unwrap()
        .with_max_concurrent_clients(NonZeroUsize::new(2).unwrap());
    let (paused_sender, paused_receiver) = mpsc::sync_channel(1);
    let (resume_sender, resume_receiver) = mpsc::sync_channel(1);
    let resume_receiver = Mutex::new(resume_receiver);
    let pause_armed = AtomicBool::new(true);
    let server = thread::spawn(move || {
        let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(4).unwrap());
        let mut frontend = XServerFrontend::bind(config).unwrap();
        let observer: Arc<X11CoreTraceObserver> = Arc::new(move |observation| {
            // Hold the originating dispatch after it exposes SelectionRequest.
            // Peer responses must see the sequence published before that
            // effect, even while the originating thread remains here.
            if observation.major_opcode == 24
                && observation.sequence == 3
                && pause_armed.swap(false, Ordering::AcqRel)
            {
                paused_sender.send(()).unwrap();
                resume_receiver
                    .lock()
                    .unwrap()
                    .recv_timeout(X_RECORD_READ_TIMEOUT)
                    .expect("the selection reader must release the dispatch");
            }
            Ok(None)
        });
        frontend
            .serve_next_concurrently_routed_traced(&broker, observer.clone())
            .unwrap();
        frontend
            .serve_next_concurrently_routed_traced(&broker, observer)
            .unwrap();
        frontend.wait_for_clients().unwrap();
    });

    wait_for_socket(&socket_path);
    let mut owner = connect_x_socket(&socket_path);
    owner
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    let owner_window = read_setup_resource_id_base(&mut owner, XByteOrder::LittleEndian) + 1;
    owner
        .write_all(&create_window_request(
            XByteOrder::LittleEndian,
            owner_window,
            0,
            0,
            160,
            90,
        ))
        .unwrap();
    owner
        .write_all(&set_selection_owner_request(
            XByteOrder::LittleEndian,
            owner_window,
            1,
            10,
        ))
        .unwrap();

    let mut requestor = connect_x_socket(&socket_path);
    requestor
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    let requestor_window =
        read_setup_resource_id_base(&mut requestor, XByteOrder::LittleEndian) + 1;
    requestor
        .write_all(&create_window_request(
            XByteOrder::LittleEndian,
            requestor_window,
            0,
            0,
            160,
            90,
        ))
        .unwrap();
    requestor
        .write_all(&change_window_event_mask_request(
            XByteOrder::LittleEndian,
            requestor_window,
            1 << 22,
        ))
        .unwrap();

    requestor
        .write_all(&convert_selection_request(
            XByteOrder::LittleEndian,
            requestor_window,
            1,
            TARGET,
            PROPERTY,
            10,
        ))
        .unwrap();
    let request = read_x_record(&mut owner);
    paused_receiver
        .recv_timeout(X_RECORD_READ_TIMEOUT)
        .expect("ConvertSelection must reach the dispatch observer");
    assert_eq!(request[0], 30);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &request[2..4]), 2);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &request[8..12]),
        owner_window
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &request[12..16]),
        requestor_window
    );
    assert_eq!(read_u32(XByteOrder::LittleEndian, &request[16..20]), 1);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &request[20..24]),
        TARGET
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &request[24..28]),
        PROPERTY
    );

    let selection_bytes = b"same namespace";
    owner
        .write_all(&change_property_request(
            XByteOrder::LittleEndian,
            XPropertyMode::Replace,
            requestor_window,
            PROPERTY,
            TARGET,
            8,
            selection_bytes,
        ))
        .unwrap();
    let new_value = read_x_record(&mut requestor);
    let property_sequence = read_u16(XByteOrder::LittleEndian, &new_value[2..4]);
    assert_eq!(new_value[0], 28);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &new_value[4..8]),
        requestor_window
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &new_value[8..12]),
        PROPERTY
    );

    owner
        .write_all(&send_selection_notify_request(
            XByteOrder::LittleEndian,
            requestor_window,
            read_u32(XByteOrder::LittleEndian, &request[4..8]),
            read_u32(XByteOrder::LittleEndian, &request[16..20]),
            read_u32(XByteOrder::LittleEndian, &request[20..24]),
            read_u32(XByteOrder::LittleEndian, &request[24..28]),
        ))
        .unwrap();
    let event = read_x_record(&mut requestor);
    let notify_sequence = read_u16(XByteOrder::LittleEndian, &event[2..4]);
    resume_sender.send(()).unwrap();
    assert_eq!(event[0], 31 | 0x80);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &event[8..12]),
        requestor_window
    );
    assert_eq!(read_u32(XByteOrder::LittleEndian, &event[12..16]), 1);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &event[16..20]),
        TARGET
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &event[20..24]),
        PROPERTY
    );

    requestor
        .write_all(&get_property_request(
            XByteOrder::LittleEndian,
            true,
            requestor_window,
            PROPERTY,
            X_PROPERTY_ANY_TYPE,
            0,
            u32::MAX,
        ))
        .unwrap();
    let reply = read_x_reply(&mut requestor, XByteOrder::LittleEndian);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &reply[8..12]),
        TARGET
    );
    assert_eq!(reply[1], 8);
    assert_eq!(&reply[32..32 + selection_bytes.len()], selection_bytes);
    let deleted = read_x_record(&mut requestor);
    assert_eq!(deleted[0], 28);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &deleted[4..8]), requestor_window);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &deleted[8..12]),
        PROPERTY
    );
    assert_eq!(deleted[16], 1);
    requestor
        .write_all(&get_property_request(
            XByteOrder::LittleEndian,
            false,
            requestor_window,
            PROPERTY,
            X_PROPERTY_ANY_TYPE,
            0,
            u32::MAX,
        ))
        .unwrap();
    let missing = read_x_reply(&mut requestor, XByteOrder::LittleEndian);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &missing[8..12]), 0);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &missing[16..20]), 0);

    requestor
        .write_all(&set_selection_owner_request(
            XByteOrder::LittleEndian,
            requestor_window,
            1,
            11,
        ))
        .unwrap();
    let clear = read_x_record(&mut owner);
    assert_eq!(clear[0], 29);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &clear[2..4]), 4);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &clear[8..12]),
        owner_window
    );
    assert_eq!(read_u32(XByteOrder::LittleEndian, &clear[12..16]), 1);

    // Exercise the same selection in the reverse direction without resetting
    // either client. Real PRIMARY workflows switch ownership as users select
    // the return token, so a one-way transfer does not cover this boundary.
    owner
        .set_read_timeout(Some(X_RECORD_READ_TIMEOUT))
        .unwrap();
    requestor
        .set_read_timeout(Some(X_RECORD_READ_TIMEOUT))
        .unwrap();
    owner
        .write_all(&convert_selection_request(
            XByteOrder::LittleEndian,
            owner_window,
            1,
            TARGET,
            PROPERTY,
            12,
        ))
        .unwrap();
    let reverse_request = read_x_record(&mut requestor);
    assert_eq!(reverse_request[0], 30);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &reverse_request[8..12]),
        requestor_window
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &reverse_request[12..16]),
        owner_window
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &reverse_request[20..24]),
        TARGET
    );

    let reverse_bytes = b"reverse same namespace";
    requestor
        .write_all(&change_property_request(
            XByteOrder::LittleEndian,
            XPropertyMode::Replace,
            owner_window,
            PROPERTY,
            TARGET,
            8,
            reverse_bytes,
        ))
        .unwrap();
    requestor
        .write_all(&send_selection_notify_request(
            XByteOrder::LittleEndian,
            owner_window,
            read_u32(XByteOrder::LittleEndian, &reverse_request[4..8]),
            read_u32(XByteOrder::LittleEndian, &reverse_request[16..20]),
            read_u32(XByteOrder::LittleEndian, &reverse_request[20..24]),
            read_u32(XByteOrder::LittleEndian, &reverse_request[24..28]),
        ))
        .unwrap();
    let reverse_event = read_x_record(&mut owner);
    // XLibre ProcSendEvent and yserver both set the SendEvent bit for the
    // recipient. Firefox rejects a selection owner's notify when it is
    // replayed as an ordinary server-generated event, so assert the complete
    // wire byte in both transfer directions.
    assert_eq!(reverse_event[0], 31 | 0x80);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &reverse_event[8..12]),
        owner_window
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &reverse_event[20..24]),
        PROPERTY
    );
    owner
        .write_all(&get_property_request(
            XByteOrder::LittleEndian,
            true,
            owner_window,
            PROPERTY,
            X_PROPERTY_ANY_TYPE,
            0,
            u32::MAX,
        ))
        .unwrap();
    let reverse_reply = read_x_reply(&mut owner, XByteOrder::LittleEndian);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &reverse_reply[8..12]),
        TARGET
    );
    assert_eq!(reverse_reply[1], 8);
    assert_eq!(
        &reverse_reply[32..32 + reverse_bytes.len()],
        reverse_bytes
    );
    owner
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    let mut unexpected = [0; 1];
    assert!(owner.read(&mut unexpected).is_err());
    owner.shutdown(Shutdown::Both).unwrap();
    requestor.shutdown(Shutdown::Both).unwrap();
    server.join().unwrap();
    std::fs::remove_file(&socket_path).unwrap();
    assert_eq!(property_sequence, 3);
    assert_eq!(notify_sequence, 3);
}
