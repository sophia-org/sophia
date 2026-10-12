//! Real launch custody; supplied endpoint selection. Neither an equal service
//! name nor equal grant numbers from another registry can replace its owner.
use super::*;
use sophia_protocol::namespace_view::{EndpointProtocol, ServiceReference};
use sophia_runtime::authenticated_files::FileAuthenticationFactory;
use sophia_runtime::{EndpointGrantPolicy, NamespaceRegistry};
use sophia_session::launch_authentication::files::TransportBudget;

#[test]
fn endpoint_binding_requires_the_exact_captured_service_scope_and_owner() {
    let mut f = Fixture::new();
    let id = f.reserve();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell]).unwrap();
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::Shell,
        ProcessLaunchSpec::new("/usr/bin/sleep")
            .arg("30")
            .protection_domain(domain),
    );
    supervisor
        .start_with_launch_channel(f.book.take_channel(id, Instant::now()).unwrap())
        .unwrap();
    f.activate(id, &mut supervisor);
    let worker = f
        .book
        .worker_authority(id, &mut supervisor, Instant::now())
        .unwrap();
    let namespace = f
        .namespaces
        .lock()
        .unwrap()
        .namespace(sophia_protocol::NamespaceId::from_raw(9))
        .unwrap();
    let policy = EndpointGrantPolicy {
        namespace: namespace.id,
        principal: 8,
        launch_generation: id.generation(),
        audience: 4,
        service: ServiceReference {
            identity: 20,
            generation: 3,
            protocol: EndpointProtocol::NineP2000L,
            interface: "inspection".into(),
            version: 1,
        },
    };
    let grant = f
        .namespaces
        .lock()
        .unwrap()
        .grant_endpoint(policy.clone())
        .unwrap();
    let budget = TransportBudget::new(f.namespaces.clone());
    let endpoint = budget.endpoint();
    let bind = |grant: sophia_runtime::EndpointGrantLease,
                reference: &sophia_protocol::namespace_view::GrantedEndpoint| {
        endpoint.bind_grant(
            worker.clone(),
            grant,
            reference,
            b"observer".to_vec(),
            b"inspection".to_vec(),
        )
    };
    let mut rebound = grant.endpoint().clone();
    rebound.service.generation += 1;
    assert!(
        matches!(bind(grant.clone(), &rebound), Err(sophia_9p::Errno::EACCES)),
        "a changed service generation cannot reuse the captured grant"
    );
    let mut alias_scope = grant.endpoint().clone();
    alias_scope.scope.grant += 1;
    assert!(
        matches!(
            bind(grant.clone(), &alias_scope),
            Err(sophia_9p::Errno::EACCES)
        ),
        "a different scope cannot inherit the endpoint grant"
    );
    for field in 0..4 {
        let mut bad = policy.clone();
        match field {
            0 => bad.principal += 1,
            1 => bad.launch_generation += 1,
            2 => bad.audience += 1,
            _ => {
                bad.namespace = f
                    .namespaces
                    .lock()
                    .unwrap()
                    .create_namespace(
                        sophia_protocol::NamespaceProfile::Confined,
                        sophia_protocol::NamespaceCapabilities::NONE,
                    )
                    .id
            }
        }
        let other = f.namespaces.lock().unwrap().grant_endpoint(bad).unwrap();
        assert!(
            matches!(
                bind(other.clone(), other.endpoint()),
                Err(sophia_9p::Errno::EACCES)
            ),
            "foreign launch policy cannot acquire this endpoint"
        );
    }
    let mut foreign = NamespaceRegistry::with_namespace(31, namespace).unwrap();
    let equal_numbers = foreign.grant_endpoint(policy).unwrap();
    assert_eq!(equal_numbers.endpoint(), grant.endpoint());
    assert!(
        matches!(
            bind(equal_numbers.clone(), equal_numbers.endpoint()),
            Err(sophia_9p::Errno::ESTALE)
        ),
        "an endpoint grant from another registry is not Session authority"
    );
    let mut factory = bind(grant.clone(), grant.endpoint()).unwrap();
    let mut owner = factory
        .create(ConnectionId(1), Instant::now(), f.wake.clone())
        .unwrap();
    f.namespaces
        .lock()
        .unwrap()
        .revoke_endpoint_grant(&grant)
        .unwrap();
    assert!(
        matches!(
            factory.create(ConnectionId(2), Instant::now(), f.wake.clone()),
            Err(sophia_9p::Errno::ESTALE)
        ),
        "revocation refuses a new transport before authentication"
    );
    assert_eq!(
        owner.poll(Instant::now()),
        Err(sophia_9p::Errno::ESTALE),
        "revocation ends a quiet pre-auth transport"
    );
    assert!(factory.retire(owner).is_ok());
    budget.poll_retiring(Instant::now());
}
