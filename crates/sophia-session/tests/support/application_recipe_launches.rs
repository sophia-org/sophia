//! Actual Session recipe resource preparation and managed process dispatch.
//! The X listener and scene are supplied; no X setup or pixels are claimed.
#![cfg(test)]
use super::*;
use crate::live_session::tests::lock_provider_factotum as factotum;
use crate::namespace_launch::ApplicationProcessEvent;
use sophia_config::namespace_policy::{NamespaceRecipeConfig, NamespaceRecipeProfile};
use std::os::unix::net::UnixListener;

#[test]
fn session_selects_recipe_owners_for_registered_and_desktop_launches() {
    for group in [None, Some(7)] {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(28000);
        let display = NEXT.fetch_add(1, Ordering::Relaxed);
        let profile = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/fixtures/mixed_output_probe.kdl");
        let mut config = PersistentXtermSessionConfig::from_args(&[
            format!(
                "--config={}",
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tools/fixtures/panel_gate_core.kdl")
                    .display()
            ),
            format!("--display=:{display}"),
            format!("--desktop-profile={}", profile.display()),
        ])
        .unwrap();
        let recipe = NamespaceRecipeConfig {
            name: "app".into(),
            profile: group.map_or(NamespaceRecipeProfile::SharedX, |group| {
                NamespaceRecipeProfile::ConfinedX { group }
            }),
            source: None,
            request: vec![],
            publish: vec![],
            developer_launch: false,
            capture_outputs: vec![],
        };
        let policy = NamespacePolicyConfig {
            recipes: BTreeMap::from([("app".into(), recipe)]),
            applications: BTreeMap::from([
                ("registered:sample".into(), "app".into()),
                ("desktop:sample.desktop".into(), "app".into()),
            ]),
        };
        // Supply accepted configuration directly: this join exercises resource
        // capture and process ownership; parser admission has separate tests.
        let mut snapshot = config.core_config_state.active().clone();
        snapshot.session.namespace_policy = policy;
        config.core_config_state = sophia_config::CoreConfigState::from_snapshot(snapshot);
        let layout = socket_directories::SocketDirectoryLayout::prepare(display).unwrap();
        let group_name = group.map_or(
            socket_directories::ClientGroup::Shared,
            socket_directories::ClientGroup::Confined,
        );
        layout.prepare_group(group_name).unwrap();
        let socket = layout
            .group_directory(group_name)
            .join(format!("X{display}"));
        let listener = UnixListener::bind(&socket).unwrap();
        let (cookie, _) = LiveXAuthorityFile::create_with_local_hosts(
            display,
            &[sophia_runtime::PROTECTION_DOMAIN_HOSTNAME.as_bytes()],
        )
        .unwrap();
        let registry = Arc::new(Mutex::new(NamespaceRegistry::new(31).unwrap()));
        let namespace = registry
            .lock()
            .unwrap()
            .create_namespace(
                if group.is_some() {
                    NamespaceProfile::Confined
                } else {
                    NamespaceProfile::ClassicShared
                },
                NamespaceCapabilities::NONE,
            )
            .id;
        let namespaces = BTreeMap::from([(
            group.map_or(
                sophia_x_authority::XServerFrontendListenerId::PRIMARY,
                sophia_x_authority::XServerFrontendListenerId,
            ),
            namespace,
        )]);
        let mut auth = factotum::Fixture::with_namespaces(registry.clone());
        let wake = sophia_wake::Wake::new().unwrap();
        let mut applications = LiveApplicationLaunches::prepare(
            &config,
            registry,
            &namespaces,
            cookie.path(),
            31,
            None,
        )
        .unwrap();
        let command = ApplicationLaunchCommand {
            executable: "/usr/bin/sleep".into(),
            arguments: vec!["30".into()],
            working_directory: None,
            namespace_recipe: None,
        };
        for identity in ["registered:sample", "desktop:sample.desktop"] {
            let child = ApplicationLaunchTurn {
                applications: &mut applications,
                authentication: &mut auth.coordinator,
                wake: wake.notifier(),
            }
            .spawn(
                &config,
                cookie.path(),
                identity,
                &command,
                LaunchContext {
                    source: crate::diagnostics::application::LaunchSource::Catalog,
                    transaction: Some(71),
                },
            )
            .unwrap();
            let mut managed = ManagedSessionChild::new(None, child);
            let occupied = applications.prepared.as_ref().unwrap().service.occupied();
            if occupied != 1 {
                managed.terminate(true).unwrap();
            }
            assert_eq!(
                occupied, 1,
                "selected Session launch must enter namespace custody"
            );
            let activation = sophia_protocol::ShellLauncherActivation {
                connection_epoch: 5,
                catalog_generation: 7,
                request_generation: 8,
                candidate_generation: 9,
                presentation_epoch: 11,
                activation: 12,
                slot: 1,
            };
            managed.shell_launch = Some((TransactionId::from_raw(71), activation));
            assert!(
                managed.shell_outcome(None).is_none(),
                "pending launch cannot report Started"
            );
            assert!(managed.shell_launch.is_some());
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                assert!(Instant::now() < deadline, "managed recipe launch deadline");
                auth.tick();
                managed.pump(&mut auth.coordinator, wake.notifier(), Instant::now());
                match managed.poll(&mut auth.coordinator, wake.notifier(), Instant::now()) {
                    Some(event @ ApplicationProcessEvent::Started { .. }) => {
                        assert_eq!(
                            managed.shell_outcome(Some(&event)),
                            Some((
                                TransactionId::from_raw(71),
                                activation,
                                sophia_protocol::ShellLaunchStatus::Started
                            ))
                        );
                        assert!(
                            managed.shell_outcome(Some(&event)).is_none(),
                            "one correlated launch outcome"
                        );
                        break;
                    }
                    Some(_) => panic!("startup progress must preserve the managed Started event"),
                    None => {}
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(
                managed
                    .poll(&mut auth.coordinator, wake.notifier(), Instant::now())
                    .is_none()
            );
            managed.shell_launch = Some((TransactionId::from_raw(72), activation));
            assert_eq!(
                managed
                    .shell_outcome(Some(&ApplicationProcessEvent::Failed(
                        "supplied failure".into()
                    )))
                    .unwrap()
                    .2,
                sophia_protocol::ShellLaunchStatus::Failed
            );
            assert!(managed.shell_launch.is_none());
            managed.terminate(true).unwrap();
            while applications.prepared.as_ref().unwrap().service.occupied() != 0 {
                assert!(
                    Instant::now() < deadline,
                    "managed launch retirement deadline"
                );
                auth.tick();
            }
        }
        let mut unprepared = LiveApplicationLaunches::default();
        assert!(
            ApplicationLaunchTurn {
                applications: &mut unprepared,
                authentication: &mut auth.coordinator,
                wake: wake.notifier()
            }
            .spawn(
                &config,
                cookie.path(),
                "registered:sample",
                &command,
                LaunchContext {
                    source: crate::diagnostics::application::LaunchSource::Startup,
                    transaction: None
                }
            )
            .is_err(),
            "selected launch must not fall back when resource preparation is absent"
        );
        drop(listener);
        std::fs::remove_file(socket).unwrap();
    }
}
