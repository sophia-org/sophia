//! `sophia_shell_fs_v1` twins of `tests/shell_component_transport.rs`: several
//! real component file exports share one `ContentEpochRegistry`, each with the
//! SDK's `connect_files` client. Every assertion of the socket cases is kept.
//! The socket handshake's "no Welcome bytes" check becomes its file-wire
//! analogue: the client's connect fails and no Limits reach it.
#[path = "support/file_component.rs"]
mod file_component;

use std::time::Duration;

use file_component::*;
use sophia_protocol::*;
use sophia_runtime::*;
use sophia_shell_client::ShellClientError;

fn limits(epoch: u64, launcher: bool) -> ContentLimits {
    let mut limits = ContentLimits::prototype(ContentGrant {
        connection_epoch: epoch,
        content_grant_epoch: epoch,
    });
    if launcher {
        limits.max_staging_bytes = 4 * MIB;
        limits.max_resident_bytes = 12 * MIB;
        limits.max_retiring_bytes = 8 * MIB;
    }
    limits
}

fn resource(id: u64) -> ContentResourceId {
    ContentResourceId { id, generation: 1 }
}

#[test]
fn two_file_connections_share_actual_stores_and_keep_neighbor_live_through_retirement() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut a = Component::new();
    let mut b = Component::new();
    let bar = limits(1, false);
    let menu = limits(2, true);
    a.transport
        .reserve_content(&mut registry, bar.clone())
        .unwrap();
    b.transport
        .reserve_content(&mut registry, menu.clone())
        .unwrap();
    assert_eq!(registry.reserved_bytes(), 64 * MIB); // before either peer connects
    assert!(!a.transport.supports_content());
    assert!(!b.transport.supports_content());
    a.connect(&mut registry, bar.clone());
    b.connect(&mut registry, menu.clone());
    a.send_upload(bar.grant, 1, 30);
    b.send_upload(menu.grant, 1, 70);
    let a_pixels = a.uploaded(&mut registry, bar.grant, 1, 0);
    let b_pixels = b.uploaded(&mut registry, menu.grant, 1, 0);
    assert_eq!(a_pixels.bytes(), [30, 0, 0, 255]);
    assert_eq!(b_pixels.bytes(), [70, 0, 0, 255]);
    assert!(
        a.transport
            .lease_content_resource(&registry, menu.grant, resource(1))
            .is_err()
    );
    // Control: the same lease through the owning transport succeeds.
    drop(
        b.transport
            .lease_content_resource(&registry, menu.grant, resource(1))
            .unwrap(),
    );
    assert_eq!(registry.accounting().resources, 2);

    b.disconnect(&mut registry);
    assert_eq!(registry.retired_bytes(), 4);
    let replacement = limits(3, true);
    assert!(matches!(
        b.transport
            .reserve_content(&mut registry, replacement.clone()),
        Err(ShellTransportError::ContentStore(ContentStoreError::Budget))
    ));
    assert_eq!(a.transport.content_grant(), Some(bar.grant));
    a.send_upload(bar.grant, 2, 90);
    let next_a = a.uploaded(&mut registry, bar.grant, 2, 1);
    assert_eq!(next_a.bytes(), [90, 0, 0, 255]);
    assert_eq!(registry.retired_bytes(), 4);

    drop(b_pixels);
    registry.collect();
    assert_eq!(registry.retired_bytes(), 0);
    b.transport
        .reserve_content(&mut registry, replacement.clone())
        .unwrap();
    // A fresh protected-peer authorization is required for the new connection.
    b.transport.authorize_protected_peer(&evidence()).unwrap();
    b.connect(&mut registry, replacement);
    assert_eq!(a.transport.content_grant(), Some(bar.grant));
    assert_eq!(a_pixels.bytes(), [30, 0, 0, 255]);
    drop((a_pixels, next_a));
    a.disconnect(&mut registry);
    b.disconnect(&mut registry);
    registry.collect();
    assert!(registry.accounting().quiescent());
}

#[test]
fn file_peer_cannot_route_a_foreign_grant_into_its_neighbors_store() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut a = Component::new();
    let mut b = Component::new();
    let bar = limits(1, false);
    let menu = limits(2, true);
    a.transport
        .reserve_content(&mut registry, bar.clone())
        .unwrap();
    b.transport
        .reserve_content(&mut registry, menu.clone())
        .unwrap();
    a.connect(&mut registry, bar.clone());
    b.connect(&mut registry, menu.clone());
    a.send_upload(menu.grant, 1, 30);
    let before = registry.accounting();
    // The file wire delivers the submission over later turns; serve until the
    // owner reaches a verdict, driving the client's pipelined writes too.
    let start = std::time::Instant::now();
    let verdict = loop {
        a.client.as_mut().unwrap().poll_io().unwrap();
        match a.transport.service_content_resources(&mut registry, 0) {
            Ok(_) => {}
            Err(error) => break error,
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "foreign grant was never refused"
        );
        std::thread::yield_now();
    };
    assert_eq!(verdict, ShellTransportError::WrongContentGrant);
    assert_eq!(registry.accounting(), before);
    assert!(
        a.transport
            .lease_content_resource(&registry, menu.grant, resource(1))
            .is_err()
    );
    a.disconnect(&mut registry);
    b.send_upload(menu.grant, 1, 70);
    let pixels = b.uploaded(&mut registry, menu.grant, 1, 0);
    assert_eq!(pixels.bytes(), [70, 0, 0, 255]);
    drop(pixels);
    b.disconnect(&mut registry);
    assert!(registry.accounting().quiescent());
}

#[test]
fn failed_file_handshake_revokes_exact_prelaunch_reservation_not_neighbor() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut a = Component::new();
    let mut b = Component::new();
    let bar = limits(1, false);
    let menu = limits(2, true);
    a.transport
        .reserve_content(&mut registry, bar.clone())
        .unwrap();
    a.connect(&mut registry, bar.clone());
    b.transport.reserve_content(&mut registry, menu).unwrap();
    // A reserved component whose client requests no content surface.
    let (owner, client) = b.negotiate(
        &mut registry,
        2,
        SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER,
    );
    assert_eq!(owner, Err(ShellTransportError::MissingCapability));
    // No Negotiated (and so no Limits) for a missing required content request.
    let Err(error) = client else {
        panic!("client connected without the required content request");
    };
    // Observed: the owner closes the lane with no Negotiated or refusal
    // record, so the client's pipeline ends (BrokenPipe or a reset).
    assert!(
        matches!(error, ShellClientError::Pipeline(_)),
        "expected the connection to end without a reply: {error:?}"
    );
    assert_eq!(registry.reserved_bytes(), 40 * MIB);
    assert_eq!(registry.accounting().active_epochs, 1);
    assert_eq!(a.transport.content_grant(), Some(bar.grant));
    assert!(!b.transport.supports_content());
    b.disconnect(&mut registry);
    assert_eq!(registry.accounting().active_epochs, 1);
    // The neighbor still uploads after the failed handshake.
    a.send_upload(bar.grant, 1, 30);
    drop(a.uploaded(&mut registry, bar.grant, 1, 0));
    a.disconnect(&mut registry);
    assert!(registry.accounting().quiescent());
}

#[test]
fn unreserved_file_negotiation_uses_common_epoch_after_a_reserved_component() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut a = Component::new();
    let mut b = Component::new();
    let small = limits(1, true);
    a.transport
        .reserve_content(&mut registry, small.clone())
        .unwrap();
    a.connect(&mut registry, small.clone());
    // No pre-reservation: the registry supplies content epoch 2, even though
    // this new transport has never negotiated. This is current behavior kept
    // under test until its retirement design is settled.
    b.connect(&mut registry, limits(2, false));
    assert_eq!(registry.reserved_bytes(), 64 * MIB);
    assert_eq!(a.transport.content_grant(), Some(small.grant));
    a.disconnect(&mut registry);
    b.disconnect(&mut registry);
    assert!(registry.accounting().quiescent());
}

#[test]
fn refused_replacement_and_wrong_file_connection_preserve_prelaunch_owner() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut a = Component::new();
    let selected = limits(1, false);
    a.transport
        .reserve_content(&mut registry, selected.clone())
        .unwrap();
    let before = registry.accounting();
    assert_eq!(
        a.transport.reserve_content(&mut registry, limits(2, true)),
        Err(ShellTransportError::WrongContentGrant)
    );
    assert_eq!(
        a.transport
            .begin_file_negotiation(&registry, 2, Duration::ZERO, granted()),
        Err(ShellTransportError::WrongContentGrant)
    );
    assert_eq!(registry.accounting(), before);
    a.connect(&mut registry, selected);
    a.disconnect(&mut registry);
    a.disconnect(&mut registry);
    assert!(registry.accounting().quiescent());
}

#[test]
fn explicit_three_owner_capacity_over_files_preserves_budget_and_neighbor_progress() {
    assert!(ContentEpochRegistry::with_active_capacity(64 * MIB, 0).is_err());
    assert!(ContentEpochRegistry::with_active_capacity(64 * MIB, 4).is_err());
    let bounded = |epoch| {
        let mut value = limits(epoch, true);
        value.max_staging_bytes = 4 * MIB;
        value.max_resident_bytes = if epoch == 1 { 12 * MIB } else { 8 * MIB };
        value.max_retiring_bytes = 8 * MIB;
        value
    };
    // Two bounded profiles total 40 MiB, so a third (64 MiB total) fits the
    // bytes: the default registry's refusal is its two-epoch active cap.
    let mut legacy = ContentEpochRegistry::new(64 * MIB).unwrap();
    legacy.admit(bounded(1)).unwrap();
    legacy.admit(bounded(2)).unwrap();
    assert_eq!(legacy.admit(bounded(3)), Err(ContentStoreError::Budget));
    let mut three = ContentEpochRegistry::with_active_capacity(64 * MIB, 3).unwrap();
    for epoch in 1..=3 {
        three.admit(bounded(epoch)).unwrap();
    }

    let mut registry = ContentEpochRegistry::with_active_capacity(64 * MIB, 3).unwrap();
    let mut peers: [_; 3] = std::array::from_fn(|_| Component::new());
    let mut held = Vec::new();
    for (index, peer) in peers.iter_mut().enumerate() {
        let budget = bounded(index as u64 + 1);
        peer.transport
            .reserve_content(&mut registry, budget.clone())
            .unwrap();
        peer.connect(&mut registry, budget.clone());
        peer.send_upload(budget.grant, 1, 30 + index as u8);
        held.push(peer.uploaded(&mut registry, budget.grant, 1, 0));
    }
    assert_eq!(registry.reserved_bytes(), 64 * MIB);
    assert_eq!(registry.admit(bounded(4)), Err(ContentStoreError::Budget));
    peers[2].disconnect(&mut registry);
    assert_eq!(registry.retired_bytes(), 4);
    assert_eq!(registry.admit(bounded(4)), Err(ContentStoreError::Budget));
    for (index, peer) in peers[..2].iter_mut().enumerate() {
        let grant = bounded(index as u64 + 1).grant;
        peer.send_upload(grant, 2, 80 + index as u8);
        let pixels = peer.uploaded(&mut registry, grant, 2, 1);
        assert_eq!(pixels.bytes(), [80 + index as u8, 0, 0, 255]);
    }
    drop(held.pop());
    registry.collect();
    assert_eq!(registry.retired_bytes(), 0);
    registry.admit(bounded(4)).unwrap();
    assert_eq!(registry.reserved_bytes(), 64 * MIB);
    assert!(registry.resources(bounded(3).grant).is_none());
    assert!(registry.disconnect(bounded(4).grant));
    for peer in &mut peers[..2] {
        peer.disconnect(&mut registry);
    }
    drop(held);
    registry.collect();
    assert!(registry.accounting().quiescent());
}
