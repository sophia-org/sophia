// Real connection reservations and registry charges; no client or display launch.
use super::*;
use sophia_runtime::OwnedRingGeometry;

#[test]
fn every_configured_role_can_register_a_default_ring_without_raising_global_budget() {
    for has_dock in [false, true] {
        let root = std::env::temp_dir().join(format!(
            "sophia-ring-headroom-{}-{has_dock}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let mut owner = ShellComponentConnections::new().unwrap();
        let mut keys = Vec::new();
        let mut sources = Vec::new();
        let roles = [
            ShellComponentRole::Bar,
            ShellComponentRole::ApplicationLauncher,
            ShellComponentRole::Dock,
        ];
        for (slot, role) in roles
            .into_iter()
            .take(if has_dock { 3 } else { 2 })
            .enumerate()
        {
            owner
                .add_with_transport(
                    &format!("role{slot}"),
                    role,
                    &root.join(slot.to_string()),
                    rustix::process::geteuid().as_raw(),
                    ShellTransportSelection::NineP2000L,
                )
                .unwrap();
        }
        // Reserve every resource envelope before registering any ring, matching
        // simultaneous component startup rather than a convenient serial order.
        for slot in 0..owner.connections.len() {
            keys.push(owner.reserve_attempt(slot).unwrap());
        }
        let resources = owner.accounting().reserved_bytes;
        assert_eq!(resources, if has_dock { 51 * MIB } else { 48 * MIB });
        let geometry = OwnedRingGeometry::new(1, (4 * MIB) as usize).unwrap();
        for key in &keys {
            sources.push(
                owner
                    .epochs
                    .reserve_owned_ring_source(key.grant, geometry)
                    .unwrap(),
            );
        }
        let charged = owner.accounting();
        assert_eq!(
            charged.source_bytes,
            keys.len() as u64 * geometry.source_bytes() as u64
        );
        assert_eq!(charged.reserved_bytes, resources + charged.source_bytes);
        assert!(charged.reserved_bytes <= 64 * MIB);
        assert!(charged.reserved_backing_bytes <= 64 * MIB);
        assert!(matches!(
            owner.epochs.reserve_owned_ring_source(
                keys[0].grant,
                OwnedRingGeometry::new(4, (4 * MIB) as usize).unwrap()
            ),
            Err(ContentStoreError::Budget)
        ));
        assert_eq!(owner.accounting(), charged);

        owner.close(keys[0]).unwrap();
        assert_eq!(owner.collect().retired_epochs, 1); // source custody survives close
        if has_dock {
            assert!(matches!(
                owner.reserve_attempt(0),
                Err(ComponentConnectionError::Transport(
                    ShellTransportError::ContentStore(ContentStoreError::Budget)
                ))
            ));
        }
        drop(sources.remove(0));
        assert_eq!(owner.collect().retired_epochs, 0);
        let replacement = owner.reserve_attempt(0).unwrap();
        assert_ne!(replacement.grant, keys[0].grant);
        sources.push(
            owner
                .epochs
                .reserve_owned_ring_source(replacement.grant, geometry)
                .unwrap(),
        );
        assert_eq!(owner.accounting().reserved_bytes, charged.reserved_bytes);
        for key in keys.into_iter().skip(1).chain([replacement]) {
            owner.close(key).unwrap();
        }
        drop(sources);
        assert!(owner.collect().quiescent());
        drop(owner);
        std::fs::remove_dir_all(root).unwrap();
    }
}
