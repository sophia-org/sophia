//! Actual connected worker, queue and ManagedSessionChild adoption. Supplied
//! transport protection/presentation; only device-hidden /bin/true executes.
use super::super::*;
use super::shell_file_peer;
use crate as sophia_session;
use crate::shell_native_launcher::{NativeLauncherActionService, NativeLauncherContentService};
use sophia_engine::PresentedContentTarget;
use sophia_protocol::*;
#[allow(dead_code)]
#[path = "content_actions/native_launcher_fixture.rs"]
mod fixture;
use fixture::*;

#[test]
fn connected_worker_adopts_exact_child_and_revocation_prevents_old_execution() {
    for mode in 0..4 {
        let revoked = mode == 1;
        let profile = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/fixtures/mixed_output_probe.kdl");
        let mut config = PersistentXtermSessionConfig::from_args(&[format!(
            "--desktop-profile={}",
            profile.display()
        )])
        .unwrap();
        let mut selected = config.session_profile.candidate().clone();
        selected
            .components
            .shell_components
            .push(sophia_config::ShellComponentConfig {
                id: "menu".into(),
                role: sophia_config::ShellComponentRole::ApplicationLauncher,
                executable: "/absent/menu".into(),
                config: None,
                reservation: None,
                gpu: sophia_config::ShellGpuMode::Denied,
                transport: sophia_config::ShellTransportSelection::NineP2000L,
            });
        config.session_profile = PreparedSessionProfile::new(selected).unwrap();
        config.application_catalog = Some(sophia_config::ApplicationCatalogConfig {
            name: "apps".into(),
            sources: vec![],
            applications: vec!["app1".into(), "app2".into()],
            terminal: None,
            terminal_arguments: vec![],
        });
        for id in ["app1", "app2"] {
            config.applications.applications.insert(
                id.into(),
                SessionApplicationSpec {
                    id: id.into(),
                    executable: "/bin/true".into(),
                    arguments: vec![],
                    placement_classification: None,
                },
            );
        }
        config.display = ":unavailable".into();
        let authority = std::path::Path::new("/nonexistent");
        let command = crate::application_catalog::ApplicationLaunchCommand {
            executable: "/bin/true".into(),
            arguments: vec![],
            working_directory: None,
        };
        let mut h = Harness::with_command(command);
        let mut catalog = component_catalog::ComponentCatalog::default();
        catalog
            .reconcile_connections(&[GRANT], &mut h.queue)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !catalog
            .visit_scan(&config, &mut h.queue, authority)
            .unwrap()
        {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let other_grant = ContentGrant {
            connection_epoch: 3,
            content_grant_epoch: 3,
        };
        let mut neighbor = if mode == 3 {
            let mut other_limits = limits();
            other_limits.grant = other_grant;
            let mut peer = Peer::with_limits(
                &mut h.epochs,
                sophia_runtime::ContentStoreProfile::NativeLauncher,
                other_limits,
            );
            peer.negotiate(&mut h.epochs);
            assert_eq!(peer.transport.content_limits().unwrap().grant, other_grant);
            catalog
                .reconcile_connections(&[GRANT, other_grant], &mut h.queue)
                .unwrap();
            assert!(
                catalog
                    .publish(&mut h.peer.transport.connection(&mut h.epochs))
                    .unwrap()
            );
            assert!(
                catalog
                    .publish(&mut peer.transport.connection(&mut h.epochs))
                    .unwrap()
            );
            let first = catalog
                .publication(GRANT)
                .unwrap()
                .published()
                .unwrap()
                .wire()
                .clone();
            let second = catalog
                .publication(other_grant)
                .unwrap()
                .published()
                .unwrap()
                .wire()
                .clone();
            assert_eq!(first.entries, second.entries);
            assert_eq!(first.generation, second.generation);
            assert_ne!(first.connection_epoch, second.connection_epoch);
            h.peer.transport.poll_io(&mut h.epochs).unwrap();
            peer.transport.poll_io(&mut h.epochs).unwrap();
            assert_eq!(h.peer.read_catalog(&mut h.epochs), first);
            assert_eq!(peer.read_catalog(&mut h.epochs), second);
            Some(peer)
        } else {
            None
        };
        let mut content =
            NativeLauncherContentService::new(&h.peer.transport.connection(&mut h.epochs)).unwrap();
        let activation = h.accept();
        assert!(
            content
                .queue_input(
                    &h.peer.transport.connection(&mut h.epochs),
                    h.focus,
                    tx(77),
                    NativeLauncherInputKind::Text,
                    "pending",
                    1001
                )
                .unwrap()
        );
        assert_eq!(h.activate(activation, 0).status, 1);
        if mode == 3 {
            // Inventory order is scheduling order, not revocation. Invalid
            // inventory must also leave both publication and launch owners intact.
            catalog
                .reconcile_connections(&[other_grant, GRANT], &mut h.queue)
                .unwrap();
            assert!(
                catalog
                    .reconcile_connections(&[GRANT, GRANT], &mut h.queue)
                    .is_err()
            );
            assert_eq!(h.queue.pending_len(), 1);
            assert!(catalog.publication(other_grant).is_some());
            assert!(catalog.publication(GRANT).is_some());
        }
        assert!(
            content
                .close_admitted(&mut h.peer.transport.connection(&mut h.epochs), tx(90))
                .unwrap()
        );
        assert!(
            !content
                .close_admitted(&mut h.peer.transport.connection(&mut h.epochs), tx(91))
                .unwrap()
        );
        assert_eq!(content.pending_inputs(), 0);
        h.peer.transport.poll_io(&mut h.epochs).unwrap();
        assert!(matches!(
            h.peer.read_native(&mut h.epochs).1,
            ShellNativeLauncherRecord::FocusRevoked(_)
        ));
        let (_, ShellNativeLauncherRecord::Closed(closed)) = h.peer.read_native(&mut h.epochs)
        else {
            panic!("closed")
        };
        assert_eq!(closed.reason, ContentReason::Cancelled as u16); // UI dismissal, not launch revocation
        assert_eq!(
            h.queue.pending_len(),
            1,
            "ordinary dismissal must preserve launch admission"
        );
        let mut children = vec![];
        let mut started = None;
        if mode == 2 {
            catalog
                .service_execution(
                    None,
                    &config,
                    authority,
                    &mut h.queue,
                    &mut children,
                    &mut started,
                )
                .unwrap();
            assert_eq!(h.queue.pending_len(), 1);
            catalog.reconcile_connections(&[], &mut h.queue).unwrap();
            catalog
                .service_execution(
                    None,
                    &config,
                    authority,
                    &mut h.queue,
                    &mut children,
                    &mut started,
                )
                .unwrap();
            assert_eq!(
                h.queue.pending_len(),
                0,
                "revocation must reach undispatched queue owners"
            );
            assert!(children.is_empty());
            let mut failures = vec![];
            catalog.stop(&mut h.queue, &mut failures);
            assert!(failures.is_empty());
            continue;
        }
        let intent = h.queue.begin_next(true).unwrap();
        assert!(h.queue.dispatch_catalog(intent.transaction));
        assert_eq!(catalog.execution_owner(&h.queue), Some(GRANT));
        if let Some(peer) = &mut neighbor {
            assert!(
                catalog
                    .service_execution(
                        Some(&peer.transport.connection(&mut h.epochs)),
                        &config,
                        authority,
                        &mut h.queue,
                        &mut children,
                        &mut started,
                    )
                    .is_err()
            );
            assert_eq!(catalog.execution_owner(&h.queue), Some(GRANT));
            assert!(children.is_empty());
            // Removing the unrelated peer must not cancel the selected owner.
            catalog
                .reconcile_connections(&[GRANT], &mut h.queue)
                .unwrap();
            assert!(catalog.publication(other_grant).is_none());
            assert!(catalog.publication(GRANT).is_some());
            assert_eq!(catalog.execution_owner(&h.queue), Some(GRANT));
        }
        catalog
            .service_execution(
                Some(&h.peer.transport.connection(&mut h.epochs)),
                &config,
                authority,
                &mut h.queue,
                &mut children,
                &mut started,
            )
            .unwrap();
        assert_eq!(catalog.execution_owner(&h.queue), Some(GRANT));
        if let Some(peer) = &mut neighbor {
            // The same guard protects a submitted worker result, not only the
            // still-queued dispatch. A wrong/absent borrow must not poll it.
            assert!(
                catalog
                    .service_execution(
                        Some(&peer.transport.connection(&mut h.epochs)),
                        &config,
                        authority,
                        &mut h.queue,
                        &mut children,
                        &mut started,
                    )
                    .is_err()
            );
            assert!(
                catalog
                    .service_execution(
                        None,
                        &config,
                        authority,
                        &mut h.queue,
                        &mut children,
                        &mut started,
                    )
                    .is_err()
            );
            assert_eq!(catalog.execution_owner(&h.queue), Some(GRANT));
            assert!(children.is_empty());
        }
        if revoked {
            catalog.reconcile_connections(&[], &mut h.queue).unwrap();
            catalog
                .service_execution(
                    None,
                    &config,
                    authority,
                    &mut h.queue,
                    &mut children,
                    &mut started,
                )
                .unwrap();
            assert!(h.queue.admission().is_none());
            assert!(children.is_empty());
            assert!(started.is_none());
        } else {
            while children.is_empty() {
                assert!(Instant::now() < deadline);
                catalog
                    .service_execution(
                        Some(&h.peer.transport.connection(&mut h.epochs)),
                        &config,
                        authority,
                        &mut h.queue,
                        &mut children,
                        &mut started,
                    )
                    .unwrap();
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(children.len(), 1);
            assert!(started.is_some());
            assert!(children[0].matches_admission(&h.queue));
            assert_eq!(
                children[0].native_catalog.as_ref().unwrap().cause,
                crate::session_actions::CatalogLaunchCause::Transient(activation)
            );
            assert_eq!(children[0].launch_transaction, Some(intent.transaction));
            // A later disconnect cannot erase the already executed child's origin.
            catalog.reconcile_connections(&[], &mut h.queue).unwrap();
            catalog
                .service_execution(
                    None,
                    &config,
                    authority,
                    &mut h.queue,
                    &mut children,
                    &mut started,
                )
                .unwrap();
            assert!(children[0].matches_admission(&h.queue));
            assert!(children[0].child.wait().unwrap().success());
        }
        let mut failures = vec![];
        catalog.stop(&mut h.queue, &mut failures);
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(children.len(), usize::from(!revoked));
    }
}
