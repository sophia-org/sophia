use super::*;
use sophia_config::namespace_policy::{LoadedNamespaceRecipe, NamespaceRecipeConfig};

fn policy(profile: &str, extra: &str) -> NamespaceRecipeConfig {
    let core = sophia_config::parse_core_config(
        format!("schema 2\nsession {{ namespace-policy {{ recipe \"selected\" profile=\"{profile}\" {extra}; }}; }}\n").as_bytes(),
        sophia_config::ConfigGeneration::INITIAL,
    ).unwrap();
    core.session.namespace_policy.recipes["selected"].clone()
}
fn observer_services() -> BTreeMap<String, ApprovedEndpoint> {
    let mut services = catalog(&[("inspection", 10), ("portal", 11)]);
    services.get_mut("portal").unwrap().service.interface = "portal-client".into();
    services
}

#[test]
fn shipped_observer_publishes_aliases_from_one_immutable_policy_snapshot() {
    let mut f = Fixture::new();
    let mut policy = policy("observer", "developer-launch=#true");
    let loaded = LoadedNamespaceRecipe::load(&policy, &f.root.join("absent")).unwrap();
    policy.name = "changed".into();
    policy.source = Some("absent".into());
    assert_eq!(loaded.policy().name, "selected");
    assert!(loaded.policy().source.is_none());
    let published = f
        .owner
        .publish_selected(1, &loaded, &observer_services())
        .unwrap();
    let view = &published.namespace.lease;
    assert_eq!(
        view.view()
            .endpoints
            .iter()
            .map(|e| e.path.as_str())
            .collect::<Vec<_>>(),
        ["roles/inspection", "roles/portal", "roles/status"]
    );
    assert_eq!(
        endpoint(view, "roles/inspection"),
        endpoint(view, "roles/status")
    );
    assert_ne!(
        endpoint(view, "roles/inspection").service,
        endpoint(view, "roles/portal").service
    );
    assert_eq!(f.registry.lock().unwrap().endpoint_grant_count(), 2);
    let discovery =
        decode_discovery(&std::fs::read(view.directory().join(DISCOVERY_NAME)).unwrap()).unwrap();
    assert_eq!(discovery.endpoints.len(), 3);
}

#[test]
fn observer_cannot_publish_other_role_interfaces_or_survive_missing_services() {
    let mut f = Fixture::new();
    let loaded = LoadedNamespaceRecipe::load(&policy("observer", ""), &f.root).unwrap();
    let mut approved = observer_services();
    approved.insert("admin".into(), approved["inspection"].clone());
    assert!(
        f.owner.publish_selected(1, &loaded, &approved).is_err(),
        "observer cannot acquire an unrelated role"
    );
    approved.remove("admin");
    approved.get_mut("inspection").unwrap().service.interface = "admin".into();
    assert!(
        f.owner.publish_selected(1, &loaded, &approved).is_err(),
        "role labels cannot disguise a different interface"
    );
    assert_eq!(f.registry.lock().unwrap().endpoint_grant_count(), 0);
    approved = observer_services();
    approved.remove("portal");
    assert!(
        f.owner.publish_selected(1, &loaded, &approved).is_err(),
        "missing required service must not publish a partial view"
    );
    assert!(f.owner.current().is_none());
    assert_eq!(f.registry.lock().unwrap().endpoint_grant_count(), 0);
}

#[test]
fn configured_custom_source_is_pinned_through_selected_publication() {
    let mut f = Fixture::new();
    std::fs::write(
        f.root.join("custom.ns"),
        "namespace-recipe 1\ninclude fragment.ns\n",
    )
    .unwrap();
    std::fs::write(
        f.root.join("fragment.ns"),
        "mount inspection custom/status\n",
    )
    .unwrap();
    let loaded =
        LoadedNamespaceRecipe::load(&policy("observer", "source=\"custom.ns\""), &f.root).unwrap();
    let old = f
        .owner
        .publish_selected(1, &loaded, &observer_services())
        .unwrap();
    assert_eq!(
        old.namespace.lease.view().endpoints[0].path,
        "custom/status"
    );
    std::fs::write(f.root.join("fragment.ns"), "mount portal custom/status\n").unwrap();
    assert!(
        f.owner
            .publish_selected(2, &loaded, &observer_services())
            .is_err()
    );
    assert_eq!(
        f.owner.current().unwrap().directory(),
        old.namespace.lease.directory()
    );
}

#[test]
fn shared_and_confined_x_compile_the_same_fragment_with_distinct_references() {
    use sophia_runtime::namespace_recipe::{Input, compile};
    let f = Fixture::new();
    let mut seen = Vec::new();
    for (profile, extra, id) in [("shared-x", "", 50), ("confined-x", "group=7", 51)] {
        let loaded = LoadedNamespaceRecipe::load(&policy(profile, extra), &f.root).unwrap();
        let services = [
            ("x-group", EndpointProtocol::X11, "x11", id),
            ("portal", EndpointProtocol::NineP2000L, "portal-client", 60),
        ]
        .into_iter()
        .map(|(name, protocol, interface, identity)| {
            (
                name.into(),
                GrantedEndpoint {
                    service: ServiceReference {
                        identity,
                        generation: 1,
                        protocol,
                        interface: interface.into(),
                        version: 1,
                    },
                    scope: EndpointGrantScope {
                        namespace: sophia_protocol::NamespaceId::from_raw(1),
                        principal: 7,
                        grant: identity,
                        generation: 1,
                    },
                },
            )
        })
        .collect();
        let view = compile(Input {
            revision: 1,
            namespace: sophia_protocol::NamespaceId::from_raw(1),
            principal: 7,
            source: loaded.source(),
            fragments: loaded.files(),
            services: &services,
        })
        .unwrap();
        assert_eq!(
            view.endpoints
                .iter()
                .map(|e| e.path.as_str())
                .collect::<Vec<_>>(),
            ["roles/x11", "roles/portal"]
        );
        assert_eq!(
            view.endpoints[0].endpoint.service.protocol,
            EndpointProtocol::X11
        );
        seen.push(view.endpoints[0].endpoint.service.clone());
    }
    assert_ne!(seen[0], seen[1]);
    // This is naming compilation, not X11 socket materialization or admission.
}

#[test]
fn configured_x_recipe_exposes_the_existing_socket_through_sdk_discovery() {
    use sophia_runtime::namespace_publication::CapturedX11Socket;
    use std::os::unix::net::UnixListener;
    let mut f = Fixture::new();
    let path = f.root.join("X77");
    let frontend = UnixListener::bind(&path).unwrap();
    frontend.set_nonblocking(true).unwrap();
    let loaded = LoadedNamespaceRecipe::load(&policy("confined-x", "group=7"), &f.root).unwrap();
    let mut approved = observer_services();
    let mut x = approved.remove("inspection").unwrap();
    x.service.protocol = EndpointProtocol::X11;
    x.service.interface = "x11".into();
    let capture = CapturedX11Socket::capture(x.service.clone(), &path).unwrap();
    approved.insert("x-group".into(), x);
    let published = f
        .owner
        .publish_selected_with_x11(1, &loaded, &approved, &[capture])
        .unwrap();
    assert_eq!(
        published.namespace.listeners.len(),
        1,
        "only the portal requires a new 9P listener"
    );
    let sdk =
        sophia_namespace_client::Namespace::open(published.namespace.lease.directory()).unwrap();
    assert_eq!(
        sdk.discovery().endpoints[0].protocol,
        sophia_namespace_client::Protocol::X11
    );
    let _connection = sdk.connect("roles/x11").unwrap();
    assert!(
        frontend.accept().is_ok(),
        "selected X recipe must connect to the existing frontend socket"
    );
    assert_eq!(
        std::fs::read_dir(&f.root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name() == "X77")
            .count(),
        1
    );
    // The fixture supplies the listener; X setup/cookies and frontend namespace
    // admission remain the existing X authority's job, not a discovery claim.
}
