use sophia_protocol::namespace_view::{EndpointProtocol, ServiceReference};
use sophia_protocol::{ClientAuthenticationMethod, NamespaceCapabilities, NamespaceProfile};
use sophia_runtime::{
    EndpointGrantPolicy, MAX_ENDPOINT_GRANTS, NamespaceRegistry, NamespaceRegistryError,
};

fn setup() -> (NamespaceRegistry, EndpointGrantPolicy) {
    let mut registry = NamespaceRegistry::new(31).unwrap();
    let namespace =
        registry.create_namespace(NamespaceProfile::Confined, NamespaceCapabilities::NONE);
    let policy = EndpointGrantPolicy {
        namespace: namespace.id,
        principal: 8,
        launch_generation: 7,
        audience: 4,
        service: ServiceReference {
            identity: 20,
            generation: 3,
            protocol: EndpointProtocol::NineP2000L,
            interface: "inspection".into(),
            version: 1,
        },
    };
    (registry, policy)
}
fn method() -> ClientAuthenticationMethod {
    ClientAuthenticationMethod::FactotumLaunch {
        principal: 8,
        launch_generation: 7,
    }
}

#[test]
fn grants_separate_shared_service_identity_from_independent_permission() {
    let (mut registry, policy) = setup();
    let a = registry.grant_endpoint(policy.clone()).unwrap();
    let b = registry.grant_endpoint(policy).unwrap();
    assert_eq!(a.endpoint().service, b.endpoint().service);
    assert_ne!(a.endpoint().scope, b.endpoint().scope);
    let alias = a.clone();
    let first = registry
        .prepare_granted_admission(&a, method(), 4)
        .unwrap()
        .publish_lease();
    let second = registry
        .prepare_granted_admission(&b, method(), 4)
        .unwrap()
        .publish_lease();
    drop(a);
    assert!(
        first.is_live(),
        "dropping a naming alias must not revoke its grant"
    );
    registry.revoke_endpoint_grant(&alias).unwrap();
    assert!(
        !first.is_live(),
        "revoked endpoint grant must fence retained admission"
    );
    assert!(!alias.is_live());
    assert!(!registry.is_current_admission(first.context()));
    assert!(
        second.is_live(),
        "unrelated grant for the same service must remain live"
    );
    assert_eq!(registry.admission_count(), 1);
    assert_eq!(registry.endpoint_grant_count(), 1);
    assert!(
        registry
            .prepare_granted_admission(&alias, method(), 4)
            .is_err()
    );
    assert_eq!(
        registry.revoke_endpoint_grant(&alias),
        Err(NamespaceRegistryError::StaleEndpointGrant)
    );
}

#[test]
fn identity_audience_and_registry_cannot_be_substituted() {
    let (mut registry, policy) = setup();
    let grant = registry.grant_endpoint(policy.clone()).unwrap();
    let other_namespace =
        registry.create_namespace(NamespaceProfile::Confined, NamespaceCapabilities::NONE);
    assert!(
        !grant.permits(other_namespace.id, method(), 4),
        "foreign namespace cannot use an endpoint grant"
    );
    for method in [
        ClientAuthenticationMethod::PeerCredentials,
        ClientAuthenticationMethod::FactotumLaunch {
            principal: 9,
            launch_generation: 7,
        },
        ClientAuthenticationMethod::FactotumLaunch {
            principal: 8,
            launch_generation: 8,
        },
    ] {
        assert!(
            registry
                .prepare_granted_admission(&grant, method, 4)
                .is_err()
        );
    }
    assert!(
        registry
            .prepare_granted_admission(&grant, method(), 5)
            .is_err()
    );
    let (mut foreign, _) = setup();
    let duplicate_numbers = foreign.grant_endpoint(policy).unwrap();
    assert_eq!(grant.endpoint(), duplicate_numbers.endpoint());
    assert!(
        !registry.endpoint_grant_is_current(&duplicate_numbers),
        "equal numbers from a foreign owner confer no authority"
    );
    assert!(
        registry
            .prepare_granted_admission(&duplicate_numbers, method(), 4)
            .is_err()
    );
    assert!(registry.revoke_endpoint_grant(&duplicate_numbers).is_err());
    assert!(grant.is_live());
}

#[test]
fn replacement_grant_never_revives_old_admissions() {
    let (mut registry, mut policy) = setup();
    let old = registry.grant_endpoint(policy.clone()).unwrap();
    let admission = registry
        .prepare_granted_admission(&old, method(), 4)
        .unwrap()
        .publish_lease();
    registry.revoke_endpoint_grant(&old).unwrap();
    // Reusing the numeric service with a new generation cannot retarget old fids.
    policy.service.generation += 1;
    let new = registry.grant_endpoint(policy.clone()).unwrap();
    assert_eq!(old.endpoint().service.generation, 3);
    assert_eq!(new.endpoint().service.generation, 4);
    assert!(new.endpoint().scope.grant > old.endpoint().scope.grant);
    assert!(!admission.is_live());
    let live = registry
        .prepare_granted_admission(&new, method(), 4)
        .unwrap()
        .publish_lease();
    let revoked = registry.revoke_namespace(policy.namespace).unwrap();
    assert_eq!(
        revoked.admissions,
        vec![live.context()],
        "namespace revocation must report live grant-backed admissions"
    );
    assert!(!live.is_live());
    assert!(!new.is_live());
    assert_eq!(registry.endpoint_grant_count(), 0);
}

#[test]
fn capacity_is_bounded_reclaimed_and_registry_drop_fences_leases() {
    let (mut registry, policy) = setup();
    let grants: Vec<_> = (0..MAX_ENDPOINT_GRANTS)
        .map(|_| registry.grant_endpoint(policy.clone()).unwrap())
        .collect();
    assert_eq!(registry.endpoint_grant_count(), MAX_ENDPOINT_GRANTS);
    assert!(matches!(
        registry.grant_endpoint(policy.clone()),
        Err(NamespaceRegistryError::EndpointGrantCapacity)
    ));
    registry.revoke_endpoint_grant(&grants[0]).unwrap();
    let new = registry.grant_endpoint(policy).unwrap();
    assert!(new.endpoint().scope.grant > grants.last().unwrap().endpoint().scope.grant);
    let admission = registry
        .prepare_granted_admission(&new, method(), 4)
        .unwrap()
        .publish_lease();
    drop(registry);
    assert!(!admission.is_live());
    assert!(!new.is_live());
    assert!(grants.iter().all(|g| !g.is_live()));
}

#[test]
fn invalid_policy_refuses_without_issuing_authority() {
    let (mut registry, policy) = setup();
    for field in 0..8 {
        let mut bad = policy.clone();
        match field {
            0 => bad.principal = 0,
            1 => bad.launch_generation = 0,
            2 => bad.audience = 0,
            3 => bad.service.identity = 0,
            4 => bad.service.generation = 0,
            5 => bad.service.version = 0,
            6 => bad.service.interface = "not/an/interface".into(),
            _ => bad.service.interface.clear(),
        }
        assert!(matches!(
            registry.grant_endpoint(bad),
            Err(NamespaceRegistryError::InvalidEndpointGrant)
        ));
        assert_eq!(registry.endpoint_grant_count(), 0);
    }
    assert_eq!(
        registry
            .grant_endpoint(policy)
            .unwrap()
            .endpoint()
            .scope
            .grant,
        1
    );
}

#[test]
fn launch_retirement_reclaims_only_its_own_grants() {
    let (mut registry, mut policy) = setup();
    let first = registry.grant_endpoint(policy.clone()).unwrap();
    let retained = registry
        .prepare_granted_admission(&first, method(), 4)
        .unwrap()
        .publish_lease();
    policy.launch_generation += 1;
    let other = registry.grant_endpoint(policy.clone()).unwrap();
    assert_eq!(
        registry.revoke_launch_endpoint_grants(policy.namespace, 99, 7),
        0
    );
    assert!(retained.is_live());
    assert_eq!(
        registry.revoke_launch_endpoint_grants(policy.namespace, 8, 7),
        1
    );
    assert!(!retained.is_live());
    assert!(other.is_live());
    assert_eq!(
        registry.revoke_launch_endpoint_grants(policy.namespace, 8, 7),
        0
    );
    assert_eq!(registry.endpoint_grant_count(), 1);
}
