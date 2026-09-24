#[test]
fn client_resource_cleanup_releases_colors_without_freeing_a_peers_references() {
    let state = X11CoreSocketServerState::new();
    let namespace = NamespaceId::from_raw(212);
    let map = XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_COLORMAP), 1);
    let owned = XResourceId::new(0x200001, 1);
    {
        let mut runtime = state.runtime.lock().unwrap();
        runtime
            .create_colormap(namespace, owned, crate::X_SETUP_DEFAULT_VISUAL, 1)
            .unwrap();
        for client in [1, 2] {
            for colormap in [map, owned] {
                runtime.allocate_color(namespace, client, colormap, 0x123456);
            }
        }
    }
    release_x11_client_lease_with_control(
        &state,
        namespace,
        XServerFrontendClientLease {
            client: XServerFrontendClientId(1),
            resource_id_range: crate::XWireClientResourceRange {
                base: 0x200000,
                mask: 0xffff,
            },
            close_down_mode: crate::XCloseDownMode::Destroy,
        },
        &[],
        None,
    )
    .unwrap();
    let mut runtime = state.runtime.lock().unwrap();
    assert_eq!(
        runtime.free_colors(namespace, 1, map, 0, &[0x123456]),
        Some((crate::XErrorCode::BadAccess, 0))
    );
    assert_eq!(runtime.free_colors(namespace, 2, map, 0, &[0x123456]), None);
    // Destruction of the owned map must remove the peer's references too:
    // recreating that XID must not revive allocations on the old map.
    runtime
        .create_colormap(namespace, owned, crate::X_SETUP_DEFAULT_VISUAL, 2)
        .unwrap();
    assert_eq!(
        runtime.free_colors(namespace, 2, owned, 0, &[0x123456]),
        Some((crate::XErrorCode::BadAccess, 0))
    );
}
