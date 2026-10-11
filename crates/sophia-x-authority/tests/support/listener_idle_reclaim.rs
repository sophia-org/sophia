use super::*;

struct GroupAdmission;
impl XServerFrontendAdmissionPolicy for GroupAdmission {
    fn admit(
        &self,
        request: XServerFrontendAdmissionRequest,
    ) -> Result<sophia_protocol::ClientAdmissionContext, XServerFrontendAdmissionError> {
        let id = match request.listener.0 {
            0 => 9701,
            7 => 9702,
            _ => return Err(XServerFrontendAdmissionError::Denied),
        };
        Ok(namespaced(
            XServerFrontendClientId(id),
            NamespaceId::from_raw(9701),
        ))
    }
    fn revoke(
        &self,
        _: sophia_protocol::ClientAdmissionContext,
    ) -> Result<(), XServerFrontendAdmissionError> {
        Ok(())
    }
}

/// Real listeners and connection workers feed the production idle guard.
/// The fixture drives service turns directly; no replacement count is supplied.
/// Both listeners use the private runner's one prepared namespace. Namespace
/// separation is exercised by the public wire tests; this checks global idleness.
#[test]
fn listener_groups_reclaim_only_after_every_listener_is_idle() {
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 16);
    let private = private_for_roles(&keeper);
    let runner = private
        .prepare_runner(NamespaceId::from_raw(9701), &keeper)
        .unwrap_or_else(|(cause, _)| panic!("runner refused: {cause:?}"));
    let private = runner.frontend();
    let lease = keeper.lease();
    let primary = private_service_socket("groups");
    let other = primary.with_extension("group");
    let namespace = sophia_protocol::NamespaceContext::new(
        NamespaceId::from_raw(9701),
        sophia_protocol::NamespaceProfile::Confined,
        sophia_protocol::NamespaceCapabilities::NONE,
    )
    .unwrap();
    let config = XServerFrontendConfig::new(&primary, NamespaceId::from_raw(9701))
        .unwrap()
        .with_admission_policy(Arc::new(GroupAdmission))
        .with_listener(crate::XServerFrontendListenerId(7), &other, namespace)
        .unwrap();
    let mut frontend = XServerFrontend::bind_exclusive(config).unwrap();
    let mut first = connect_private_client(&primary);
    frontend
        .serve_next_concurrently_routed(&private.broker)
        .unwrap();
    handshake(&mut first);
    assert!(waited_for(|| attach_ready_workers(private, &lease) == 1));
    let departed = lease
        .custodies_of(&private.broker.registry)
        .into_iter()
        .next()
        .unwrap();
    let mut second = connect_private_client(&other);
    frontend
        .serve_next_concurrently_routed(&private.broker)
        .unwrap();
    handshake(&mut second);
    assert!(waited_for(|| attach_ready_workers(private, &lease) == 1));
    assert_eq!(frontend.active_client_worker_count(), 2);

    drop(first);
    assert!(waited_for(|| {
        frontend.poll_client_workers().unwrap();
        frontend.active_client_worker_count() == 1
    }));
    assert!(matches!(
        departed.cleanup_record().destruction_standing(),
        PrivateDestructionStanding::Decided(PrivateDestructionDecision::Deferred(_))
    ));
    assert_eq!(
        reclaim_idle_departures(private, &lease, frontend.active_client_worker_count()),
        0,
        "a live client on another listener must prevent idle reclaim"
    );
    assert_eq!(
        departed.deferred_cleanup_standing(),
        PrivateDeferredCleanupStanding::NotVisited
    );

    drop(second);
    assert!(waited_for(|| {
        frontend.poll_client_workers().unwrap();
        frontend.active_client_worker_count() == 0
    }));
    assert!(
        waited_for(|| {
            reclaim_idle_departures(private, &lease, frontend.active_client_worker_count());
            matches!(
                departed.deferred_cleanup_standing(),
                PrivateDeferredCleanupStanding::Done(_)
            )
        }),
        "departure must be reclaimed once every listener is idle"
    );
    assert!(waited_for(|| {
        reclaim_idle_departures(private, &lease, frontend.active_client_worker_count());
        lease.custodies_of(&private.broker.registry).is_empty()
    }));
    drop(frontend);
    std::fs::remove_file(primary).unwrap();
}
