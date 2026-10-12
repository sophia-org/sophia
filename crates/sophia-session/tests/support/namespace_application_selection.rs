//! Exact application selection joined to real namespace launch custody.
//! X listeners and cookies are supplied; no X setup or pixels are claimed.
#![cfg(test)]
use super::*;
use crate::namespace_launch::tests::Fixture;
use sophia_config::namespace_policy::NamespaceRecipeConfig;
use sophia_protocol::{
    NamespaceCapabilities,
    namespace_view::{ServiceAvailability, ServiceReference},
};
use std::os::unix::{
    fs::{DirBuilderExt, OpenOptionsExt},
    net::UnixListener,
};
use std::time::Duration;

fn policy(profile: NamespaceRecipeProfile) -> NamespacePolicyConfig {
    NamespacePolicyConfig {
        recipes: BTreeMap::from([(
            "app".into(),
            NamespaceRecipeConfig {
                name: "app".into(),
                profile,
                source: None,
                request: vec![],
                publish: vec![],
                developer_launch: false,
            },
        )]),
        applications: BTreeMap::from([
            ("registered:browser".into(), "app".into()),
            ("desktop:example.desktop".into(), "app".into()),
        ]),
    }
}
fn command() -> ApplicationLaunchCommand {
    ApplicationLaunchCommand {
        executable: "/usr/bin/sleep".into(),
        arguments: vec!["30".into()],
        working_directory: None,
        namespace_recipe: None,
    }
}
fn endpoint(identity: u64, interface: &str, protocol: EndpointProtocol) -> ApprovedEndpoint {
    ApprovedEndpoint {
        service: ServiceReference {
            identity,
            generation: 1,
            interface: interface.into(),
            protocol,
            version: 1,
        },
        audience: identity,
        availability: ServiceAvailability::Available,
    }
}
fn target(f: &Fixture, group: Option<u32>) -> (RecipeLaunchTarget, UnixListener) {
    let context = f
        .service
        .registry
        .lock()
        .unwrap()
        .namespace(NamespaceId::from_raw(2))
        .unwrap();
    let directory = f.root.join("group");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    let listener = UnixListener::bind(directory.join("X77")).unwrap();
    let cookie = f.root.join("cookie");
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&cookie)
        .unwrap();
    let x = RecipeXTarget::capture(
        context,
        group,
        endpoint(11, "x11", EndpointProtocol::X11).service,
        &ConfinedXApplicationLaunch {
            executable: "/usr/bin/sleep".into(),
            args: vec![],
            bubblewrap: sophia_runtime::DEFAULT_BUBBLEWRAP_PATH.into(),
            group_directory: directory,
            cookie_file: cookie,
            display_number: 77,
        },
    )
    .unwrap();
    (
        RecipeLaunchTarget {
            context,
            principal: 19,
            x11: Some(Arc::new(x)),
            inspection: None,
            portal: (
                endpoint(4, "portal-client", EndpointProtocol::NineP2000L),
                Arc::new(PortalObjectRecipient::new(context.id)),
            ),
            bubblewrap: sophia_runtime::DEFAULT_BUBBLEWRAP_PATH.into(),
            host_control: None,
            host_inspection: None,
        },
        listener,
    )
}

#[test]
fn recipe_selection_is_automatic_and_cannot_be_replaced_by_metadata() {
    let f = Fixture::with_profile(NamespaceProfile::Confined);
    let set = ApplicationRecipeSet::load(
        &policy(NamespaceRecipeProfile::ConfinedX { group: 7 }),
        &f.root,
    )
    .unwrap();
    for identity in ["registered:browser", "desktop:example.desktop"] {
        let mut command = command();
        assert_eq!(
            set.application(identity, &command)
                .unwrap()
                .unwrap()
                .policy()
                .name,
            "app"
        );
        command.namespace_recipe = Some("unapproved".into());
        assert!(
            set.application(identity, &command).is_err(),
            "metadata must not replace the selected application recipe"
        );
    }
    assert!(
        set.application("registered:other", &command())
            .unwrap()
            .is_none()
    );
    let mut requested = command();
    requested.namespace_recipe = Some("app".into());
    assert!(set.application("registered:other", &requested).is_err());
    assert!(
        set.developer("app").is_err(),
        "developer launch is disabled by default"
    );
}

#[test]
fn selected_x_applications_enter_the_common_supervisor_and_keep_their_group() {
    for group in [None, Some(7)] {
        let profile = if group.is_some() {
            NamespaceProfile::Confined
        } else {
            NamespaceProfile::ClassicShared
        };
        let mut f = Fixture::with_profile(profile);
        let (target, _listener) = target(&f, group);
        let profile = group.map_or(NamespaceRecipeProfile::SharedX, |group| {
            NamespaceRecipeProfile::ConfinedX { group }
        });
        let set = ApplicationRecipeSet::load(&policy(profile), &f.root).unwrap();
        let command = command();
        let selected = set
            .application("registered:browser", &command)
            .unwrap()
            .unwrap();
        let resolved = target.resolve(selected, &command).unwrap();
        assert_eq!(
            resolved.request.launch.protection_domain.is_some(),
            group.is_some()
        );
        assert_eq!(resolved.request.approved["x-group"].service.identity, 11);
        assert!(resolved.request.launch.null_stdin);
        assert!(resolved.request.launch.process_group);
        let mut app = resolved
            .start(
                &f.service,
                &mut f.auth.coordinator,
                f.wake.notifier(),
                Instant::now(),
                crate::diagnostics::application::LaunchContext {
                    source: crate::diagnostics::application::LaunchSource::Catalog,
                    transaction: Some(31),
                },
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert!(
                Instant::now() < deadline,
                "selected application activation deadline"
            );
            f.auth.tick();
            match app.poll(&mut f.auth.coordinator, f.wake.notifier(), Instant::now()) {
                Some(ApplicationProcessEvent::Started { .. }) => break,
                None => {}
                Some(ApplicationProcessEvent::Failed(reason)) => {
                    panic!("selected application: {reason}")
                }
                _ => panic!("selected application ended before activation"),
            }
        }
        assert!(app.launch_identity().is_some());
        assert!(app.cancel_namespace());
        f.reclaimed();
    }
}

#[test]
fn selection_refuses_foreign_groups_owners_and_ungranted_participation() {
    let f = Fixture::with_profile(NamespaceProfile::Confined);
    let (mut target, _listener) = target(&f, Some(7));
    let command = command();
    let other = ApplicationRecipeSet::load(
        &policy(NamespaceRecipeProfile::ConfinedX { group: 8 }),
        &f.root,
    )
    .unwrap();
    assert!(
        target
            .resolve(
                other
                    .application("registered:browser", &command)
                    .unwrap()
                    .unwrap(),
                &command
            )
            .is_err(),
        "recipe must not substitute another confined X group"
    );
    let mut policy = policy(NamespaceRecipeProfile::ConfinedX { group: 7 });
    policy
        .recipes
        .get_mut("app")
        .unwrap()
        .request
        .push(RecipePortalCapability::Clipboard);
    let set = ApplicationRecipeSet::load(&policy, &f.root).unwrap();
    assert!(
        target
            .resolve(
                set.application("registered:browser", &command)
                    .unwrap()
                    .unwrap(),
                &command
            )
            .is_err(),
        "recipe participation cannot exceed the admitted namespace"
    );
    policy.recipes.get_mut("app").unwrap().request.clear();
    let set = ApplicationRecipeSet::load(&policy, &f.root).unwrap();
    target.portal.1 = Arc::new(PortalObjectRecipient::new(NamespaceId::from_raw(99)));
    assert!(
        target
            .resolve(
                set.application("registered:browser", &command)
                    .unwrap()
                    .unwrap(),
                &command
            )
            .is_err()
    );
}

#[test]
fn preparation_refuses_a_replaced_conventional_x_listener() {
    let mut f = Fixture::with_profile(NamespaceProfile::Confined);
    let (target, old_listener) = target(&f, Some(7));
    let set = ApplicationRecipeSet::load(
        &policy(NamespaceRecipeProfile::ConfinedX { group: 7 }),
        &f.root,
    )
    .unwrap();
    let command = command();
    let resolved = target
        .resolve(
            set.application("registered:browser", &command)
                .unwrap()
                .unwrap(),
            &command,
        )
        .unwrap();
    let socket = f.root.join("group/X77");
    std::fs::remove_file(&socket).unwrap();
    let _replacement = UnixListener::bind(&socket).unwrap();
    drop(old_listener);
    let mut app = resolved
        .start(
            &f.service,
            &mut f.auth.coordinator,
            f.wake.notifier(),
            Instant::now(),
            crate::diagnostics::application::LaunchContext {
                source: crate::diagnostics::application::LaunchSource::Catalog,
                transaction: Some(32),
            },
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline, "replaced group refusal deadline");
        f.auth.tick();
        match app.poll(&mut f.auth.coordinator, f.wake.notifier(), Instant::now()) {
            Some(ApplicationProcessEvent::Failed(_)) => break,
            None => {}
            _ => panic!("changed X listener must never start the application"),
        }
    }
    assert!(app.launch_identity().is_none());
    f.reclaimed();
}

#[test]
fn observer_resolution_has_no_x_or_host_admin_reach() {
    let f = Fixture::with_profile(NamespaceProfile::Confined);
    let (mut target, _listener) = target(&f, Some(7));
    target.x11 = None;
    let mut service = sophia_runtime::inspection::InspectionService::private().unwrap();
    service.fence(1, None).unwrap();
    let mut inspection = endpoint(5, "inspection", EndpointProtocol::NineP2000L);
    inspection.service.generation = service.generation().unwrap();
    target.inspection = Some((
        inspection.clone(),
        service
            .authenticated_acceptor(inspection.service.generation)
            .unwrap(),
    ));
    let set =
        ApplicationRecipeSet::load(&policy(NamespaceRecipeProfile::Observer), &f.root).unwrap();
    let resolved = target
        .resolve(
            set.application("registered:browser", &command())
                .unwrap()
                .unwrap(),
            &command(),
        )
        .unwrap();
    assert_eq!(
        resolved
            .request
            .approved
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["inspection", "portal"]
    );
    assert!(resolved.request.launch.environment.is_empty());
    assert!(resolved.request.launch.protection_domain.is_some());
    target.context.capabilities =
        NamespaceCapabilities::NONE.with_request(NamespacePortalCapability::Clipboard);
    assert!(
        target
            .resolve(
                set.application("registered:browser", &command())
                    .unwrap()
                    .unwrap(),
                &command()
            )
            .is_err(),
        "observer cannot inherit clipboard participation"
    );
}
