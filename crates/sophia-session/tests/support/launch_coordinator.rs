//! Real shared Factotum owner; publication and monotonic time are supplied.
use super::*;
use crate::launch_authentication::{Error, LaunchCoordinator, LaunchPolicy, Phase};
use sophia_9p::{
    Limits,
    unix::{Server, Wake},
};
use sophia_factotum::{
    client::SessionClient,
    export::FactotumExport,
    launch::queue::{self, ConnectionOwner},
    proto::{ChannelClass, Settings},
};
use sophia_runtime::{
    ProcessLaunchSpec, ProcessSupervisor, ProtectionDomainRole, ProtectionDomainSpec,
    SupervisedProcessKind,
};
use std::{
    os::unix::net::UnixStream,
    sync::mpsc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

struct Agent {
    ingress: Ingress,
    owner: ConnectionOwner,
    wake: Wake,
    thread: Option<JoinHandle<()>>,
}
fn namespaces() -> std::sync::Arc<std::sync::Mutex<sophia_runtime::NamespaceRegistry>> {
    use sophia_protocol::{NamespaceCapabilities, NamespaceContext, NamespaceId, NamespaceProfile};
    std::sync::Arc::new(std::sync::Mutex::new(
        sophia_runtime::NamespaceRegistry::with_namespace(
            31,
            NamespaceContext::new(
                NamespaceId::from_raw(1),
                NamespaceProfile::Confined,
                NamespaceCapabilities::NONE,
            )
            .unwrap(),
        )
        .unwrap(),
    ))
}
impl Agent {
    fn new() -> Self {
        let (client, stream) = UnixStream::pair().unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            let mut server = Server::new(
                FactotumExport::new(Settings {
                    owner: "fixture".into(),
                    pam_service: "fixture".into(),
                }),
                Limits::default(),
            )
            .unwrap();
            let connection = server.adopt(stream).unwrap_or_else(|_| panic!("adopt"));
            server.export_mut().admit(connection, ChannelClass::Session);
            sender.send(server.wake()).unwrap();
            while server.turn(Some(Duration::from_secs(1))).unwrap() {}
        });
        let wake = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        let client = SessionClient::over(client, Duration::from_secs(2)).unwrap();
        let (ingress, owner) = queue::channel();
        Self {
            ingress,
            owner: owner.connect(client),
            wake,
            thread: Some(thread),
        }
    }
    fn until(
        &mut self,
        coordinator: &mut LaunchCoordinator,
        done: impl Fn(&LaunchCoordinator) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done(coordinator) {
            assert!(
                Instant::now() < deadline,
                "shared launch coordinator progress deadline"
            );
            self.owner.poll(Instant::now()).unwrap();
            coordinator.poll(Instant::now()).unwrap();
            std::thread::yield_now();
        }
    }
}
impl Drop for Agent {
    fn drop(&mut self) {
        self.wake.stop();
        let result = self.thread.take().unwrap().join();
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}
fn policy(principal: u64) -> LaunchPolicy {
    LaunchPolicy {
        principal,
        namespace: 1,
        audience: 2,
        role: ProtectionDomainRole::LockProvider,
    }
}

#[test]
fn replacement_retires_old_registrations_and_preserves_supervisor_generations() {
    let service = LaunchService::default();
    let mut coordinator = LaunchCoordinator::new(service.clone(), namespaces());
    coordinator.poll(Instant::now()).unwrap();
    assert!(!coordinator.ready());
    assert!(matches!(
        coordinator.reserve(policy(1), Instant::now()),
        Err(Error::NotReady)
    ));
    let mut first = Agent::new();
    assert!(service.publish(Some(first.ingress.clone())));
    first.until(&mut coordinator, LaunchCoordinator::ready);
    let old = coordinator.reserve(policy(1), Instant::now()).unwrap();
    first.until(&mut coordinator, |c| c.phase(old) == Some(Phase::Ready));
    // A publication lock held by the supervisor is not a disconnection.
    let publication = service.0.lock().unwrap();
    coordinator.poll(Instant::now()).unwrap();
    assert_eq!(coordinator.phase(old), Some(Phase::Ready));
    drop(publication);
    let old_channel = coordinator.take_channel(old, Instant::now()).unwrap();
    let old_generation = old_channel.reservation().generation();
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::LockProvider,
        ProcessLaunchSpec::new("/usr/bin/sleep")
            .arg("30")
            .protection_domain(
                ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::LockProvider]).unwrap(),
            ),
    );
    supervisor.start_with_launch_channel(old_channel).unwrap();

    let mut second = Agent::new();
    assert!(service.publish(Some(second.ingress.clone())));
    assert!(!first.ingress.is_open());
    second.until(&mut coordinator, |c| c.phase(old).is_none() && c.ready());
    let fresh = coordinator.reserve(policy(1), Instant::now()).unwrap();
    second.until(&mut coordinator, |c| c.phase(fresh) == Some(Phase::Ready));
    assert!(
        matches!(
            coordinator.take_channel(old, Instant::now()),
            Err(Error::Stale)
        ),
        "a prior Factotum generation must not take the replacement's launch channel"
    );
    let channel = coordinator.take_channel(fresh, Instant::now()).unwrap();
    assert!(
        channel.reservation().generation() > old_generation,
        "Factotum replacement must not reuse a supervisor launch generation"
    );
    supervisor.request_termination().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while supervisor.poll().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "old protected launch reap deadline"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    supervisor
        .start_with_launch_channel(channel)
        .expect("the same supervisor must accept the fresh Factotum generation's launch");
    assert_eq!(coordinator.phase(fresh), Some(Phase::Spawning));
    service.withdraw();
    assert_eq!(coordinator.phase(fresh), None);
    assert!(!coordinator.ready());
    coordinator.poll(Instant::now()).unwrap();
    assert!(matches!(
        coordinator.revoke(fresh, Instant::now()),
        Err(Error::Stale)
    ));
}

#[test]
fn role_reservations_share_one_factotum_registration_bound() {
    let service = LaunchService::default();
    let mut coordinator = LaunchCoordinator::new(service.clone(), namespaces());
    let mut agent = Agent::new();
    assert!(service.publish(Some(agent.ingress.clone())));
    agent.until(&mut coordinator, LaunchCoordinator::ready);
    for n in 0..64 {
        let registration = coordinator
            .reserve(policy(1 + n % 2), Instant::now())
            .unwrap();
        agent.until(&mut coordinator, |c| {
            c.phase(registration) == Some(Phase::Ready)
        });
    }
    assert!(
        matches!(
            coordinator.reserve(policy(3), Instant::now()),
            Err(Error::Capacity)
        ),
        "role owners must share the 64-launch Session bound"
    );
    drop(coordinator);
    assert!(
        !agent.ingress.is_open(),
        "owner drop retires its shared authority"
    );
}

#[test]
fn initialization_timeout_does_not_retry_or_publish_an_unverified_book() {
    let service = LaunchService::default();
    let (ingress, _owner) = queue::channel();
    assert!(service.publish(Some(ingress.clone())));
    let mut coordinator = LaunchCoordinator::new(service, namespaces());
    let now = Instant::now();
    coordinator.poll(now).unwrap();
    assert!(!coordinator.ready());
    assert!(coordinator.poll(now + Duration::from_secs(6)).is_err());
    assert!(!ingress.is_open());
    coordinator.poll(now + Duration::from_secs(7)).unwrap();
    assert!(!coordinator.ready());
}
