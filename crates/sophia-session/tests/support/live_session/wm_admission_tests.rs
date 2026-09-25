use super::*;
use crate::live_session::PersistentLiveLayout;
use crate::resize_transaction::ResizeVisualCommit;
use sophia_protocol::{SurfaceConstraints, TransactionId};

#[test]
fn renderer_residency_tracks_only_cpu_buffers_owned_by_admission_groups() {
    let surface = SurfaceId::new(62, 1);
    let transaction = TransactionId::from_raw(368);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 640,
        height: 480,
    };
    let group = crate::live_session::LiveAdmissionAuthorityGroup {
        transaction,
        transactions: vec![SurfaceTransaction {
            input_region: None,
            transaction,
            authority: sophia_protocol::AuthorityKind::SophiaX,
            surface,
            namespace: None,
            target_geometry: geometry,
            presentation_extent: Size {
                width: (geometry).width,
                height: (geometry).height,
            },
            content: sophia_protocol::SurfaceContentSet::singleton(
                BufferSource::CpuBuffer { handle: 369 },
                sophia_protocol::Size {
                    width: geometry.width,
                    height: geometry.height,
                },
            ),

            damage: Region::single(geometry),
            readiness: sophia_protocol::SurfaceTransactionReadiness::Ready,
            timeout_msec: 250,
            previous_committed_generation: 0,
        }],
        cpu_buffer_updates: Vec::new(),
        present_submissions: Vec::new(),
        software_present_submissions: Vec::new(),
        superseded: false,
    };
    let mut layout = PersistentLiveLayout::default();
    let mut handles = Vec::new();

    layout.pre_admission_groups.push_back(group.clone());
    layout.write_pending_cpu_buffer_handles(&mut handles);
    assert_eq!(handles, vec![369]);

    layout.pre_admission_groups.clear();
    layout.released_admission_groups.push_back(group);
    layout.write_pending_cpu_buffer_handles(&mut handles);
    assert_eq!(handles, vec![369]);

    layout.released_admission_groups.clear();
    layout.write_pending_cpu_buffer_handles(&mut handles);
    assert!(handles.is_empty());
}

#[test]
fn released_admission_pixels_wait_for_policy_assignment() {
    let surface = SurfaceId::new(63, 1);
    let transaction = TransactionId::from_raw(370);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 640,
        height: 480,
    };
    let group = crate::live_session::LiveAdmissionAuthorityGroup {
        transaction,
        transactions: vec![SurfaceTransaction {
            input_region: None,
            transaction,
            authority: sophia_protocol::AuthorityKind::SophiaX,
            surface,
            namespace: None,
            target_geometry: geometry,
            presentation_extent: Size {
                width: (geometry).width,
                height: (geometry).height,
            },
            content: sophia_protocol::SurfaceContentSet::singleton(
                BufferSource::CpuBuffer { handle: 371 },
                sophia_protocol::Size {
                    width: geometry.width,
                    height: geometry.height,
                },
            ),

            damage: Region::single(geometry),
            readiness: sophia_protocol::SurfaceTransactionReadiness::Ready,
            timeout_msec: 250,
            previous_committed_generation: 0,
        }],
        cpu_buffer_updates: Vec::new(),
        present_submissions: Vec::new(),
        software_present_submissions: Vec::new(),
        superseded: false,
    };
    let batch = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(372));
    let mut layout = PersistentLiveLayout::default();
    layout.unmanaged_surfaces.insert(surface);
    layout.released_admission_groups.push_back(group);

    let (_, released) = layout.projected_batch(&batch);
    assert!(released.is_empty());
    assert_eq!(layout.released_admission_groups.len(), 1);

    layout.unmanaged_surfaces.remove(&surface);
    let (_, released) = layout.projected_batch(&batch);
    assert_eq!(released.len(), 1);
    assert!(layout.released_admission_groups.is_empty());
}

#[test]
fn exact_armed_launch_candidate_bypasses_a_different_standing_target() {
    let surface = SurfaceId::new(64, 1);
    let transaction = TransactionId::from_raw(373);
    let other_transaction = TransactionId::from_raw(374);
    let buffer = sophia_protocol::BufferHandle::from_raw(375);
    let launch = Size {
        width: 1280,
        height: 1040,
    };
    let target = Size {
        width: 1276,
        height: 1422,
    };
    let mut layout = PersistentLiveLayout::default();
    layout.dma_buf_sizes.insert(buffer, launch);
    layout.layout_epochs.set_pending_target(surface, target);
    layout
        .awaiting_visual_commits
        .arm(ResizeVisualCommit {
            candidate: sophia_protocol::SurfaceTransactionKey {
                transaction,
                surface,
                target_buffer: BufferSource::DmaBuf {
                    handle: buffer.raw(),
                },
            },
            size: launch,
            layout_size: launch,
        })
        .unwrap();

    assert_eq!(
        layout.present_layout_disposition(transaction, surface, buffer),
        sophia_backend_live::LiveProductionPresentDisposition::Immediate
    );
    assert_eq!(
        layout.present_layout_disposition(other_transaction, surface, buffer),
        sophia_backend_live::LiveProductionPresentDisposition::Immediate
    );
}

#[test]
fn pre_admission_group_queue_fails_closed_at_its_fixed_capacity() {
    let surface = SurfaceId::new(8, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 64,
        height: 64,
    };
    let mut batch = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(20));
    batch.surface_presentations.push(
        sophia_x_authority::XAuthoritySurfacePresentationObservation {
            surface,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            stack_rank: 0,
            owner: None,
            mapped: false,
            geometry,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    );
    batch
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
    let first_observation = layout.observe_authority_batch(&batch);
    assert!(!first_observation.admission_group_overflowed);

    let mut overflowed = false;
    for index in 0..=crate::live_session::PRE_ADMISSION_GROUP_CAPACITY {
        let transaction = TransactionId::from_raw(u64::try_from(index + 21).unwrap());
        let mut present = crate::live_session::wm_update_coordinator_batch(transaction);
        present.transactions.push(SurfaceTransaction {
            input_region: None,
            transaction,
            authority: sophia_protocol::AuthorityKind::SophiaX,
            surface,
            namespace: None,
            target_geometry: geometry,
            presentation_extent: Size {
                width: (geometry).width,
                height: (geometry).height,
            },
            content: sophia_protocol::SurfaceContentSet::singleton(
                BufferSource::DmaBuf {
                    handle: transaction.raw(),
                },
                sophia_protocol::Size {
                    width: geometry.width,
                    height: geometry.height,
                },
            ),

            damage: Region::single(geometry),
            readiness: sophia_protocol::SurfaceTransactionReadiness::Ready,
            timeout_msec: 250,
            previous_committed_generation: 0,
        });
        present
            .present_submissions
            .push(sophia_x_authority::XAuthorityPresentSubmission {
                transaction,
                surface,
                buffer: sophia_protocol::BufferHandle::from_raw(transaction.raw()),
                x_offset: 0,
                y_offset: 0,
                acquire_fence: None,
                idle_fence: None,
            });
        overflowed |= layout
            .observe_authority_batch(&present)
            .admission_group_overflowed;
    }

    assert!(overflowed);
    assert_eq!(
        layout.pre_admission_groups.len(),
        crate::live_session::PRE_ADMISSION_GROUP_CAPACITY
    );
}

#[test]
fn a_first_frame_admits_its_surface_rather_than_settling_a_standing_target() {
    // A launch deferred out of a resize epoch keeps its extent as a standing
    // obligation. That target must not capture the surface's first presented
    // frame: standing recovery keeps a successor beside a fallback that has
    // already retired, and a surface still awaiting admission has no fallback.
    // Diverting the frame settles layout and never completes admission, so the
    // surface ends up holding pixels nothing composites -- a window that is
    // placed, sized, and permanently empty.
    let surface = SurfaceId::new(65, 1);
    let transaction = TransactionId::from_raw(2252);
    let buffer = sophia_protocol::BufferHandle::from_raw(15);
    let target = Size {
        width: 1278,
        height: 1424,
    };
    let mut layout = PersistentLiveLayout::default();
    layout.dma_buf_sizes.insert(buffer, target);
    layout.layout_epochs.set_pending_target(surface, target);
    // Policy-managed and not yet admitted: exactly a launched browser's surface
    // at the moment its first frame arrives.
    layout.presentation_roles.insert(
        surface,
        sophia_protocol::SurfacePresentationRole::PolicyManaged,
    );
    layout
        .admissions
        .observe_intent(sophia_protocol::SurfacePresentationIntent {
            surface,
            kind: sophia_protocol::SurfacePresentationIntentKind::Request,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            surface_kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            presentation_owner: None,
            stack_rank: 0,
            geometry: Rect::default(),
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        });
    assert!(
        layout.surface_requires_admission(surface),
        "the surface must still require admission for this test to mean anything",
    );

    let geometry = Rect {
        x: 0,
        y: 0,
        width: target.width,
        height: target.height,
    };
    let frame = SurfaceTransaction {
        input_region: None,
        transaction,
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: geometry,
        presentation_extent: target,
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::DmaBuf {
                handle: buffer.raw(),
            },
            target,
        ),
        damage: Region::single(geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 1,
    };

    let armed = layout.arm_standing_recovery_candidate(
        &frame,
        target,
        sophia_engine::SurfaceVisualEvidence::PresentedBuffer,
        true,
    );

    assert!(
        !armed,
        "a pre-admission first frame must stay on the admission path",
    );
    assert!(
        !layout
            .awaiting_visual_commits
            .surface_layout_awaiting(surface, target),
        "no resize commit may capture the frame that should admit the surface",
    );
}
