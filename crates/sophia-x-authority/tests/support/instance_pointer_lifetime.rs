use crate::{XInputAuthorityState, XPointerObservation, XResourceId};
use sophia_protocol::{NamespaceId, SurfaceId};

#[test]
fn a_private_instance_keeps_root_position_but_retires_the_last_clients_query_scope() {
    let namespace = NamespaceId::from_raw(11);
    let root = XResourceId::new(1, 1);
    let mut authority = XInputAuthorityState::default();
    authority.prepare_ordered_namespace(namespace);
    authority.retain_instance_pointer(namespace, root);
    authority.register_query_client(namespace, 7);
    authority.warp_query_pointer(
        namespace,
        XPointerObservation {
            surface_window: XResourceId::new(71, 1),
            surface: SurfaceId::new(71, 1),
            root_x: 317,
            root_y: 219,
            local_x: 17,
            local_y: 19,
        },
    );
    authority.observe_query_modifiers(namespace, 8);
    let old_scope = authority.ordered_query_scope(namespace).unwrap();
    authority.cleanup_ordered_owner(namespace, 7);

    assert!(old_scope.retired());
    assert!(!authority.query_namespace_active(namespace));
    let query = authority.pointer_query_state(namespace);
    assert_eq!(query.mask, 0);
    let position = query.position.unwrap();
    assert_eq!(position.surface_window, root);
    assert_eq!(position.surface, crate::ROOT_POINTER_SURFACE);
    assert_eq!((position.root_x, position.root_y), (317, 219));
    assert_eq!((position.local_x, position.local_y), (317, 219));

    authority.register_query_client(namespace, 8);
    assert!(!authority.ordered_query_scope(namespace).unwrap().retired());
    assert!(
        old_scope.retired(),
        "a new client cannot revive an old scope"
    );
    assert_eq!(
        authority
            .pointer_query_state(namespace)
            .position
            .unwrap()
            .root_x,
        317
    );
}

#[test]
fn ordinary_namespaces_still_drop_the_last_clients_query_state() {
    let namespace = NamespaceId::from_raw(12);
    let mut authority = XInputAuthorityState::default();
    authority.register_query_client(namespace, 7);
    authority.warp_query_pointer(
        namespace,
        XPointerObservation {
            surface_window: XResourceId::new(71, 1),
            surface: SurfaceId::new(71, 1),
            root_x: 317,
            root_y: 219,
            local_x: 17,
            local_y: 19,
        },
    );
    let scope = authority.ordered_query_scope(namespace).unwrap();
    authority.cleanup_ordered_owner(namespace, 7);
    assert!(scope.retired());
    assert!(!authority.has_ordered_namespace(namespace));
    assert!(authority.pointer_query_state(namespace).position.is_none());
}
