use super::*;

#[test]
fn named_x_groups_and_xtest_are_explicit_and_bounded() {
    let parse = |args: &[&str]| {
        isolated_session_config(&args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
    };
    let base = parse(&[]).unwrap();
    assert!(base.x_groups.confined.is_empty());
    assert!(!base.admit_xtest);
    let config = parse(&[
        "--confined-group=2",
        "--confined-group=7",
        "--admit-xtest=confined-7",
    ])
    .unwrap();
    assert_eq!(config.x_groups.confined, [2, 7]);
    assert_eq!(
        config.x_groups.xtest,
        Some(socket_directories::ClientGroup::Confined(7))
    );
    assert!(config.admit_xtest);
    for args in [
        vec!["--confined-group=0"],
        vec!["--confined-group=1", "--confined-group=1"],
        vec!["--confined-group=../shared"],
        vec!["--admit-xtest=confined-7"],
        vec!["--admit-xtest", "--admit-xtest=shared"],
        vec![
            "--confined-group=7",
            "--admit-xtest=confined-7",
            "--expect-physical-pointer",
        ],
    ] {
        assert!(parse(&args).is_err(), "must refuse {args:?}");
    }
    let too_many = (1..=9)
        .map(|id| format!("--confined-group={id}"))
        .collect::<Vec<_>>();
    assert!(isolated_session_config(&too_many).is_err());
}

#[test]
fn live_x_admission_refuses_unknown_listener_without_allocating_identity() {
    use sophia_x_authority::{
        XServerFrontendAdmissionRequest, XServerFrontendListenerId, XServerFrontendPeerCredentials,
    };
    let mut registry = NamespaceRegistry::new(1).unwrap();
    let shared =
        registry.create_namespace(NamespaceProfile::ClassicShared, NamespaceCapabilities::NONE);
    let confined =
        registry.create_namespace(NamespaceProfile::Confined, NamespaceCapabilities::NONE);
    let registry = Arc::new(Mutex::new(registry));
    let policy = LiveXAdmissionPolicy {
        launch_origins: Default::default(),
        registry: registry.clone(),
        session_user_id: 1000,
        namespaces: std::collections::BTreeMap::from([
            (XServerFrontendListenerId::PRIMARY, shared.id),
            (XServerFrontendListenerId(7), confined.id),
        ]),
    };
    let mut request = XServerFrontendAdmissionRequest {
        listener: XServerFrontendListenerId(99),
        peer_credentials: Some(XServerFrontendPeerCredentials {
            process_start_time: None,
            process_id: 1,
            user_id: 1000,
            group_id: 1000,
        }),
        setup_authentication: sophia_protocol::ClientAuthenticationMethod::PeerCredentials,
        verified_private_input: None,
    };
    assert!(matches!(
        policy.admit(request),
        Err(XServerFrontendAdmissionError::Denied)
    ));
    assert_eq!(registry.lock().unwrap().admission_count(), 0);
    request.listener = XServerFrontendListenerId(7);
    let admission = policy.admit(request).unwrap();
    assert_eq!(admission.namespace, confined);
    policy.revoke(admission).unwrap();
    request.peer_credentials.as_mut().unwrap().user_id = 1001;
    assert!(matches!(
        policy.admit(request),
        Err(XServerFrontendAdmissionError::Denied)
    ));
    assert_eq!(registry.lock().unwrap().admission_count(), 0);
}
