//! Real Factotum registration and protected process lifetime; no display.
#![cfg(test)]
use super::*;
use crate::live_session::tests::lock_provider_factotum as factotum;
use sophia_config::namespace_policy::NamespaceRecipeConfig;
use sophia_protocol::namespace_view::{ServiceAvailability, ServiceReference};
use sophia_runtime::ProtectionDomainSpec;
use std::os::unix::fs::DirBuilderExt;
use std::time::Duration;

struct Fixture {
    auth: factotum::Fixture,
    service: NamespaceLaunchService,
    root: PathBuf,
    wake: sophia_wake::Wake,
}
impl Fixture {
    fn new() -> Self {
        Self::with_profile(sophia_protocol::NamespaceProfile::Confined)
    }
    fn with_profile(profile: sophia_protocol::NamespaceProfile) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "s318-launch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        std::fs::write(
            root.join("recipe"),
            "namespace-recipe 1\nmount inspection roles/status\n",
        )
        .unwrap();
        let registry = Arc::new(Mutex::new(
            NamespaceRegistry::with_namespace(
                31,
                sophia_protocol::NamespaceContext::new(
                    NamespaceId::from_raw(2),
                    profile,
                    sophia_protocol::NamespaceCapabilities::NONE,
                )
                .unwrap(),
            )
            .unwrap(),
        ));
        Self {
            auth: factotum::Fixture::with_namespaces(registry.clone()),
            service: NamespaceLaunchService::new(registry, root.clone()),
            root,
            wake: sophia_wake::Wake::new().unwrap(),
        }
    }
    fn request(&self) -> ClientLaunchRequest {
        ClientLaunchRequest {
            launch: ProcessLaunchSpec::new("/usr/bin/sleep")
                .arg("30")
                .protection_domain(
                    ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::NamespaceClient])
                        .unwrap(),
                ),
            recipe: LoadedNamespaceRecipe::load(
                &NamespaceRecipeConfig {
                    name: "observer".into(),
                    profile: NamespaceRecipeProfile::Observer,
                    source: Some("recipe".into()),
                    request: vec![],
                    publish: vec![],
                    developer_launch: true,
                },
                &self.root,
            )
            .unwrap(),
            namespace: NamespaceId::from_raw(2),
            principal: 3,
            approved: BTreeMap::from([(
                "inspection".into(),
                ApprovedEndpoint {
                    service: ServiceReference {
                        identity: 11,
                        generation: 1,
                        protocol: EndpointProtocol::NineP2000L,
                        interface: "inspection".into(),
                        version: 1,
                    },
                    audience: 4,
                    availability: ServiceAvailability::Available,
                },
            )]),
            x11: vec![],
        }
    }
    fn start(&mut self) -> PendingNamespaceClient {
        self.service
            .start(
                self.request(),
                &mut self.auth.coordinator,
                self.wake.notifier(),
                Instant::now(),
            )
            .unwrap()
    }
    fn reclaimed(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.service.occupied() != 0 {
            assert!(
                Instant::now() < deadline,
                "retirement must reclaim the launch slot"
            );
            self.auth.tick();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn abandoned_preparation_retires_registration_and_slot() {
    let mut f = Fixture::new();
    let pending = f.start();
    let registration = pending.registration.as_ref().unwrap().registration();
    drop(pending);
    f.reclaimed();
    f.auth.tick();
    assert_ne!(f.auth.coordinator.phase(registration), Some(Phase::Active));
}

#[test]
fn protected_recipe_client_runs_and_retires_without_blocking_the_owner() {
    let mut f = Fixture::new();
    let mut pending = f.start();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut running = loop {
        assert!(Instant::now() < deadline, "recipe client startup deadline");
        f.auth.tick();
        if let Some(running) = pending
            .poll(&mut f.auth.coordinator, f.wake.notifier(), Instant::now())
            .unwrap()
        {
            break running;
        }
    };
    assert!(running.peer_id().is_some());
    assert!(running.poll(&f.auth.coordinator).unwrap().is_none());
    let mut endpoints = running.take_endpoints();
    assert_eq!(endpoints.len(), 1);
    let server = sophia_9p::unix::Server::new(
        sophia_factotum::export::FactotumExport::new(sophia_factotum::proto::Settings {
            owner: "fixture".into(),
            pam_service: "fixture".into(),
        }),
        sophia_9p::Limits::default(),
    )
    .unwrap();
    let mut transport = endpoints[0]
        .factory
        .create(sophia_9p::ConnectionId(1), Instant::now(), server.wake())
        .unwrap();
    assert_eq!(f.service.occupied(), 1);
    let before = Instant::now();
    drop(running);
    assert_eq!(
        transport.poll(Instant::now()),
        Err(sophia_9p::Errno::ESTALE),
        "abandonment fences a quiet endpoint before the coordinator's next turn"
    );
    assert!(
        before.elapsed() < Duration::from_secs(1),
        "owner drop must not wait for child retirement"
    );
    f.reclaimed();
}

#[test]
fn abandoned_spawn_retains_its_slot_until_the_worker_returns() {
    let mut f = Fixture::new();
    let mut pending = f.start();
    let registration = pending.registration.as_ref().unwrap().registration();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "recipe reaches spawn");
        f.auth.tick();
        assert!(
            pending
                .poll(&mut f.auth.coordinator, f.wake.notifier(), Instant::now())
                .unwrap()
                .is_none()
        );
        if f.auth.coordinator.phase(registration) == Some(Phase::Spawning) {
            break;
        }
    }
    assert_eq!(f.service.occupied(), 1);
    let before = Instant::now();
    drop(pending);
    assert!(
        before.elapsed() < Duration::from_secs(1),
        "cancelling a spawn must not block Session"
    );
    f.reclaimed();
    f.auth.tick();
    assert_ne!(f.auth.coordinator.phase(registration), Some(Phase::Active));
}

#[test]
fn failed_executable_reclaims_its_publication_and_slot() {
    let mut f = Fixture::new();
    let mut request = f.request();
    request.launch = ProcessLaunchSpec::new("/definitely-absent-t318-executable")
        .protection_domain(
            ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::NamespaceClient]).unwrap(),
        );
    let mut pending = f
        .service
        .start(
            request,
            &mut f.auth.coordinator,
            f.wake.notifier(),
            Instant::now(),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "failed launch returns custody");
        f.auth.tick();
        match pending.poll(&mut f.auth.coordinator, f.wake.notifier(), Instant::now()) {
            Ok(None) => {}
            Ok(Some(_)) => panic!("missing executable cannot launch"),
            Err(_) => break,
        }
    }
    f.reclaimed();
    assert_eq!(f.service.registry.lock().unwrap().endpoint_grant_count(), 0);
}

#[test]
fn equal_namespace_numbers_from_another_registry_never_authorize_a_launch() {
    let mut f = Fixture::new();
    let other = NamespaceLaunchService::new(factotum::namespaces(), f.root.clone());
    let result = other.start(
        f.request(),
        &mut f.auth.coordinator,
        f.wake.notifier(),
        Instant::now(),
    );
    assert_eq!(
        result.err().unwrap(),
        "namespace launch and authentication owners differ"
    );
    assert_eq!(other.occupied(), 0);
}

#[test]
fn shared_recipe_uses_host_custody_and_the_same_endpoint_grant_owner() {
    let mut f = Fixture::with_profile(sophia_protocol::NamespaceProfile::ClassicShared);
    std::fs::write(
        f.root.join("shared"),
        "namespace-recipe 1\nmount portal roles/portal\n",
    )
    .unwrap();
    let mut request = f.request();
    request.launch = ProcessLaunchSpec::new("/usr/bin/sleep").arg("30");
    let mut policy = request.recipe.policy().clone();
    policy.profile = NamespaceRecipeProfile::SharedX;
    policy.source = Some("shared".into());
    policy.developer_launch = false;
    request.recipe = LoadedNamespaceRecipe::load(&policy, &f.root).unwrap();
    let mut endpoint = request.approved.remove("inspection").unwrap();
    endpoint.service.interface = "portal-requester".into();
    request.approved.insert("portal".into(), endpoint);
    let mut pending = f
        .service
        .start(
            request,
            &mut f.auth.coordinator,
            f.wake.notifier(),
            Instant::now(),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut running = loop {
        assert!(Instant::now() < deadline, "shared recipe startup deadline");
        f.auth.tick();
        if let Some(running) = pending
            .poll(&mut f.auth.coordinator, f.wake.notifier(), Instant::now())
            .unwrap()
        {
            break running;
        }
    };
    assert!(
        running
            .supervisor
            .as_ref()
            .unwrap()
            .protection_evidence()
            .is_none()
    );
    assert!(running.poll(&f.auth.coordinator).unwrap().is_none());
    let endpoints = running.take_endpoints();
    assert_eq!(endpoints.len(), 1);
    assert_eq!(
        endpoints[0].listener.endpoint.endpoint.service.interface,
        "portal-requester"
    );
    assert_eq!(f.service.registry.lock().unwrap().endpoint_grant_count(), 1);
    drop(running);
    f.reclaimed();
    assert_eq!(f.service.registry.lock().unwrap().endpoint_grant_count(), 0);
}

#[test]
fn a_confined_recipe_cannot_fall_back_to_the_host() {
    let mut f = Fixture::new();
    let mut request = f.request();
    request.launch.protection_domain = None;
    let result = f.service.start(
        request,
        &mut f.auth.coordinator,
        f.wake.notifier(),
        Instant::now(),
    );
    assert_eq!(result.err().unwrap(), "recipe requires containment");
    assert_eq!(f.service.occupied(), 0);
}
