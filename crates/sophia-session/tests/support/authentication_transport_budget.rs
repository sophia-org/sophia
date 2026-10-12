//! Real launch custody and Factotum cleanup, with supplied transport creation.
use super::*;
use sophia_9p::{Errno, Fid};
use sophia_runtime::authenticated_files::FileAuthenticationFactory;
use sophia_session::launch_authentication::files::TransportBudget;

#[test]
fn transport_slots_are_shared_bounded_and_retained_through_remote_cancellation() {
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
    let budget = TransportBudget::new(f.namespaces.clone());
    let endpoints: Vec<_> = (0..5).map(|_| budget.endpoint()).collect();
    let mut factories: Vec<_> = endpoints
        .iter()
        .map(|endpoint| {
            endpoint
                .bind(worker.clone(), b"role".to_vec(), b"role".to_vec())
                .unwrap()
        })
        .collect();
    let mut owners = Vec::new();
    for factory in factories.iter_mut().take(4) {
        let mut group = Vec::new();
        for _ in 0..8 {
            group.push(
                factory
                    .create(ConnectionId(1), Instant::now(), f.wake.clone())
                    .unwrap(),
            );
        }
        assert!(
            matches!(
                factory.create(ConnectionId(1), Instant::now(), f.wake.clone()),
                Err(Errno::EAGAIN)
            ),
            "a ninth unattached transport must not consume another endpoint's quota"
        );
        owners.push(group);
    }
    assert!(
        matches!(
            factories[4].create(ConnectionId(1), Instant::now(), f.wake.clone()),
            Err(Errno::EAGAIN)
        ),
        "different endpoints must share the Session's 32 transport slots"
    );

    // A real remote attempt must be cancelled before the transport owner is
    // discarded. Merely closing its wire connection does not release custody.
    let mut retiring = owners[0].pop().unwrap();
    let attempt = retiring.begin(Fid(7), Instant::now()).unwrap();
    let end = deadline();
    loop {
        f.tick(end);
        if let sophia_9p::ReadOutcome::Ready(bytes) =
            retiring.read(attempt, 0, 40, Instant::now()).unwrap()
        {
            assert_eq!(bytes.len(), 40);
            break;
        }
    }
    retiring.close(Instant::now());
    assert!(!retiring.drained(), "closing must retain remote cleanup");
    assert!(matches!(
        factories[0].create(ConnectionId(1), Instant::now(), f.wake.clone()),
        Err(Errno::EAGAIN)
    ));
    while !retiring.drained() {
        f.tick(end);
        assert_eq!(retiring.poll(Instant::now()), Err(Errno::ESTALE));
    }
    drop(retiring);
    let replacement = factories[0]
        .create(ConnectionId(1), Instant::now(), f.wake.clone())
        .expect("acknowledged cancellation returns exactly one transport slot");
    assert!(
        f.ingress.is_open(),
        "normal cleanup preserves other launches"
    );
    assert!(matches!(
        factories[4].create(ConnectionId(1), Instant::now(), f.wake.clone()),
        Err(Errno::EAGAIN)
    ));
    drop(replacement);
    let other_endpoint = factories[4]
        .create(ConnectionId(1), Instant::now(), f.wake.clone())
        .expect("a returned global slot is available to another endpoint");
    drop(other_endpoint);
    drop(owners);
    supervisor.request_termination().unwrap();
}
