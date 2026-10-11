#[cfg(unix)]
struct ListenerXAdmissionPolicy {
    namespaces: std::collections::BTreeMap<XServerFrontendListenerId, NamespaceContext>,
    next_client: std::sync::atomic::AtomicU64,
    requests: std::sync::Mutex<Vec<XServerFrontendAdmissionRequest>>,
    revoked: std::sync::Mutex<Vec<ClientAdmissionContext>>,
}

#[cfg(unix)]
impl ListenerXAdmissionPolicy {
    fn new(primary: NamespaceContext, additional: NamespaceContext) -> Self {
        Self {
            namespaces: [
                (XServerFrontendListenerId::PRIMARY, primary),
                (XServerFrontendListenerId(7), additional),
            ]
            .into(),
            next_client: std::sync::atomic::AtomicU64::new(1),
            requests: std::sync::Mutex::new(Vec::new()),
            revoked: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(unix)]
impl XServerFrontendAdmissionPolicy for ListenerXAdmissionPolicy {
    fn admit(
        &self,
        request: XServerFrontendAdmissionRequest,
    ) -> Result<ClientAdmissionContext, XServerFrontendAdmissionError> {
        let namespace = *self
            .namespaces
            .get(&request.listener)
            .ok_or(XServerFrontendAdmissionError::Denied)?;
        self.requests.lock().unwrap().push(request);
        let id = self
            .next_client
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ClientAdmissionContext::new(
            ClientAdmissionId::from_raw(id),
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
fn listener_group_paths() -> (std::path::PathBuf, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "sxg-{}-{}.sock",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let other = path.with_extension("group");
    (path, other)
}

#[cfg(unix)]
fn listener_group_namespaces() -> (NamespaceContext, NamespaceContext) {
    (
        NamespaceContext::new(
            NamespaceId::from_raw(910),
            NamespaceProfile::ClassicShared,
            NamespaceCapabilities::NONE,
        )
        .unwrap(),
        NamespaceContext::new(
            NamespaceId::from_raw(911),
            NamespaceProfile::Confined,
            NamespaceCapabilities::NONE,
        )
        .unwrap(),
    )
}

#[cfg(unix)]
#[test]
fn listener_groups_reserve_shared_capacity_even_during_confined_setup() {
    use std::io::Write;
    use std::num::NonZeroUsize;
    use std::sync::Arc;
    let (path, group) = listener_group_paths();
    let (shared, confined) = listener_group_namespaces();
    let policy = Arc::new(ListenerXAdmissionPolicy::new(shared, confined));
    let config = XServerFrontendConfig::new_with_namespace_context(&path, shared)
        .unwrap()
        .with_admission_policy(policy.clone())
        .with_max_concurrent_clients(NonZeroUsize::new(4).unwrap())
        .with_listener(XServerFrontendListenerId(7), &group, confined)
        .unwrap();
    let mut frontend = XServerFrontend::bind_exclusive(config).unwrap();
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(4).unwrap());
    let observer: Arc<X11CoreTraceObserver> = Arc::new(|_| Ok(None));
    // Unfinished setup consumes worker capacity too.
    let stalled_a = connect_x_socket(&group);
    frontend.serve_next_concurrently().unwrap();
    let stalled_b = connect_x_socket(&group);
    frontend.serve_next_concurrently().unwrap();
    let mut deferred = connect_x_socket(&group);
    deferred
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    assert!(
        !frontend
            .try_serve_next_concurrently_routed_traced(&broker, observer.clone())
            .unwrap(),
        "a full confined group must defer acceptance"
    );
    let mut trusted = connect_x_socket(&path);
    trusted
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    assert!(
        frontend
            .try_serve_next_concurrently_routed_traced(&broker, observer.clone())
            .unwrap(),
        "confined setup must leave the shared listener a worker slot"
    );
    read_setup_success(&mut trusted, XByteOrder::LittleEndian);
    assert_eq!(
        policy.requests.lock().unwrap()[0].listener,
        XServerFrontendListenerId::PRIMARY
    );
    assert_eq!(frontend.active_client_worker_count(), 3);
    drop(stalled_a);
    drop(stalled_b);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !frontend
        .try_serve_next_concurrently_routed_traced(&broker, observer.clone())
        .unwrap()
    {
        assert!(
            std::time::Instant::now() < deadline,
            "departed confined workers must release their slots"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    read_setup_success(&mut deferred, XByteOrder::LittleEndian);
    assert_eq!(
        policy.requests.lock().unwrap()[1].listener,
        XServerFrontendListenerId(7)
    );
    drop(trusted);
    drop(deferred);
    frontend.wait_for_clients().unwrap();
    let revoked = policy.revoked.lock().unwrap();
    assert!(revoked.iter().any(|context| context.namespace == shared));
    assert!(revoked.iter().any(|context| context.namespace == confined));
    drop(frontend);
    assert!(
        !group.exists(),
        "additional listener path must be retired with its owner"
    );
    std::fs::remove_file(path).unwrap();
}

#[cfg(unix)]
#[test]
fn listener_groups_roll_back_partial_bind_without_unlinking_the_busy_listener() {
    let (path, group) = listener_group_paths();
    let occupied = path.with_extension("occupied");
    let live = std::os::unix::net::UnixListener::bind(&occupied).unwrap();
    let (shared, confined) = listener_group_namespaces();
    let config = XServerFrontendConfig::new_with_namespace_context(&path, shared)
        .unwrap()
        .with_admission_policy(std::sync::Arc::new(ListenerXAdmissionPolicy::new(
            shared, confined,
        )))
        .with_listener(XServerFrontendListenerId(7), &group, confined)
        .unwrap()
        .with_listener(XServerFrontendListenerId(8), &occupied, confined)
        .unwrap();
    assert!(XServerFrontend::bind_exclusive(config).is_err());
    assert!(!path.exists());
    assert!(!group.exists());
    assert!(std::os::unix::net::UnixStream::connect(&occupied).is_ok());
    drop(live);
    std::fs::remove_file(occupied).unwrap();
}
