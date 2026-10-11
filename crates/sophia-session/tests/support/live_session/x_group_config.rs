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

fn parse_groups(values: &[&str]) -> Result<x_groups::XGroups, Box<dyn std::error::Error>> {
    x_groups::XGroups::parse(&values.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
}

#[test]
fn clipboard_rules_are_explicit_directional_and_bounded() {
    use socket_directories::ClientGroup::{Confined, Shared};
    assert!(parse_groups(&[]).unwrap().clipboard_rules.is_empty());
    let groups = parse_groups(&[
        "--confined-group=7",
        "--confined-group=3",
        "--clipboard-rule=confined-7:shared",
        "--clipboard-rule=shared:confined-7",
        "--clipboard-rule=confined-3:confined-7",
    ])
    .unwrap();
    // Argument order; the reverse of a pair is its own rule.
    assert_eq!(
        groups.clipboard_rules,
        [
            (Confined(7), Shared),
            (Shared, Confined(7)),
            (Confined(3), Confined(7))
        ]
    );
    let refused: &[&[&str]] = &[
        &["--clipboard-rule"],
        &["--clipboard-rule="],
        &["--confined-group=7", "--clipboard-rule=confined-7"],
        &["--confined-group=7", "--clipboard-rule=confined-7:"],
        &["--confined-group=7", "--clipboard-rule=:shared"],
        &["--confined-group=7", "--clipboard-rule=confined-7:shared:x"],
        &["--confined-group=7", "--clipboard-rule=confined-7:trusted"],
        &["--confined-group=7", "--clipboard-rule=confined-0:shared"],
        &["--confined-group=7", "--clipboard-rule=confined-07x:shared"],
        &["--confined-group=7", "--clipboard-rule=confined-3:shared"],
        &["--confined-group=7", "--clipboard-rule=shared:confined-3"],
        &["--clipboard-rule=shared:shared"],
        &[
            "--confined-group=7",
            "--clipboard-rule=confined-7:confined-7",
        ],
        &[
            "--confined-group=7",
            "--clipboard-rule=confined-7:shared",
            "--clipboard-rule=confined-7:shared",
        ],
    ];
    for args in refused {
        assert!(parse_groups(args).is_err(), "must refuse {args:?}");
    }
    // Configuration is judged after every argument, so order does not matter.
    assert!(parse_groups(&["--clipboard-rule=confined-7:shared", "--confined-group=7"]).is_ok());

    // Sixteen pairs fit; a seventeenth is refused.
    let mut args: Vec<String> = (1..=8).map(|id| format!("--confined-group={id}")).collect();
    let mut pairs = Vec::new();
    for id in 1..=8 {
        pairs.push(format!("--clipboard-rule=confined-{id}:shared"));
        pairs.push(format!("--clipboard-rule=shared:confined-{id}"));
    }
    args.extend(pairs);
    assert_eq!(
        x_groups::XGroups::parse(&args)
            .unwrap()
            .clipboard_rules
            .len(),
        16
    );
    args.push("--clipboard-rule=confined-1:confined-2".into());
    assert!(x_groups::XGroups::parse(&args).is_err());
}

#[test]
fn clipboard_rules_only_add_clipboard_capability_in_their_direction() {
    use socket_directories::ClientGroup::{Confined, Shared};
    use sophia_protocol::NamespacePortalCapability::{Clipboard, UriOpen};
    let groups = parse_groups(&[
        "--confined-group=7",
        "--confined-group=3",
        "--clipboard-rule=confined-7:shared",
    ])
    .unwrap();
    let none = NamespaceCapabilities::NONE;
    let source = groups.capabilities(Confined(7), none);
    assert!(source.allows_publish(Clipboard) && !source.allows_request(Clipboard));
    let target = groups.capabilities(Shared, none);
    assert!(target.allows_request(Clipboard) && !target.allows_publish(Clipboard));
    // A group no rule names keeps exactly its base.
    assert_eq!(groups.capabilities(Confined(3), none), none);
    // Only Clipboard bits are added, and nothing in the base is cleared.
    let base = NamespaceCapabilities::NONE
        .with_request(UriOpen)
        .with_publish(UriOpen);
    let augmented = groups.capabilities(Confined(7), base);
    assert!(augmented.allows_request(UriOpen) && augmented.allows_publish(UriOpen));
    assert_eq!(
        augmented,
        base.with_publish(Clipboard),
        "only the Clipboard publish bit is added"
    );
    assert_eq!(
        groups.capabilities(Shared, NamespaceCapabilities::ALL),
        NamespaceCapabilities::ALL
    );
    // Both directions configured give both capabilities.
    let both = parse_groups(&[
        "--confined-group=7",
        "--clipboard-rule=confined-7:shared",
        "--clipboard-rule=shared:confined-7",
    ])
    .unwrap();
    let capabilities = both.capabilities(Confined(7), none);
    assert!(capabilities.allows_publish(Clipboard) && capabilities.allows_request(Clipboard));
}

#[test]
fn xtest_selection_is_unchanged_by_the_shared_selector() {
    use socket_directories::ClientGroup::{Confined, Shared};
    assert_eq!(
        parse_groups(&["--admit-xtest"]).unwrap().xtest,
        Some(Shared)
    );
    assert_eq!(
        parse_groups(&["--admit-xtest=shared"]).unwrap().xtest,
        Some(Shared)
    );
    assert_eq!(
        parse_groups(&["--confined-group=4", "--admit-xtest=confined-4"])
            .unwrap()
            .xtest,
        Some(Confined(4))
    );
    for args in [
        ["--admit-xtest=confined-0"],
        ["--admit-xtest=trusted"],
        ["--admit-xtest=confined-4"],
    ] {
        let error = parse_groups(&args).unwrap_err().to_string();
        assert!(error.contains("--admit-xtest"), "{args:?}: {error}");
    }
}
