use super::*;
use crate::live_session::PendingLiveWmLayout;
use sophia_protocol::{LayoutNodeKind, SurfacePlacementPreference, SurfacePresentationRole};
use sophia_protocol::{TransactionCommit, TransactionOutcome};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

fn observe(
    layout: &mut PersistentLiveLayout,
    surface: SurfaceId,
    role: SurfacePresentationRole,
    owner: Option<SurfaceId>,
    mapped: bool,
) {
    let mut batch = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(70));
    batch.surface_presentations.push(
        sophia_x_authority::XAuthoritySurfacePresentationObservation {
            surface,
            role,
            kind: LayoutNodeKind::Toplevel,
            placement_preference: SurfacePlacementPreference::Default,
            owner,
            stack_rank: 0,
            mapped,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 240,
                height: 112,
            },
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    );
    layout.observe_authority_batch(&batch);
}

#[test]
fn panel_popups_inherit_mapping_without_requesting_wm_placement() {
    let panel = SurfaceId::new(70, 1);
    let popup = SurfaceId::new(71, 1);
    let nested = SurfaceId::new(72, 1);
    let mut layout = PersistentLiveLayout::default();
    for (surface, owner) in [(panel, None), (popup, Some(panel)), (nested, Some(popup))] {
        observe(
            &mut layout,
            surface,
            SurfacePresentationRole::ClientPositioned,
            owner,
            true,
        );
    }
    let visible = |layout: &PersistentLiveLayout, surface| {
        layout
            .client_positioned_visible::<()>(surface, |_| panic!("panel has no WM placement"))
            .unwrap()
    };
    assert!(visible(&layout, nested));
    observe(
        &mut layout,
        panel,
        SurfacePresentationRole::ClientPositioned,
        None,
        false,
    );
    assert!(!visible(&layout, nested));
    observe(
        &mut layout,
        panel,
        SurfacePresentationRole::ClientPositioned,
        None,
        true,
    );
    assert!(visible(&layout, popup));

    let mut removal = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(71));
    removal.removed_surfaces.push(panel);
    layout.observe_authority_batch(&removal);
    observe(
        &mut layout,
        SurfaceId::new(70, 2),
        SurfacePresentationRole::ClientPositioned,
        None,
        true,
    );
    assert!(!visible(&layout, popup));
}

#[test]
fn nested_popup_visibility_reaches_managed_owner_and_propagates_errors() {
    let managed = SurfaceId::new(80, 1);
    let popup = SurfaceId::new(81, 1);
    let nested = SurfaceId::new(82, 1);
    let mut layout = PersistentLiveLayout::default();
    observe(
        &mut layout,
        managed,
        SurfacePresentationRole::PolicyManaged,
        None,
        true,
    );
    observe(
        &mut layout,
        popup,
        SurfacePresentationRole::ClientPositioned,
        Some(managed),
        true,
    );
    observe(
        &mut layout,
        nested,
        SurfacePresentationRole::ClientPositioned,
        Some(popup),
        true,
    );
    for visible in [false, true] {
        assert_eq!(
            layout.client_positioned_visible::<()>(nested, |owner| {
                assert_eq!(owner, managed);
                Ok(visible)
            }),
            Ok(visible)
        );
    }
    assert_eq!(
        layout.client_positioned_visible(nested, |_| Err("policy unavailable")),
        Err("policy unavailable")
    );
}

#[test]
fn cyclic_popup_ownership_is_not_visible() {
    let first = SurfaceId::new(90, 1);
    let second = SurfaceId::new(91, 1);
    let mut layout = PersistentLiveLayout::default();
    for (surface, owner) in [(first, second), (second, first)] {
        observe(
            &mut layout,
            surface,
            SurfacePresentationRole::ClientPositioned,
            Some(owner),
            true,
        );
    }
    assert!(
        !layout
            .client_positioned_visible::<()>(first, |_| panic!("cycle has no managed ancestor"))
            .unwrap()
    );
}

/// An unmapped managed window leaves the scene before policy agrees.
///
/// Policy projects a layout that is not refreshed in the step carrying an
/// unmap, so a window torn down this cycle is still projected as visible. A
/// captured session showed the consequence: six Thunar menu windows reported
/// `mapped=0` by `GetWindowAttributes` while still present in the scene. Being
/// listed as a child of the root is not being mapped.
#[test]
fn an_unmapped_managed_surface_leaves_the_scene_though_policy_still_projects_it() {
    let managed = SurfaceId::new(100, 1);
    let mut layout = PersistentLiveLayout::default();
    observe(
        &mut layout,
        managed,
        SurfacePresentationRole::PolicyManaged,
        None,
        true,
    );
    // Policy is asked while the window is mapped, and its answer is honoured.
    assert_eq!(
        layout.managed_scene_visible::<()>(managed, |surface| {
            assert_eq!(surface, managed);
            Ok(true)
        }),
        Ok(true)
    );

    observe(
        &mut layout,
        managed,
        SurfacePresentationRole::PolicyManaged,
        None,
        false,
    );
    // Now policy is stale. It must not be consulted at all: mapping already
    // settles the question, and a projection that says otherwise is a view of
    // a layout one step behind.
    assert_eq!(
        layout.managed_scene_visible::<()>(managed, |_| {
            panic!("an unmapped surface must not consult a stale projection")
        }),
        Ok(false)
    );
}

/// Unmapping a window takes its popups with it, whatever policy still says.
#[test]
fn unmapping_a_managed_owner_hides_the_popups_it_owns() {
    let managed = SurfaceId::new(110, 1);
    let popup = SurfaceId::new(111, 1);
    let nested = SurfaceId::new(112, 1);
    let mut layout = PersistentLiveLayout::default();
    observe(
        &mut layout,
        managed,
        SurfacePresentationRole::PolicyManaged,
        None,
        true,
    );
    observe(
        &mut layout,
        popup,
        SurfacePresentationRole::ClientPositioned,
        Some(managed),
        true,
    );
    observe(
        &mut layout,
        nested,
        SurfacePresentationRole::ClientPositioned,
        Some(popup),
        true,
    );
    // Policy answers "visible" throughout, which is exactly the stale
    // projection the repair has to survive.
    let visible = |layout: &PersistentLiveLayout, surface| {
        layout
            .client_positioned_visible::<()>(surface, |_| Ok(true))
            .unwrap()
    };
    assert!(visible(&layout, nested));

    observe(
        &mut layout,
        managed,
        SurfacePresentationRole::PolicyManaged,
        None,
        false,
    );
    assert!(
        !visible(&layout, popup),
        "a popup whose window is unmapped is not visible"
    );
    assert!(
        !visible(&layout, nested),
        "and neither is a popup of that popup"
    );

    // Remapping the window restores both, so this hides rather than forgets.
    observe(
        &mut layout,
        managed,
        SurfacePresentationRole::PolicyManaged,
        None,
        true,
    );
    assert!(visible(&layout, popup));
    assert!(visible(&layout, nested));
}

/// A popup that unmaps and remaps returns, and a destroyed one does not come
/// back when its identifier is reused.
#[test]
fn a_popup_returns_on_remap_but_not_through_a_reused_identifier() {
    let managed = SurfaceId::new(120, 1);
    let popup = SurfaceId::new(121, 1);
    let mut layout = PersistentLiveLayout::default();
    observe(
        &mut layout,
        managed,
        SurfacePresentationRole::PolicyManaged,
        None,
        true,
    );
    observe(
        &mut layout,
        popup,
        SurfacePresentationRole::ClientPositioned,
        Some(managed),
        true,
    );
    let visible = |layout: &PersistentLiveLayout, surface| {
        layout
            .client_positioned_visible::<()>(surface, |_| Ok(true))
            .unwrap()
    };
    assert!(visible(&layout, popup));

    // A menu closing is an unmap, not a destroy.
    observe(
        &mut layout,
        popup,
        SurfacePresentationRole::ClientPositioned,
        Some(managed),
        false,
    );
    assert!(!visible(&layout, popup));
    observe(
        &mut layout,
        popup,
        SurfacePresentationRole::ClientPositioned,
        Some(managed),
        true,
    );
    assert!(visible(&layout, popup), "reopening the menu shows it again");

    // Destroying it drops the surface entirely. The generation makes a reused
    // X identifier a different surface, so the destroyed popup cannot be
    // revived by whatever the client creates next at the same id.
    let mut removal = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(72));
    removal.removed_surfaces.push(popup);
    layout.observe_authority_batch(&removal);
    assert!(!visible(&layout, popup));

    let reused = SurfaceId::new(121, 2);
    assert!(
        !visible(&layout, reused),
        "a reused identifier is not the destroyed popup and has no standing of its own"
    );
}

fn admission_geometry() -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: 240,
        height: 112,
    }
}

/// Drives a managed surface to the point where the frontend has been asked to
/// admit it and has not yet answered.
fn awaiting_admission(surface: SurfaceId, transaction: TransactionId) -> PersistentLiveLayout {
    let mut layout = PersistentLiveLayout::default();
    // The window exists and the authority reports it not yet mapped, which is
    // what a policy-managed window looks like before placement.
    observe(
        &mut layout,
        surface,
        SurfacePresentationRole::PolicyManaged,
        None,
        false,
    );
    assert!(
        layout
            .admissions
            .observe_intent(sophia_protocol::SurfacePresentationIntent {
                surface,
                kind: sophia_protocol::SurfacePresentationIntentKind::Request,
                role: SurfacePresentationRole::PolicyManaged,
                surface_kind: LayoutNodeKind::Toplevel,
                placement_preference: SurfacePlacementPreference::Default,
                presentation_owner: None,
                stack_rank: 0,
                geometry: admission_geometry(),
                constraints: SurfaceConstraints {
                    min_size: None,
                    max_size: None,
                },
                generation: 1,
            })
    );
    assert!(
        layout
            .admissions
            .begin_control(surface, transaction, admission_geometry())
    );
    layout
}

/// An acknowledged admission makes the window visible immediately, with no
/// further traffic of any kind.
///
/// The authority marks a policy-managed window viewable when it admits it, and
/// that transition rides in no presentation of its own, because the engine
/// asked for the map rather than a client. A live session showed the cost: two
/// Kitty windows viewable on the X server, drawing, and an entirely empty
/// scene. Nothing here observes another batch after the acknowledgement, which
/// is the point -- a client that admits and then goes idle must still appear.
#[test]
fn an_acknowledged_admission_shows_the_window_without_any_later_traffic() {
    let surface = SurfaceId::new(160, 1);
    let transaction = TransactionId::from_raw(90);
    let mut layout = awaiting_admission(surface, transaction);

    assert_eq!(
        layout.managed_scene_visible::<()>(surface, |_| Ok(true)),
        Ok(false),
        "before the frontend answers, the window is not in the scene"
    );

    assert!(layout.acknowledge_admission_control(transaction, surface));

    assert_eq!(
        layout.managed_scene_visible::<()>(surface, |_| Ok(true)),
        Ok(true),
        "the acknowledgement alone makes it eligible"
    );
}

/// Only a correlated acknowledgement counts, and it counts once.
#[test]
fn an_uncorrelated_admission_acknowledgement_does_not_map_anything() {
    let surface = SurfaceId::new(161, 1);
    let transaction = TransactionId::from_raw(91);
    let visible = |layout: &PersistentLiveLayout, surface| {
        layout.managed_scene_visible::<()>(surface, |_| Ok(true)) == Ok(true)
    };

    // A different transaction is not this admission.
    let mut layout = awaiting_admission(surface, transaction);
    assert!(!layout.acknowledge_admission_control(TransactionId::from_raw(92), surface));
    assert!(!visible(&layout, surface));
    // ...and the real one still works afterwards.
    assert!(layout.acknowledge_admission_control(transaction, surface));
    assert!(visible(&layout, surface));

    // A repeat is refused: the surface has left ControlPending.
    assert!(!layout.acknowledge_admission_control(transaction, surface));

    // A surface the authority never asked to admit cannot be mapped by an
    // acknowledgement naming it.
    let mut fresh = awaiting_admission(surface, transaction);
    let stranger = SurfaceId::new(162, 1);
    assert!(!fresh.acknowledge_admission_control(transaction, stranger));
    assert!(!visible(&fresh, stranger));

    // A reused identifier is a different surface, so an acknowledgement for the
    // old generation does not map the new one.
    let reused = SurfaceId::new(161, 2);
    assert!(!fresh.acknowledge_admission_control(transaction, reused));
    assert!(!visible(&fresh, reused));
}

/// An acknowledgement that arrives after the window is gone maps nothing.
#[test]
fn a_late_admission_acknowledgement_cannot_revive_a_removed_window() {
    let surface = SurfaceId::new(163, 1);
    let transaction = TransactionId::from_raw(93);
    let mut layout = awaiting_admission(surface, transaction);

    // The client destroys the window while the admission is in flight.
    let mut removal = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(94));
    removal.removed_surfaces.push(surface);
    layout.observe_authority_batch(&removal);

    assert!(
        !layout.acknowledge_admission_control(transaction, surface),
        "a destroyed window has no admission left to acknowledge"
    );
    assert_eq!(
        layout.managed_scene_visible::<()>(surface, |_| Ok(true)),
        Ok(false)
    );

    // And an unmap published after a good acknowledgement still wins.
    let mut unmapped = awaiting_admission(SurfaceId::new(164, 1), transaction);
    let live = SurfaceId::new(164, 1);
    assert!(unmapped.acknowledge_admission_control(transaction, live));
    observe(
        &mut unmapped,
        live,
        SurfacePresentationRole::PolicyManaged,
        None,
        false,
    );
    assert_eq!(
        unmapped.managed_scene_visible::<()>(live, |_| Ok(true)),
        Ok(false),
        "an unmap after admission removes it again"
    );
}

/// The admitted window's pixels actually reach a composed frame, and only
/// after the acknowledgement.
///
/// Every other test here reasons about eligibility. This one carries it
/// through to bytes, because eligibility is not what the user sees: the live
/// regression had two viewable Kitty windows drawing into retained buffers and
/// a scene that composed nothing, and an offline suite that asserted only
/// eligibility passed throughout. The scene is composed from exactly the
/// surfaces the visibility rule selects, which is what
/// `authority_production.rs` does when it builds `presentation_layout`.
#[test]
fn an_admitted_window_composes_its_pixels_and_an_unmapped_one_does_not() {
    let surface = SurfaceId::new(165, 1);
    let transaction = TransactionId::from_raw(95);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 2,
        height: 1,
    };
    // A distinctive marker so a composed frame cannot be confused with a
    // cleared one, in either direction.
    let marker = [0x21, 0x43, 0x65, 0xff, 0x21, 0x43, 0x65, 0xff];
    let empty = [0u8; 8];

    let mut scene = LiveProductionCpuScene::new(Size {
        width: 2,
        height: 1,
    });
    let committed = CommittedSurfaceState {
        surface,
        committed_generation: 1,
        geometry,
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 11 },
            Size {
                width: geometry.width,
                height: geometry.height,
            },
        ),
        damage: Region::single(geometry),
    };
    scene
        .apply_updates([sophia_backend_live::LiveCpuBufferUpdate::Replace(
            sophia_backend_live::LiveCpuBufferSource {
                pixel_version: None,
                handle: 11,
                size: Size {
                    width: 2,
                    height: 1,
                },
                stride: 8,
                format: X_AUTHORITY_CPU_BUFFER_FORMAT_XRGB8888,
                generation: 1,
                bytes: std::sync::Arc::new(marker.to_vec()),
            },
        )])
        .unwrap();
    scene.reconcile_buffer_residency(&[11]);

    let mut layout = awaiting_admission(surface, transaction);

    // The scene is composed from exactly what the visibility rule admits,
    // the way the production cycle builds its presentation layout.
    let selected = |layout: &PersistentLiveLayout| -> Vec<CommittedSurfaceState> {
        [committed.clone()]
            .into_iter()
            .filter(|state| {
                layout
                    .managed_scene_visible::<()>(state.surface, |_| Ok(true))
                    .unwrap()
            })
            .collect()
    };

    // The client has retained pixels and the frontend has not answered yet.
    // Nothing composes, and the marker is nowhere in the frame.
    let before = selected(&layout);
    assert!(before.is_empty());
    assert_eq!(
        scene.compose(&before, None, None).unwrap().frame.bytes,
        empty.to_vec().into(),
        "a window awaiting admission contributes no pixels"
    );

    // The frontend acknowledges. No further client traffic of any kind.
    assert!(layout.acknowledge_admission_control(transaction, surface));

    let after = selected(&layout);
    assert_eq!(after.len(), 1);
    assert_eq!(
        scene.compose(&after, None, None).unwrap().frame.bytes,
        marker.to_vec().into(),
        "the acknowledged window's retained pixels reach the frame"
    );

    // Unmapping takes them out again.
    observe(
        &mut layout,
        surface,
        SurfacePresentationRole::PolicyManaged,
        None,
        false,
    );
    let unmapped = selected(&layout);
    assert!(unmapped.is_empty());
    assert_eq!(
        scene.compose(&unmapped, None, None).unwrap().frame.bytes,
        empty.to_vec().into(),
        "an unmapped window stops contributing pixels"
    );
}

/// Hiding a window ends the input eligibility of the popups it owns, even
/// though their own mapped bit never changes.
///
/// This is the case a mapped-bit test cannot see, and it is the one that
/// matters: the surface most likely to hold a pointer grab when it stops being
/// eligible is a menu, and a menu usually stops being eligible because the
/// window under it was hidden, not because anything happened to the menu. A
/// surface whose own bit never moved would keep its focus, its pressed keys
/// and its route lease.
#[test]
fn hiding_an_owner_makes_the_popups_it_owns_newly_ineligible() {
    let owner = SurfaceId::new(190, 1);
    let popup = SurfaceId::new(191, 1);
    let nested = SurfaceId::new(192, 1);
    let mut layout = PersistentLiveLayout::default();
    for (surface, role, parent) in [
        (owner, SurfacePresentationRole::PolicyManaged, None),
        (
            popup,
            SurfacePresentationRole::ClientPositioned,
            Some(owner),
        ),
        (
            nested,
            SurfacePresentationRole::ClientPositioned,
            Some(popup),
        ),
    ] {
        observe(&mut layout, surface, role, parent, true);
    }
    let eligible_before = layout.input_eligible_surfaces();
    assert!(eligible_before.contains(&owner));
    assert!(eligible_before.contains(&popup));
    assert!(eligible_before.contains(&nested));

    // Only the owner is hidden. Both popups stay mapped.
    observe(
        &mut layout,
        owner,
        SurfacePresentationRole::PolicyManaged,
        None,
        false,
    );
    assert!(
        layout.mapped_surfaces.contains(&popup) && layout.mapped_surfaces.contains(&nested),
        "the popups' own mapped bits are untouched, which is the point"
    );

    let mut ended = layout.newly_ineligible_surfaces(&eligible_before);
    ended.sort_by_key(|surface| surface.index());
    assert_eq!(
        ended,
        vec![owner, popup, nested],
        "hiding the owner ends eligibility down the whole chain"
    );

    // Showing it again restores them, so this hides rather than forgets.
    observe(
        &mut layout,
        owner,
        SurfacePresentationRole::PolicyManaged,
        None,
        true,
    );
    assert!(
        layout
            .newly_ineligible_surfaces(&eligible_before)
            .is_empty()
    );
}

/// A surface awaiting its first admission never becomes newly ineligible.
///
/// It is unmapped, so a state test would name it and take input standing from
/// a window the user is still using. It was never eligible, so a difference
/// cannot name it.
#[test]
fn a_surface_awaiting_admission_is_never_newly_ineligible() {
    let established = SurfaceId::new(193, 1);
    let opening = SurfaceId::new(194, 1);
    let mut layout = PersistentLiveLayout::default();
    observe(
        &mut layout,
        established,
        SurfacePresentationRole::PolicyManaged,
        None,
        true,
    );
    let eligible_before = layout.input_eligible_surfaces();

    observe(
        &mut layout,
        opening,
        SurfacePresentationRole::PolicyManaged,
        None,
        false,
    );
    assert!(
        !layout.input_eligible(opening),
        "a surface awaiting admission cannot answer input"
    );
    assert!(
        layout
            .newly_ineligible_surfaces(&eligible_before)
            .is_empty(),
        "and it takes nothing away from the window still in use"
    );
    assert!(layout.input_eligible(established));
}

/// A repaint does not sweep eligibility; a lifecycle change does.
///
/// The sweep runs on every authority batch, and most batches are repaints, so
/// gating it is what keeps a per-frame allocation off the steady-state path.
/// The gate has to stay generous in one direction: reparenting a mapped popup
/// under a hidden window ends its eligibility without touching its own mapped
/// bit, so an owner change counts even when everything reports itself mapped.
#[test]
fn only_lifecycle_batches_sweep_input_eligibility() {
    let owner = SurfaceId::new(195, 1);
    let popup = SurfaceId::new(196, 1);
    let other = SurfaceId::new(197, 1);
    let mut layout = PersistentLiveLayout::default();
    observe(
        &mut layout,
        owner,
        SurfacePresentationRole::PolicyManaged,
        None,
        true,
    );
    observe(
        &mut layout,
        other,
        SurfacePresentationRole::PolicyManaged,
        None,
        true,
    );
    observe(
        &mut layout,
        popup,
        SurfacePresentationRole::ClientPositioned,
        Some(owner),
        true,
    );

    let batch_for = |surface, role, parent, mapped| {
        let mut batch =
            crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(98));
        batch.surface_presentations.push(
            sophia_x_authority::XAuthoritySurfacePresentationObservation {
                surface,
                role,
                kind: LayoutNodeKind::Toplevel,
                placement_preference: SurfacePlacementPreference::Default,
                owner: parent,
                stack_rank: 0,
                mapped,
                geometry: admission_geometry(),
                constraints: SurfaceConstraints {
                    min_size: None,
                    max_size: None,
                },
                generation: 1,
            },
        );
        batch
    };

    // An ordinary repaint: same role, same owner, still mapped.
    let repaint = batch_for(
        popup,
        SurfacePresentationRole::ClientPositioned,
        Some(owner),
        true,
    );
    assert!(
        !layout.batch_can_end_input_eligibility(&repaint),
        "a repaint cannot end eligibility and must not sweep"
    );

    // Hiding can.
    let hide = batch_for(
        popup,
        SurfacePresentationRole::ClientPositioned,
        Some(owner),
        false,
    );
    assert!(layout.batch_can_end_input_eligibility(&hide));

    // So can reparenting onto a different owner, with nothing unmapped.
    let reparent = batch_for(
        popup,
        SurfacePresentationRole::ClientPositioned,
        Some(other),
        true,
    );
    assert!(
        layout.batch_can_end_input_eligibility(&reparent),
        "an owner change can hide a mapped popup and must sweep"
    );

    // And so can a removal, whatever its presentations say.
    let mut removal = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(99));
    removal.removed_surfaces.push(popup);
    assert!(layout.batch_can_end_input_eligibility(&removal));
}

/// Retiring a hidden surface clears the staged focus that would otherwise
/// hand it the keyboard one commit later.
///
/// Clearing `focus_to_apply` and `retirement_focus` is not sufficient on its
/// own: a staged proposal carries its own `focus`, and committing it puts that
/// surface straight back into `retirement_focus` or queues a handoff for it.
/// A hidden window would take the keyboard again a commit after it was taken
/// away, which is a late resurrection rather than a fix.
#[test]
fn retiring_a_hidden_surface_clears_the_staged_focus_that_would_restore_it() {
    let hidden = SurfaceId::new(198, 1);
    let other = SurfaceId::new(199, 1);
    let transaction = TransactionId::from_raw(100);
    let geometry = admission_geometry();
    let mut layout = PersistentLiveLayout::default();
    observe(
        &mut layout,
        hidden,
        SurfacePresentationRole::PolicyManaged,
        None,
        true,
    );

    let staged_layer = |surface| LayerSnapshot {
        input_region: None,
        translation: None,
        output: None,
        surface,
        authority_local_id: None,
        namespace: None,
        stack_rank: 0,
        geometry,
        source_size: Size {
            width: geometry.width,
            height: geometry.height,
        },
        source: BufferSource::CpuBuffer { handle: 1 },
        damage: Region::empty(),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: 1,
        resize_sync: ResizeSyncCapability::ImplicitOnly,
    };
    layout.focus_to_apply = Some((transaction, hidden));
    layout.pending = Some(PendingLiveWmLayout {
        transaction,
        layers: vec![staged_layer(hidden), staged_layer(other)],
        requested_sizes: BTreeMap::from([
            (
                hidden,
                Size {
                    width: geometry.width,
                    height: geometry.height,
                },
            ),
            (
                other,
                Size {
                    width: geometry.width,
                    height: geometry.height,
                },
            ),
        ]),
        presentation_states: BTreeMap::new(),
        presentation_settlements: BTreeSet::new(),
        configure_deliveries: 0,
        focus: Some(hidden),
        deadline: Instant::now() + Duration::from_secs(1),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![hidden],
            },
        },
        moved_surfaces: 0,
        staged_transactions: BTreeMap::new(),
        admission_surfaces: BTreeSet::new(),
        source: None,
        policy_settlement: None,
    });

    layout.retire_hidden_input_claims(hidden);

    let pending = layout.pending.as_ref().expect("the proposal is retained");
    assert_eq!(
        pending.focus, None,
        "the staged focus cannot hand the keyboard back a commit later"
    );
    assert_eq!(layout.focus_to_apply, None);
    assert!(!layout.retirement_focus.contains_key(&hidden));
    assert_eq!(
        pending
            .layers
            .iter()
            .map(|layer| layer.surface)
            .collect::<Vec<_>>(),
        vec![other],
        "the hidden surface stops being positioned, and its neighbour is untouched"
    );
    assert!(!pending.requested_sizes.contains_key(&hidden));
    assert!(pending.requested_sizes.contains_key(&other));
    // Retiring claims is not destroying: nothing here removes content,
    // admission or the settlement identity the proposal is waiting on.
    assert_eq!(pending.transaction, transaction);
    assert!(pending.policy_settlement.is_none());
}
