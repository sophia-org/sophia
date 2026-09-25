use super::*;
use crate::live_session::PersistentLiveLayout;
use sophia_protocol::{SurfaceConstraints, TransactionId};

#[path = "../presentation_owner_visibility.rs"]
mod presentation_owner_visibility;

#[test]
fn client_positioned_role_transition_withdraws_surface_from_wm_policy() {
    let surface = SurfaceId::new(44, 1);
    let geometry = Rect {
        x: 10,
        y: 20,
        width: 640,
        height: 480,
    };
    let mut request = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(40));
    request
        .presentation_intents
        .push(sophia_protocol::SurfacePresentationIntent {
            surface,
            kind: sophia_protocol::SurfacePresentationIntentKind::Request,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            surface_kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            presentation_owner: None,
            stack_rank: 0,
            geometry,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        });
    let mut layout = PersistentLiveLayout::default();
    layout.observe_authority_batch(&request);
    assert_eq!(layout.next_unmanaged_surface(), Some(surface));

    let mut reparent =
        crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(41));
    reparent.surface_presentations.push(
        sophia_x_authority::XAuthoritySurfacePresentationObservation {
            surface,
            role: sophia_protocol::SurfacePresentationRole::ClientPositioned,
            kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            owner: None,
            stack_rank: 0,
            mapped: true,
            geometry,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 2,
        },
    );

    let observation = layout.observe_authority_batch(&reparent);

    assert_eq!(observation.withdrawn_surfaces, vec![surface]);
    assert_eq!(layout.next_unmanaged_surface(), None);
    assert!(!layout.is_policy_managed(surface));
}

#[test]
fn attached_surface_keeps_removed_owner_as_a_non_visible_dependency() {
    let owner = SurfaceId::new(45, 1);
    let dialog = SurfaceId::new(46, 1);
    let geometry = Rect {
        x: 20,
        y: 30,
        width: 480,
        height: 320,
    };
    let mut batch = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(42));
    for (surface, role, presentation_owner) in [
        (
            owner,
            sophia_protocol::SurfacePresentationRole::PolicyManaged,
            None,
        ),
        (
            dialog,
            sophia_protocol::SurfacePresentationRole::ClientPositioned,
            Some(owner),
        ),
    ] {
        batch.surface_presentations.push(
            sophia_x_authority::XAuthoritySurfacePresentationObservation {
                surface,
                role,
                kind: sophia_protocol::LayoutNodeKind::Toplevel,
                placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
                owner: presentation_owner,
                stack_rank: 0,
                mapped: true,
                geometry,
                constraints: SurfaceConstraints {
                    min_size: None,
                    max_size: None,
                },
                generation: 1,
            },
        );
    }
    let mut layout = PersistentLiveLayout::default();
    layout.observe_authority_batch(&batch);
    assert_eq!(layout.presentation_owner(dialog), Some(owner));

    let mut removal = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(43));
    removal.removed_surfaces.push(owner);
    layout.observe_authority_batch(&removal);

    assert!(!layout.knows_surface(owner));
    assert_eq!(layout.presentation_owner(dialog), Some(owner));
    assert!(layout.client_positioned_mapped(dialog));
}
