//! Connection-owner assertions moved from shell_component_connections.rs at
//! 9f562e44e. The public SDK now drives the production 9P export. Admission,
//! budget, stale-attempt and real lease assertions are unchanged.
use sophia_config::ShellComponentRole;
use sophia_protocol::*;
use sophia_runtime::*;
use sophia_session::shell_component_connections::*;
use std::time::Duration;
#[path = "support/component_files.rs"]
mod component_files;
use component_files::{Harness, evidence, upload};
#[path = "support/component_budget_records.rs"]
mod budget_records;
#[path = "../../sophia-runtime/tests/support/shell_file_peer.rs"]
mod shell_file_peer;

#[test]
fn retained_launcher_bytes_tighten_replacement_while_bar_uploads() {
    let mut h = Harness::new();
    let panel = h.owner.reserve_attempt(0).unwrap();
    let mut bar = h.connect(panel);
    let menu_key = h.owner.reserve_attempt(1).unwrap();
    let mut menu = h.connect(menu_key);
    let before = h.owner.accounting();
    let output = ContentOutputId {
        id: 1,
        generation: 1,
    };
    h.owner
        .with_connection(panel, |transport| {
            assert_eq!(
                transport.content_prepared(menu_key.grant, output, 1, 1, 1, 1),
                Err(ShellTransportError::WrongContentGrant)
            );
            assert_eq!(
                transport.content_presented(menu_key.grant, output, 1, 1, 1, 1),
                Err(ShellTransportError::WrongContentGrant)
            );
            assert_eq!(
                transport.content_renderer_failed(menu_key.grant, output, 1),
                Err(ShellTransportError::WrongContentGrant)
            );
        })
        .unwrap();
    assert_eq!(h.owner.accounting(), before);
    let held = upload(&mut h, menu_key, &mut menu, 1);
    h.owner.close(menu_key).unwrap();
    let retained = h.owner.collect();
    assert_eq!(retained.retired_epochs, 1);
    assert_eq!(retained.memory.resident, 4);
    assert_eq!(retained.reserved_bytes, 40 * 1024 * 1024 + 4);
    assert_eq!(held.bytes(), &[1, 2, 3, 255]);
    let replacement = h.owner.reserve_attempt(1).unwrap();
    let _replacement = h.connect(replacement);
    let limits = h
        .owner
        .with_connection(replacement, |t| t.content_limits().unwrap().clone())
        .unwrap();
    assert_eq!(limits.max_retiring_bytes, 8 * 1024 * 1024 - 4);
    assert_eq!(limits.max_resident_bytes, 12 * 1024 * 1024);
    assert_eq!(h.owner.accounting().reserved_bytes, 64 * 1024 * 1024);
    let bar_bytes = upload(&mut h, panel, &mut bar, 1);
    assert_eq!(bar_bytes.description().grant, panel.grant);
    drop(held);
    let released = h.owner.collect();
    assert_eq!(released.retired_epochs, 0);
    assert_eq!(released.reserved_bytes, 64 * 1024 * 1024 - 4);
    assert_eq!(replacement.grant.connection_epoch, 3);
    assert_eq!(replacement.grant.content_grant_epoch, 3);
    assert_eq!(
        h.owner
            .with_connection(panel, |t| t.content_grant())
            .unwrap(),
        Some(panel.grant)
    );
    assert!(h.owner.with_connection(menu_key, |_| ()).is_err());
    drop(bar_bytes);
    h.owner.close(panel).unwrap();
    h.owner.close(replacement).unwrap();
    assert!(h.owner.collect().quiescent());
}

#[test]
fn admission_requires_exact_attempt_and_protected_role() {
    let mut h = Harness::new();
    let key = h.owner.reserve_attempt(0).unwrap();
    assert_eq!(
        h.owner.reserve_attempt(0),
        Err(ComponentConnectionError::Busy)
    );
    assert!(h.owner.with_connection(key, |_| ()).is_err());
    let forged = ComponentConnectionKey { slot: 1, ..key };
    assert_eq!(
        h.owner.close(forged),
        Err(ComponentConnectionError::StaleAttempt)
    );
    let mut wrong = evidence();
    wrong.roles.clear();
    assert!(
        h.owner
            .begin_negotiation(
                key,
                &wrong,
                Duration::from_secs(1),
                ShellContentAdmissionPolicy::Unavailable
            )
            .is_err()
    );
    assert_eq!(h.owner.phase(key), Ok(ComponentConnectionPhase::Revoked));
    assert!(h.owner.collect().quiescent());
    assert_eq!(
        h.owner.add(
            "third",
            ShellComponentRole::Bar,
            &h.directory.join("third"),
            rustix::process::geteuid().as_raw()
        ),
        Err(ComponentConnectionError::InvalidSelection)
    );
    assert!(!h.directory.join("third").exists());
}

#[test]
fn final_owner_transfer_refuses_live_admission_and_drops_the_actual_consumer() {
    let mut h = Harness::new();
    let key = h.owner.reserve_attempt(1).unwrap();
    let mut client = h.connect(key);
    let held = upload(&mut h, key, &mut client, 1);
    let held = h
        .owner
        .finish_after_backend_drop(held)
        .expect_err("live admission retains actual consumer");
    assert_eq!(held.bytes(), &[1, 2, 3, 255]);
    h.owner.close(key).unwrap();
    assert_eq!(h.owner.collect().retired_epochs, 1);
    let (settled, accounting) = h
        .owner
        .finish_after_backend_drop(held)
        .unwrap_or_else(|_| panic!("closed owner must accept final disposition"));
    assert_eq!(settled, 0);
    assert!(accounting.quiescent());
}

#[cfg(feature = "native-session")]
#[test]
fn three_roles_negotiate_independent_profiles_and_catalog_service_borrows_only_its_grant() {
    use sophia_session::shell_catalog_service::CatalogComponentService;
    use sophia_session::shell_panel_service::PanelComponentService;
    let mut h = Harness::new();
    h.owner
        .add_with_transport(
            "dock",
            ShellComponentRole::Dock,
            &h.directory.join("dock"),
            rustix::process::geteuid().as_raw(),
            sophia_config::ShellTransportSelection::NineP2000L,
        )
        .unwrap();
    let bar = h.owner.reserve_attempt(0).unwrap();
    let menu = h.owner.reserve_attempt(1).unwrap();
    let dock = h.owner.reserve_attempt(2).unwrap();
    let mut bar_peer = h.connect(bar);
    let mut menu_peer = h.connect(menu);
    let mut client = h.connect(dock);
    assert_eq!(client.welcome().selected_revision, 8);
    assert_eq!(
        client.welcome().capabilities,
        SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG
            | SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
            | SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION
            | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
            | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
    );
    let accounting = h.owner.accounting();
    let mut service = h
        .owner
        .with_connection(dock, |transport| {
            assert!(PanelComponentService::new(transport, 64, true).is_err());
            CatalogComponentService::new(transport, 64, None).unwrap()
        })
        .unwrap();
    let outputs = [sophia_engine::HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 800,
            height: 600,
        },
        scale: 1,
    }];
    let runtime = sophia_backend_live::LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    for wrong in [bar, menu] {
        h.owner
            .with_connection(wrong, |transport| {
                assert!(CatalogComponentService::new(transport, 64, None).is_err());
                assert!(service.observe_presentation(transport, &runtime).is_err());
            })
            .unwrap();
    }
    h.owner
        .with_connection(dock, |transport| {
            assert!(!service.observe_presentation(transport, &runtime).unwrap())
        })
        .unwrap();
    assert_eq!(service.grant(), dock.grant);
    assert_eq!(h.owner.accounting(), accounting);
    // The real aggregate stores retain a closed dock's consumer independently
    // while both other grants can still accept resources. No native device or
    // compositor completion is supplied by this connection-owner control.
    let dock_pixels = upload(&mut h, dock, &mut client, 1);
    h.owner.close(dock).unwrap();
    assert_eq!(h.owner.collect().retired_epochs, 1);
    let bar_pixels = upload(&mut h, bar, &mut bar_peer, 1);
    let menu_pixels = upload(&mut h, menu, &mut menu_peer, 1);
    assert_eq!(dock_pixels.bytes(), &[1, 2, 3, 255]);
    assert_eq!(bar_pixels.description().grant, bar.grant);
    assert_eq!(menu_pixels.description().grant, menu.grant);
    assert_eq!(h.owner.phase(bar), Ok(ComponentConnectionPhase::Connected));
    assert_eq!(h.owner.phase(menu), Ok(ComponentConnectionPhase::Connected));
    drop(dock_pixels);
    assert_eq!(h.owner.collect().retired_epochs, 0);
    let fresh = h.owner.reserve_attempt(2).unwrap();
    assert_ne!(fresh.grant, dock.grant);
    assert!(h.owner.with_connection(dock, |_| ()).is_err());
    h.owner.close(fresh).unwrap();
    h.owner.close(bar).unwrap();
    h.owner.close(menu).unwrap();
    assert!(!h.owner.collect().quiescent());
    drop((bar_pixels, menu_pixels));
    assert!(h.owner.collect().quiescent());
}

#[test]
fn failed_launcher_attempt_burns_epochs_without_resetting_bar() {
    let mut h = Harness::new();
    let panel = h.owner.reserve_attempt(0).unwrap();
    let _bar = h.connect(panel);
    let first = h.owner.reserve_attempt(1).unwrap();
    h.owner
        .begin_negotiation(
            first,
            &evidence(),
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: true,
            },
        )
        .unwrap();
    let mut menu =
        std::os::unix::net::UnixStream::connect(h.owner.socket_path(1).unwrap()).unwrap();
    // Invalid 9P frame size. This burns only the launcher's exact attempt.
    std::io::Write::write_all(&mut menu, &[0; 24]).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let events = loop {
        let events = h.owner.poll_negotiations(65536);
        if events.iter().any(Option::is_some) {
            break events;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "malformed frame not refused"
        );
        std::thread::yield_now();
    };
    assert_eq!(events.iter().flatten().count(), 1);
    assert!(events.into_iter().flatten().next().unwrap().1.is_err());
    assert_eq!(h.owner.phase(first), Ok(ComponentConnectionPhase::Revoked));
    assert_eq!(
        h.owner.phase(panel),
        Ok(ComponentConnectionPhase::Connected)
    );
    assert!(h.owner.poll_negotiations(65536).iter().all(Option::is_none));
    let second = h.owner.reserve_attempt(1).unwrap();
    assert!(second.grant.connection_epoch > first.grant.connection_epoch);
    assert!(second.grant.content_grant_epoch > first.grant.content_grant_epoch);
    assert_eq!(
        h.owner.close(first),
        Err(ComponentConnectionError::StaleAttempt)
    );
    let _new = h.connect(second);
    assert_eq!(h.owner.accounting().active_epochs, 2);
    h.owner.close(panel).unwrap();
    h.owner.close(second).unwrap();
    h.owner.close(second).unwrap();
    assert!(h.owner.collect().quiescent());
}

#[cfg(feature = "native-session")]
#[path = "support/component_raw_files.rs"]
mod raw_files;

#[cfg(feature = "native-session")]
#[test]
fn borrowed_native_content_places_real_wire_request_without_granting_early_focus() {
    use sophia_backend_live::{LiveProductionCpuScene, LiveProductionVisualRuntime};
    use sophia_engine::HeadlessOutput;
    use sophia_protocol::shell_files::*;
    use sophia_session::shell_native_launcher::NativeLauncherContentService;
    let mut h = Harness::new();
    let key = h.owner.reserve_attempt(1).unwrap();
    let mut client = raw_files::connect(&mut h, key, true);
    let outputs = [HeadlessOutput {
        id: OutputId::from_raw(2),
        size: Size {
            width: 800,
            height: 600,
        },
        scale: 1,
    }];
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let catalog = ShellApplicationCatalog {
        connection_epoch: key.grant.connection_epoch,
        generation: 1,
        entries: vec![],
    };
    let opening = NativeLauncherOpening {
        grant: key.grant,
        opening: 1,
        output: ContentOutputId {
            id: 2,
            generation: 1,
        },
        catalog_generation: 1,
        state_revision: 1,
    };
    let mut service = h
        .owner
        .with_connection(key, |t| {
            let service = NativeLauncherContentService::new(t).unwrap();
            t.publish_native_launcher_opening(TransactionId::from_raw(1), opening)
                .unwrap();
            t.poll_io().unwrap();
            service
        })
        .unwrap();
    assert_eq!(service.grant(), key.grant);
    raw_files::drive(
        &mut h,
        key,
        &mut client,
        |peer| {
            let event = peer.next_event();
            assert_eq!(
                decode_shell_file_native_launcher_transaction(&event, ShellFileKind::NativeOpening)
                    .unwrap()
                    .record,
                ShellNativeLauncherRecord::Opening(opening)
            );
            peer.ack(&event);
            let request = encode_shell_file_native_launcher_transaction(
                raw_files::header(key, ShellFileKind::NativeAllocationRequest, 2),
                &ShellFileNativeLauncherRecord {
                    transaction: TransactionId::from_raw(2),
                    record: ShellNativeLauncherRecord::AllocationRequest(
                        NativeLauncherAllocationRequest {
                            grant: key.grant,
                            opening: 1,
                            output: opening.output,
                            request_id: 1,
                            prior: ContentAllocationId::default(),
                            operation: 1,
                            edge: 1,
                            desired_width: 300,
                            desired_height: 100,
                            margins: ContentMargins::default(),
                        },
                    ),
                },
            )
            .unwrap();
            peer.submit_acknowledged(&request, 2);
        },
        |_| {},
    );
    let root = Rect {
        x: 0,
        y: 0,
        width: 800,
        height: 600,
    };
    let mut serial = 10;
    h.owner
        .with_connection(key, |t| {
            service
                .service_open(
                    t,
                    &catalog,
                    &mut runtime,
                    &scene,
                    None,
                    &outputs,
                    &[(outputs[0].id, root)],
                    root,
                    &mut || {
                        serial += 1;
                        Ok(TransactionId::from_raw(serial))
                    },
                )
                .unwrap();
            assert_eq!(t.native_launcher_focus(), None);
            assert!(
                t.install_native_launcher_focus(TransactionId::from_raw(20))
                    .is_err()
            );
            assert!(!service.observe_presentation(t, &runtime).unwrap());
            let allocations = t.content_allocation_snapshots();
            assert_eq!(allocations.len(), 1);
            assert_eq!(allocations[0].native_opening, Some(1));
            assert_eq!(allocations[0].logical.x, 250);
            assert_eq!(allocations[0].allowed_reservation_extent, 0);
            t.poll_io().unwrap();
        })
        .unwrap();
    let result = raw_files::drive(
        &mut h,
        key,
        &mut client,
        |peer| {
            let published = peer.next_event();
            assert_eq!(
                decode_shell_file_object_published(&published)
                    .unwrap()
                    .object,
                ShellFileKind::Outputs
            );
            peer.open(7, b"outputs", 0);
            assert!(matches!(
                decode_shell_file_outputs(&peer.read(7, 0)).unwrap().record,
                ShellContentRecord::OutputFacts(_)
            ));
            peer.ack(&published);
            let event = peer.next_event();
            let ShellContentRecord::AllocationResult(result) =
                decode_shell_file_allocation_result(&event).unwrap().record
            else {
                panic!("allocation reply missing");
            };
            peer.ack(&event);
            result
        },
        |_| {},
    );
    assert_eq!(result.status, 1);
    assert_eq!(result.grant, key.grant);
    assert_eq!((result.pixel.x, result.pixel.width), (250, 300));
    let demand_transaction = TransactionId::from_raw(913);
    raw_files::drive(
        &mut h,
        key,
        &mut client,
        |peer| {
            let demand = encode_shell_file_transaction(
                raw_files::header(key, ShellFileKind::FrameDemand, 3),
                &ShellFileTransactionRecord {
                    transaction: demand_transaction,
                    record: ShellContentRecord::FrameDemand(ContentFrameDemand {
                        grant: key.grant,
                        output: opening.output,
                        allocation: result.allocation,
                        demand_id: 1,
                        reason: 1,
                    }),
                },
            )
            .unwrap();
            peer.submit_acknowledged(&demand, 3);
        },
        |_| {},
    );
    let serial_before = serial;
    h.owner
        .with_connection(key, |t| {
            service
                .service_open(
                    t,
                    &catalog,
                    &mut runtime,
                    &scene,
                    None,
                    &outputs,
                    &[(outputs[0].id, root)],
                    root,
                    &mut || {
                        serial += 1;
                        Ok(TransactionId::from_raw(serial))
                    },
                )
                .unwrap();
            t.poll_io().unwrap();
        })
        .unwrap();
    let (permit_transaction, permit) = raw_files::drive(
        &mut h,
        key,
        &mut client,
        |peer| {
            let event = peer.next_event();
            let record = decode_shell_file_transaction(&event, ShellFileKind::FramePermit).unwrap();
            let ShellContentRecord::FramePermit(permit) = record.record else {
                panic!("permit missing");
            };
            peer.ack(&event);
            (record.transaction, permit)
        },
        |_| {},
    );
    assert_eq!(permit_transaction, demand_transaction);
    assert_eq!(
        serial, serial_before,
        "reply must not mint a server transaction"
    );
    assert_eq!((permit.demand_id, permit.state), (1, 1));
    h.owner
        .with_connection(key, |t| {
            let close_tx = TransactionId::from_raw(914);
            let mut wrong = opening;
            wrong.opening += 1;
            assert!(
                service
                    .begin_close(t, wrong, close_tx, ContentReason::Cancelled)
                    .is_err()
            );
            service
                .begin_close(t, opening, close_tx, ContentReason::Cancelled)
                .unwrap();
            assert!(
                service
                    .begin_close(
                        t,
                        opening,
                        TransactionId::from_raw(915),
                        ContentReason::Cancelled
                    )
                    .is_err()
            );
            // No native candidate was submitted. Pixel absence must not imply that
            // the active allocation or the connection's grant has been released.
            for _ in 0..2 {
                assert!(
                    service
                        .service_close_pixels(t, &mut runtime, &scene, None)
                        .unwrap()
                );
                assert_eq!(t.content_allocation_snapshots().len(), 1);
                assert_eq!(t.content_grant(), Some(key.grant));
            }
            assert!(!t.closed_native_owners_settled(opening).unwrap());
            let mut next = opening;
            next.opening += 1;
            assert!(
                !service
                    .reopen(t, TransactionId::from_raw(930), next)
                    .unwrap()
            );
            let mut invalidations = 0;
            for _ in 0..2 {
                assert!(
                    service
                        .service_close_if_requested(t, &mut runtime, &scene, None, &mut || {
                            invalidations += 1;
                            Ok(TransactionId::from_raw(920))
                        })
                        .unwrap()
                        == Some(true)
                );
                assert!(t.content_allocation_snapshots().is_empty());
            }
            assert_eq!(
                invalidations, 1,
                "repeat must not enqueue another invalidation"
            );
            assert!(
                service
                    .service_open(
                        t,
                        &catalog,
                        &mut runtime,
                        &scene,
                        None,
                        &outputs,
                        &[(outputs[0].id, root)],
                        root,
                        &mut || Ok(TransactionId::from_raw(916)),
                    )
                    .is_err()
            );
            assert!(
                service
                    .reopen(t, TransactionId::from_raw(930), next)
                    .unwrap()
            );
            assert_eq!(t.native_launcher_state().unwrap().0, next);
            assert!(
                service
                    .begin_close(t, opening, close_tx, ContentReason::Cancelled)
                    .is_err()
            );
            service
                .service_open(
                    t,
                    &catalog,
                    &mut runtime,
                    &scene,
                    None,
                    &outputs,
                    &[(outputs[0].id, root)],
                    root,
                    &mut || Ok(TransactionId::from_raw(931)),
                )
                .unwrap();
        })
        .unwrap();
    h.owner.close(key).unwrap();
    h.owner.collect();
    let replacement = h.owner.reserve_attempt(1).unwrap();
    let _new_client = h.connect(replacement);
    h.owner
        .with_connection(replacement, |t| {
            assert!(service.observe_presentation(t, &runtime).is_err());
            assert_eq!(
                NativeLauncherContentService::new(t).unwrap().grant(),
                replacement.grant
            );
        })
        .unwrap();
    h.owner.close(replacement).unwrap();
    assert!(h.owner.collect().quiescent());
}

#[cfg(feature = "native-session")]
#[test]
fn borrowed_panel_service_uses_the_shared_registry_and_refuses_another_attempt() {
    use sophia_protocol::shell_files::*;
    use sophia_session::shell_panel_service::PanelComponentService;
    let mut h = Harness::new();
    let panel = h.owner.reserve_attempt(0).unwrap();
    let native = h.owner.reserve_attempt(1).unwrap();
    let mut client = raw_files::connect(&mut h, panel, false);
    let mut neighbor = raw_files::connect(&mut h, native, true);
    let accounting = h.owner.accounting();
    let mut service = h
        .owner
        .with_connection(panel, |t| {
            // The operator request cannot upgrade the actual negotiated grant.
            assert!(PanelComponentService::new(t, 30, true).is_err());
            assert!(PanelComponentService::new(t, 0, false).is_err());
            PanelComponentService::new(t, 30, false).unwrap()
        })
        .unwrap();
    assert_eq!(service.grant(), panel.grant);
    let publication = sophia_engine::PolicyIndicatorPublication {
        tab_groups: vec![],
        generation: 1,
        connection_epoch: Some(20),
        indicators: vec![],
        output_statuses: vec![],
    };
    h.owner
        .with_connection(native, |t| {
            assert!(PanelComponentService::new(t, 30, false).is_err());
            assert!(
                service
                    .service_indicators(t, Some(&publication), None)
                    .is_err()
            );
        })
        .unwrap();
    h.owner
        .with_connection(panel, |t| {
            service
                .service_indicators(t, Some(&publication), None)
                .unwrap();
            t.poll_io().unwrap();
        })
        .unwrap();
    let snapshot = sophia_session::shell_indicator_publication::indicator_snapshot(
        &publication,
        None,
        panel.grant.connection_epoch,
    );
    raw_files::drive(
        &mut h,
        panel,
        &mut client,
        |peer| {
            let event = peer.next_event();
            assert_eq!(
                decode_shell_file_object_published(&event).unwrap().object,
                ShellFileKind::Indicators
            );
            peer.open(7, b"indicators", 0);
            let received = decode_shell_file_indicators(&peer.read(7, 0)).unwrap();
            assert_eq!(received.transaction, TransactionId::from_raw(1));
            assert_eq!(received.snapshot, snapshot);
            peer.ack(&event);
        },
        |_| {},
    );
    // Unchanged snapshots enqueue nothing and the native peer receives no panel frames.
    h.owner
        .with_connection(panel, |t| {
            service
                .service_indicators(t, Some(&publication), None)
                .unwrap();
            t.poll_io().unwrap();
        })
        .unwrap();
    // A fresh custody record is an ordered journal barrier. Any duplicate or
    // cross-role publication would precede it and fail submit_acknowledged.
    // The marker stays in the export; no resource owner is serviced here.
    for (key, peer) in [(panel, &mut client), (native, &mut neighbor)] {
        raw_files::drive(
            &mut h,
            key,
            peer,
            |peer| {
                let marker = encode_shell_file_resource_retire(
                    raw_files::header(key, ShellFileKind::ResourceRetire, 2),
                    &ShellFileTransactionRecord {
                        transaction: TransactionId::from_raw(999),
                        record: ShellContentRecord::ResourceRetire(ContentResourceRetire {
                            grant: key.grant,
                            resource: ContentResourceId {
                                id: 999,
                                generation: 1,
                            },
                        }),
                    },
                )
                .unwrap();
                peer.submit_acknowledged(&marker, 2);
            },
            |_| {},
        );
    }
    assert_eq!(h.owner.accounting(), accounting);
    h.owner.close(panel).unwrap();
    h.owner.collect();
    let replacement = h.owner.reserve_attempt(0).unwrap();
    let _new_client = h.connect(replacement);
    h.owner
        .with_connection(replacement, |t| {
            assert!(
                service
                    .service_indicators(t, Some(&publication), None)
                    .is_err()
            );
            assert_eq!(
                PanelComponentService::new(t, 30, false).unwrap().grant(),
                replacement.grant
            );
        })
        .unwrap();
    h.owner.close(replacement).unwrap();
    h.owner.close(native).unwrap();
    assert!(h.owner.collect().quiescent());
}

#[test]
fn three_role_inventory_is_frozen_before_the_first_budget_reservation() {
    let mut h = Harness::new();
    let uid = rustix::process::geteuid().as_raw();
    let dock = h
        .owner
        .add_with_transport(
            "dock",
            ShellComponentRole::Dock,
            &h.directory.join("dock"),
            uid,
            sophia_config::ShellTransportSelection::NineP2000L,
        )
        .unwrap();
    assert_eq!(dock, 2);
    let bar = h.owner.reserve_attempt(0).unwrap();
    assert_eq!(h.owner.accounting().reserved_bytes, 24 * 1024 * 1024);
    let menu = h.owner.reserve_attempt(1).unwrap();
    let dock = h.owner.reserve_attempt(2).unwrap();
    assert_eq!(h.owner.accounting().active_epochs, 3);
    assert_eq!(h.owner.accounting().reserved_bytes, 64 * 1024 * 1024);
    let _bar_socket = h.connect(bar);
    let _menu_socket = h.connect(menu);
    // The persistent role cannot negotiate as a bar or transient menu.
    h.owner
        .begin_negotiation(
            dock,
            &evidence(),
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: true,
            },
        )
        .unwrap();
    let socket = h.owner.socket_path(dock.slot).unwrap().to_owned();
    let peer = std::thread::spawn(move || {
        sophia_shell_client::ShellConnection::connect_files(
            socket,
            sophia_shell_client::ShellClientOptions {
                minimum_revision: 5,
                maximum_revision: 6,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
                handshake_timeout: Duration::from_secs(2),
            },
        )
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let refused = loop {
        let events = h.owner.poll_negotiations(65536);
        if events.iter().any(Option::is_some) {
            break events;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "wrong dock profile not refused"
        );
        std::thread::yield_now();
    };
    assert!(peer.join().unwrap().is_err());
    assert!(
        refused
            .into_iter()
            .flatten()
            .any(|(key, result)| key == dock && result.is_err())
    );
    assert_eq!(h.owner.phase(bar), Ok(ComponentConnectionPhase::Connected));
    assert_eq!(h.owner.phase(menu), Ok(ComponentConnectionPhase::Connected));
    h.owner.close(bar).unwrap();
    h.owner.close(menu).unwrap();
    assert!(h.owner.collect().quiescent());

    let mut late = Harness::new();
    let key = late.owner.reserve_attempt(0).unwrap();
    late.owner.close(key).unwrap();
    let path = late.directory.join("late");
    assert_eq!(
        late.owner.add("late", ShellComponentRole::Dock, &path, uid),
        Err(ComponentConnectionError::InvalidSelection)
    );
    assert!(!path.exists());
}

#[path = "support/component_file_negotiation.rs"]
mod file_negotiation;

#[test]
fn content_connection_owner_cannot_admit_descriptor_authority() {
    let mut harness = Harness::new();
    let endpoint = harness.directory.join("descriptor");
    for explicit in [false, true] {
        let result = if explicit {
            harness.owner.add_with_transport(
                "metadata",
                ShellComponentRole::Descriptor,
                &endpoint,
                rustix::process::geteuid().as_raw(),
                sophia_config::ShellTransportSelection::NineP2000L,
            )
        } else {
            harness.owner.add(
                "metadata",
                ShellComponentRole::Descriptor,
                &endpoint,
                rustix::process::geteuid().as_raw(),
            )
        };
        assert_eq!(result, Err(ComponentConnectionError::InvalidSelection));
        assert!(!endpoint.exists());
    }
}

#[path = "support/component_publication_files.rs"]
mod publication_files;
