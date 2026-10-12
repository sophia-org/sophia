//! Production compiler, grant owner and filesystem publication. These tests
//! supply policy and admissions; afid verification is joined in launch_authentication.
use sophia_config::namespace_recipe::PinnedNamespaceRecipe;
use sophia_protocol::{
    ClientAuthenticationMethod, NamespaceCapabilities, NamespaceProfile, namespace_view::*,
};
use sophia_runtime::{NamespaceRegistry, namespace_publication::NamespaceViewLease};
use sophia_session::namespace_views::{ApprovedEndpoint, NamespaceRecipeOwner, PublishedRecipe};
use std::collections::BTreeMap;
use std::os::unix::{fs::DirBuilderExt, net::UnixStream};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[path = "support/namespace_selected_recipe.rs"]
mod selected_recipe;

struct Fixture {
    root: PathBuf,
    registry: Arc<Mutex<NamespaceRegistry>>,
    owner: NamespaceRecipeOwner,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "s318-view-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        let mut registry = NamespaceRegistry::new(31).unwrap();
        let namespace =
            registry.create_namespace(NamespaceProfile::Confined, NamespaceCapabilities::NONE);
        let registry = Arc::new(Mutex::new(registry));
        let owner = NamespaceRecipeOwner::new(&root, registry.clone(), namespace.id, 7, 1).unwrap();
        Self {
            root,
            registry,
            owner,
        }
    }
    fn recipe(&self, name: &str, text: &str) -> PinnedNamespaceRecipe {
        std::fs::write(
            self.root.join(name),
            format!("namespace-recipe 1\n{text}\n"),
        )
        .unwrap();
        PinnedNamespaceRecipe::load(&self.root, name).unwrap()
    }
    fn published_a(&mut self) -> PublishedRecipe {
        let recipe = self.recipe("first", "mount a roles/item\nbind roles/item roles/alias");
        self.owner
            .publish(1, &recipe, &catalog(&[("a", 10)]))
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn catalog(values: &[(&str, u64)]) -> BTreeMap<String, ApprovedEndpoint> {
    values
        .iter()
        .map(|(name, id)| {
            (
                (*name).into(),
                ApprovedEndpoint {
                    service: ServiceReference {
                        identity: *id,
                        generation: 3,
                        protocol: EndpointProtocol::NineP2000L,
                        interface: "inspection".into(),
                        version: 1,
                    },
                    audience: *id,
                    availability: ServiceAvailability::Available,
                },
            )
        })
        .collect()
}
fn endpoint(view: &NamespaceViewLease, path: &str) -> GrantedEndpoint {
    view.view()
        .endpoints
        .iter()
        .find(|e| e.path == path)
        .unwrap()
        .endpoint
        .clone()
}
fn method(principal: u64) -> ClientAuthenticationMethod {
    ClientAuthenticationMethod::FactotumLaunch {
        principal,
        launch_generation: 1,
    }
}

#[test]
fn rebind_changes_new_names_without_retargeting_old_service_or_permission() {
    let mut f = Fixture::new();
    let old = f.published_a();
    let captured = endpoint(&old.namespace.lease, "roles/item");
    let grant = old.grant(&captured).unwrap().clone();
    let admitted = f
        .registry
        .lock()
        .unwrap()
        .prepare_granted_admission(&grant, method(7), 10)
        .unwrap()
        .publish_lease();
    let recipe = f.recipe("second", "mount b roles/item\nmount a archive/item");
    let new = f
        .owner
        .publish(2, &recipe, &catalog(&[("a", 10), ("b", 20)]))
        .unwrap();
    assert_eq!(
        endpoint(&new.namespace.lease, "roles/item")
            .service
            .identity,
        20
    );
    assert_eq!(endpoint(&new.namespace.lease, "archive/item"), captured);
    assert_eq!(endpoint(&old.namespace.lease, "roles/alias"), captured);
    assert!(
        admitted.is_live(),
        "naming rebind must not revoke still-approved A authority"
    );
    let _stream = UnixStream::connect(old.namespace.lease.directory().join("roles/item")).unwrap();
    assert!(old.namespace.listeners[0].listener.accept().is_ok());
    assert_eq!(
        new.namespace.listeners[0]
            .listener
            .accept()
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(f.registry.lock().unwrap().endpoint_grant_count(), 2);
}

#[test]
fn failed_publication_keeps_old_names_but_never_restores_reduced_authority() {
    let mut f = Fixture::new();
    let old = f.published_a();
    let captured = endpoint(&old.namespace.lease, "roles/item");
    let grant = old.grant(&captured).unwrap().clone();
    let admitted = f
        .registry
        .lock()
        .unwrap()
        .prepare_granted_admission(&grant, method(7), 10)
        .unwrap()
        .publish_lease();
    let recipe = f.recipe("second", "mount b roles/item");
    let fault = f.owner.directory().join(".current-next");
    std::fs::create_dir(&fault).unwrap();
    assert!(f.owner.publish(2, &recipe, &catalog(&[("b", 20)])).is_err());
    assert!(
        !admitted.is_live(),
        "failed naming publication must not revive revoked authority"
    );
    assert_eq!(
        f.owner.current().unwrap().directory(),
        old.namespace.lease.directory()
    );
    assert_eq!(
        std::fs::canonicalize(f.owner.directory().join("current")).unwrap(),
        old.namespace.lease.directory()
    );
    assert_eq!(
        f.registry.lock().unwrap().endpoint_grant_count(),
        0,
        "failed candidate grants must be reclaimed"
    );
    assert!(
        !f.owner.directory().join("v2").exists(),
        "failed candidate directory must be reclaimed"
    );
    std::fs::remove_dir(fault).unwrap();
    assert!(f.owner.publish(1, &recipe, &catalog(&[("a", 10)])).is_err());
    assert!(f.owner.publish(2, &recipe, &catalog(&[("b", 20)])).is_err());
    assert!(
        !grant.is_live(),
        "stale policy retries cannot revive the old grant"
    );
    let new = f.owner.publish(3, &recipe, &catalog(&[("b", 20)])).unwrap();
    let next = endpoint(&new.namespace.lease, "roles/item");
    assert!(
        next.scope.grant > captured.scope.grant + 1,
        "failed candidate identities are burned"
    );
    assert_eq!(next.service.identity, 20);
    assert!(!admitted.is_live());
}

#[test]
fn private_clients_share_service_identity_but_never_grant_scopes() {
    let mut f = Fixture::new();
    let first = f.published_a();
    let namespace = f
        .registry
        .lock()
        .unwrap()
        .create_namespace(NamespaceProfile::Confined, NamespaceCapabilities::NONE);
    let mut other =
        NamespaceRecipeOwner::new(&f.root, f.registry.clone(), namespace.id, 8, 1).unwrap();
    let recipe = f.recipe("other", "mount a roles/item\nmount a-again roles/alias");
    let second = other
        .publish(1, &recipe, &catalog(&[("a", 10), ("a-again", 10)]))
        .unwrap();
    let a = endpoint(&first.namespace.lease, "roles/item");
    let b = endpoint(&second.namespace.lease, "roles/item");
    assert_eq!(a.service, b.service);
    assert_ne!(a.scope, b.scope);
    assert_ne!(
        first.namespace.lease.directory(),
        second.namespace.lease.directory()
    );
    assert_eq!(endpoint(&second.namespace.lease, "roles/alias"), b);
    assert_eq!(
        f.registry.lock().unwrap().endpoint_grant_count(),
        2,
        "service aliases must not mint more grants"
    );
    assert!(
        second.grant(&a).is_none(),
        "foreign scope must not resolve through an equal service name"
    );
    let grant = second.grant(&b).unwrap();
    assert!(
        f.registry
            .lock()
            .unwrap()
            .prepare_granted_admission(grant, method(7), 10)
            .is_err()
    );
    let own = f
        .registry
        .lock()
        .unwrap()
        .prepare_granted_admission(grant, method(8), 10)
        .unwrap()
        .publish_lease();
    // Drop under the registry lock: policy custody must revoke without blocking
    // or needing mutable registry access, even during shutdown or failed reload.
    let registry = f.registry.lock().unwrap();
    let (done, completion) = std::sync::mpsc::sync_channel(1);
    let cleanup = std::thread::spawn(move || {
        drop(other);
        done.send(()).unwrap();
    });
    completion
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("policy cleanup must not wait for the registry mutex");
    cleanup.join().unwrap();
    assert!(
        !own.is_live(),
        "dropping policy custody must fence retained admissions"
    );
    assert!(first.grant(&a).unwrap().is_live());
    assert_eq!(registry.endpoint_grant_count(), 1);
}

#[test]
fn invalid_or_changed_recipe_never_exposes_partial_endpoints() {
    let mut f = Fixture::new();
    let bad = f.recipe("bad", "mount a roles/item\nmount missing roles/other");
    assert!(f.owner.publish(1, &bad, &catalog(&[("a", 10)])).is_err());
    assert!(f.owner.current().is_none());
    assert_eq!(f.registry.lock().unwrap().endpoint_grant_count(), 0);
    let valid = f.recipe("valid", "mount a roles/item");
    std::fs::write(f.root.join("valid"), "namespace-recipe 1\n").unwrap();
    assert!(f.owner.publish(2, &valid, &catalog(&[("a", 10)])).is_err());
    assert!(f.owner.current().is_none());
    assert_eq!(f.registry.lock().unwrap().endpoint_grant_count(), 0);
}

#[test]
fn authority_contention_refuses_before_reducing_current_grants() {
    let mut f = Fixture::new();
    let old = f.published_a();
    let captured = endpoint(&old.namespace.lease, "roles/item");
    let grant = old.grant(&captured).unwrap();
    let recipe = f.recipe("second", "mount b roles/item");
    let registry = f.registry.lock().unwrap();
    let error = f
        .owner
        .publish(2, &recipe, &catalog(&[("b", 20)]))
        .err()
        .unwrap();
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
    assert!(grant.is_live());
    drop(registry);
    assert!(f.owner.publish(2, &recipe, &catalog(&[("b", 20)])).is_ok());
    assert!(!grant.is_live());
}

#[test]
fn approved_catalog_is_bounded_and_rejects_conflicting_service_observations() {
    let mut f = Fixture::new();
    let old = f.published_a();
    let captured = endpoint(&old.namespace.lease, "roles/item");
    let recipe = f.recipe("empty", "");
    let mut approved = catalog(&[("a", 10), ("alias", 10)]);
    approved.get_mut("alias").unwrap().availability = ServiceAvailability::Recovering;
    assert!(f.owner.publish(2, &recipe, &approved).is_err());
    assert!(old.grant(&captured).unwrap().is_live());
    approved.get_mut("alias").unwrap().availability = ServiceAvailability::Available;
    approved.get_mut("alias").unwrap().service.version += 1;
    assert!(
        f.owner.publish(2, &recipe, &approved).is_err(),
        "one service incarnation cannot advertise conflicting metadata"
    );
    assert!(old.grant(&captured).unwrap().is_live());
    let mut approved: BTreeMap<_, _> = (1..=64)
        .map(|id| {
            let mut single = catalog(&[("service", id)]);
            (format!("service-{id}"), single.remove("service").unwrap())
        })
        .collect();
    let extra = approved["service-1"].clone();
    approved.insert("extra".into(), extra);
    assert!(f.owner.publish(2, &recipe, &approved).is_err());
    assert!(old.grant(&captured).unwrap().is_live());
    approved.remove("extra");
    assert!(f.owner.publish(2, &recipe, &approved).is_ok());
    assert_eq!(f.registry.lock().unwrap().endpoint_grant_count(), 64);
}
