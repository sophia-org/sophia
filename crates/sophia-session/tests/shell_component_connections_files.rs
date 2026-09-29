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
