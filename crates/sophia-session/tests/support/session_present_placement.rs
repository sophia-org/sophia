//! Layout, compositor selection and real prepared-Pixmap admission. No device,
//! elapsed-time sleep or pixel availability is needed to choose the first clock.
use super::selection::select_with_projections;
use super::*;
use crate::live_session::present_clock::admission_tests::{AdmissionFrontend, NS, SURFACE, WINDOW};
use crate::live_session::{LivePolicyMapMode, PendingLiveWmLayout, PersistentLiveLayout};
use sophia_backend_live::LiveProductionVisualRuntime;
use sophia_engine::{HeadlessOutput, WmTransactionUpdate};
use sophia_protocol::*;
use sophia_x_authority::{XPresentFenceResources, XPresentMscTiming, XResourceId};
use std::collections::BTreeSet;

const OUTPUT: OutputId = OutputId::from_raw(1);
const EPOCH: TransactionId = TransactionId::from_raw(70);
const REQUEST: TransactionId = TransactionId::from_raw(3);

fn geometry() -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: 20,
        height: 20,
    }
}

fn visual() -> LiveProductionVisualRuntime {
    LiveProductionVisualRuntime::new(
        &[HeadlessOutput {
            id: OUTPUT,
            size: Size {
                width: 64,
                height: 32,
            },
            scale: 1,
        }],
        None,
    )
    .unwrap()
}

fn layer(surface: SurfaceId) -> LayerSnapshot {
    LayerSnapshot {
        surface,
        input_region: None,
        translation: None,
        output: Some(OUTPUT),
        authority_local_id: None,
        namespace: None,
        stack_rank: 0,
        geometry: geometry(),
        source_size: Size {
            width: 20,
            height: 20,
        },
        source: BufferSource::None,
        damage: Region::empty(),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: 1,
        resize_sync: ResizeSyncCapability::ImplicitOnly,
    }
}

fn projection(minimized: bool) -> Vec<PolicyOutputProjection> {
    vec![PolicyOutputProjection {
        output: OUTPUT,
        focus: None,
        placements: vec![PolicySurfacePlacement {
            surface: SURFACE,
            surface_generation: 1,
            // Outer/chrome rectangle deliberately differs from content.
            geometry: Rect {
                y: -100,
                ..geometry()
            },
            requested_size: None,
            crop: None,
            transform: PolicyTransform::Identity,
            presentation: PolicyPresentationState {
                minimized,
                ..Default::default()
            },
        }],
    }]
}

fn managed() -> PersistentLiveLayout {
    let mut layout = PersistentLiveLayout::new(
        LivePolicyMapMode::Deferred,
        Size {
            width: 64,
            height: 32,
        },
    );
    layout.layers.insert(SURFACE, layer(SURFACE));
    layout.mapped_surfaces.insert(SURFACE);
    layout
        .presentation_roles
        .insert(SURFACE, SurfacePresentationRole::PolicyManaged);
    layout
}

fn pending(layout: &mut PersistentLiveLayout) {
    layout.admissions.observe_intent(SurfacePresentationIntent {
        surface: SURFACE,
        kind: SurfacePresentationIntentKind::Request,
        role: SurfacePresentationRole::PolicyManaged,
        surface_kind: LayoutNodeKind::Toplevel,
        placement_preference: SurfacePlacementPreference::Default,
        presentation_owner: None,
        stack_rank: 0,
        geometry: geometry(),
        constraints: SurfaceConstraints {
            min_size: None,
            max_size: None,
        },
        generation: 1,
    });
    assert!(layout.admissions.begin_control(SURFACE, EPOCH, geometry()));
    let sibling = SurfaceId::new(400, 1);
    layout.pending = Some(PendingLiveWmLayout {
        transaction: EPOCH,
        layers: vec![layer(SURFACE), layer(sibling)],
        // A neighbour still owes its exact-size frame: the epoch cannot commit.
        requested_sizes: BTreeMap::from([(
            sibling,
            Size {
                width: 20,
                height: 20,
            },
        )]),
        presentation_states: BTreeMap::new(),
        presentation_settlements: BTreeSet::new(),
        configure_deliveries: 1,
        focus: None,
        deadline: Instant::now() + Duration::from_secs(4),
        update: WmTransactionUpdate {
            commit: TransactionCommit {
                transaction: EPOCH,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![],
            },
        },
        moved_surfaces: 1,
        staged_transactions: BTreeMap::new(),
        admission_surfaces: BTreeSet::from([SURFACE]),
        source: None,
        policy_settlement: None,
    });
}

fn pixmap_frontend() -> AdmissionFrontend {
    let frontend = AdmissionFrontend::new(true);
    let pixmap = XResourceId::new(0x400002, 1);
    {
        let mut runtime = frontend.runtime.borrow_mut();
        runtime
            .create_pixmap(
                NS,
                pixmap,
                Size {
                    width: 20,
                    height: 20,
                },
                24,
                1,
            )
            .unwrap();
        runtime
            .prepare_standard_pixmap(
                1,
                REQUEST,
                NS,
                WINDOW,
                pixmap,
                (0, 0),
                None,
                None,
                XPresentFenceResources::default(),
            )
            .unwrap();
        runtime
            .request_prepared_present_clock(
                REQUEST,
                XPresentMscTiming::new(0, 0, 0, false).unwrap(),
            )
            .unwrap();
    }
    frontend
}

fn selected(
    visual: &LiveProductionVisualRuntime,
    layout: &PersistentLiveLayout,
    projections: &[PolicyOutputProjection],
    surface: SurfaceId,
) -> Option<OutputId> {
    select_with_projections(
        visual,
        layout,
        Some(projections),
        &[XPresentClockAdmission {
            request: REQUEST,
            target: Some((surface, geometry())),
        }],
        Some(OUTPUT),
    )[&surface]
}

#[test]
fn first_pixmap_uses_placement_before_pixels_instead_of_waiting_900ms_on_fake() {
    for pending_epoch in [false, true] {
        for clocked in [false, true] {
            let visual = visual();
            let mut layout = managed();
            let projections = if pending_epoch {
                pending(&mut layout);
                layout.layers.clear();
                assert!(!layout.pending_is_ready());
                vec![]
            } else {
                projection(false)
            };
            let frontend = pixmap_frontend();
            // This is the old selection, red for either first-frame shape.
            assert_eq!(
                visual.present_clock_outputs([(SURFACE, geometry())], Some(OUTPUT)),
                vec![(SURFACE, None)]
            );
            SessionPresentClocks::default().admit_with(&frontend, 100_000,
                |admissions| select_with_projections(&visual, &layout,
                    Some(&projections), admissions, Some(OUTPUT)),
                |output| {
                    assert_eq!(output, OUTPUT);
                    let source = LiveNativePresentClockSource { owner: 4, incarnation: 2 };
                    vec![(RenderHeadId::from_raw(1), LiveNativePresentClockObservation {
                        lost: None,
                        current: clocked.then_some(sophia_backend_live::LiveNativePresentClockSample {
                            source, ust_usec: 100_000, msc: 6,
                        }),
                        status: if clocked { LiveNativePresentClockStatus::Observed } else {
                            LiveNativePresentClockStatus::Unclocked(LiveNativeUnclockedPresentClock {
                                source, minimum_period_usec: 16_667,
                                reason: sophia_backend_live::LiveNativeUnclockedReason::SequenceUnsupported { errno: 95 },
                            })
                        },
                    })]
                }).unwrap();
            assert_ne!(
                frontend.bound.borrow()[0].1.source,
                XPresentClockSource::Fake
            );
            assert_eq!(
                frontend.runtime.borrow().ready_prepared_presents(),
                vec![REQUEST]
            );
            assert!(frontend.admissions().unwrap().is_empty());
        }
    }
}

#[test]
fn hidden_and_minimized_viewable_windows_and_their_popups_stay_fake() {
    let visual = visual();
    for projections in [vec![], projection(true)] {
        let mut layout = managed();
        assert_eq!(selected(&visual, &layout, &projections, SURFACE), None);
        let popup = SurfaceId::new(401, 1);
        layout.mapped_surfaces.insert(popup);
        layout
            .presentation_roles
            .insert(popup, SurfacePresentationRole::ClientPositioned);
        layout.presentation_owners.insert(popup, SURFACE);
        assert_eq!(selected(&visual, &layout, &projections, popup), None);
        // Even a pending epoch is not permission to sample an omitted admission.
        pending(&mut layout);
        layout
            .pending
            .as_mut()
            .unwrap()
            .layers
            .retain(|layer| layer.surface != SURFACE);
        assert_eq!(selected(&visual, &layout, &projections, SURFACE), None);
        assert_eq!(selected(&visual, &layout, &projections, popup), None);
    }
}

#[test]
fn pending_placement_expires_and_does_not_move_an_already_managed_window() {
    let visual = visual();
    let mut layout = managed();
    pending(&mut layout);
    assert_eq!(selected(&visual, &layout, &[], SURFACE), Some(OUTPUT));
    layout.pending.as_mut().unwrap().deadline = Instant::now();
    layout
        .expire_pending(&mut crate::session_control::SessionControlQueue::default())
        .unwrap()
        .unwrap();
    assert_eq!(selected(&visual, &layout, &[], SURFACE), None);
    pending(&mut layout);
    // A normal move is in the epoch but does not own this surface's admission.
    layout.pending.as_mut().unwrap().admission_surfaces.clear();
    layout.pending.as_mut().unwrap().layers[0].geometry.x = 200;
    assert_eq!(
        selected(&visual, &layout, &projection(false), SURFACE),
        Some(OUTPUT)
    );
}

#[test]
fn popup_map_and_remap_before_observation_use_viewable_without_exposing_known_hidden_owners() {
    let visual = visual();
    let mut layout = managed();
    let popup = SurfaceId::new(402, 1);
    // X map+Present is published before the first Session authority visit.
    assert_eq!(selected(&visual, &layout, &[], popup), Some(OUTPUT));
    // Remapping a toolkit's retained menu also precedes Session's mapped bit.
    layout
        .presentation_roles
        .insert(popup, SurfacePresentationRole::ClientPositioned);
    assert!(!layout.mapped_surfaces.contains(&popup));
    assert_eq!(selected(&visual, &layout, &[], popup), Some(OUTPUT));
    layout.presentation_owners.insert(popup, SURFACE);
    assert_eq!(selected(&visual, &layout, &[], popup), None);
    assert_eq!(
        selected(&visual, &layout, &projection(false), popup),
        Some(OUTPUT)
    );
    layout.mapped_surfaces.remove(&SURFACE);
    assert_eq!(selected(&visual, &layout, &projection(false), popup), None);
    layout.presentation_owners.insert(SURFACE, popup);
    layout
        .presentation_roles
        .insert(SURFACE, SurfacePresentationRole::ClientPositioned);
    layout.mapped_surfaces.insert(SURFACE);
    assert_eq!(
        selected(&visual, &layout, &projection(false), popup),
        None,
        "owner cycle"
    );
}

#[test]
fn direct_first_map_and_visible_popup_use_geometry_but_lock_wins() {
    let mut visual = visual();
    let direct = PersistentLiveLayout::new(
        LivePolicyMapMode::Direct,
        Size {
            width: 64,
            height: 32,
        },
    );
    assert_eq!(selected(&visual, &direct, &[], SURFACE), Some(OUTPUT));
    let mut layout = managed();
    let popup = SurfaceId::new(401, 1);
    layout.mapped_surfaces.insert(popup);
    layout
        .presentation_roles
        .insert(popup, SurfacePresentationRole::ClientPositioned);
    layout.presentation_owners.insert(popup, SURFACE);
    assert_eq!(
        selected(&visual, &layout, &projection(false), popup),
        Some(OUTPUT)
    );
    visual
        .set_session_lock(
            Some(sophia_engine::SessionLockCover {
                epoch: sophia_engine::SessionLockEpoch::from_raw(1).unwrap(),
                fill: sophia_engine::CompositorRgb8 {
                    red: 0,
                    green: 0,
                    blue: 0,
                },
            }),
            &sophia_backend_live::LiveProductionCpuScene::new(Size {
                width: 64,
                height: 32,
            }),
            None,
        )
        .unwrap();
    assert_eq!(selected(&visual, &direct, &[], SURFACE), None);
    assert_eq!(selected(&visual, &layout, &projection(false), popup), None);
}
