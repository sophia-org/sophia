use super::*;
use crate::live_session::{PendingLiveWmLayout, PersistentLiveLayout};
use sophia_protocol::{SurfaceConstraints, TransactionCommit, TransactionId, TransactionOutcome};
use std::collections::{BTreeMap, BTreeSet};

fn hold_resize(
    layout: &mut PersistentLiveLayout,
    surface: SurfaceId,
    transaction: TransactionId,
    geometry: Rect,
) {
    layout.pending = Some(PendingLiveWmLayout {
        transaction,
        layers: vec![LayerSnapshot {
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
            source: BufferSource::None,
            damage: Region::single(geometry),
            opacity: 1.0,
            crop: None,
            transform: Transform::IDENTITY,
            generation: 1,
            resize_sync: ResizeSyncCapability::ImplicitOnly,
        }],
        requested_sizes: BTreeMap::from([(
            surface,
            Size {
                width: geometry.width,
                height: geometry.height,
            },
        )]),
        presentation_states: BTreeMap::new(),
        presentation_settlements: BTreeSet::new(),
        configure_deliveries: 0,
        focus: Some(surface),
        deadline: Instant::now() + Duration::from_secs(1),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![surface],
            },
        },
        moved_surfaces: 0,
        staged_transactions: BTreeMap::new(),
        admission_surfaces: BTreeSet::new(),
        source: None,
        policy_settlement: None,
    });
}

#[test]
fn explicit_software_present_completes_resize_only_after_native_retirement() {
    let surface = SurfaceId::new(85, 1);
    let launch = Size {
        width: 640,
        height: 480,
    };
    let target = Size {
        width: 800,
        height: 600,
    };
    let target_geometry = Rect {
        x: 0,
        y: 0,
        width: target.width,
        height: target.height,
    };
    let mut layout = PersistentLiveLayout::default();
    layout.layout_epochs.record_safe_observation(
        sophia_protocol::SurfaceTransactionKey {
            transaction: TransactionId::from_raw(850),
            surface,
            target_buffer: BufferSource::DmaBuf { handle: 850 },
        },
        launch,
        sophia_engine::SurfaceVisualEvidence::PresentedBuffer,
    );
    layout.layout_epochs.record_committed(surface, launch);
    layout
        .layout_epochs
        .set_admission(surface, sophia_engine::SurfaceAdmissionState::Managed);
    layout.layout_epochs.set_pending_target(surface, target);
    hold_resize(
        &mut layout,
        surface,
        TransactionId::from_raw(851),
        target_geometry,
    );

    let buffer = 852;
    layout.cpu_buffer_sizes.insert(buffer, target);
    let transaction = TransactionId::from_raw(852);
    let mut presented = crate::live_session::wm_update_coordinator_batch(transaction);
    let pixels = SurfaceTransaction {
        input_region: None,
        transaction,
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry,
        presentation_extent: Size {
            width: target_geometry.width,
            height: target_geometry.height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: buffer },
            sophia_protocol::Size {
                width: target_geometry.width,
                height: target_geometry.height,
            },
        ),

        damage: Region::single(target_geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 1,
    };
    let candidate = pixels.key();
    presented.transactions.push(pixels);
    presented.software_present_submissions.push(
        sophia_x_authority::XAuthoritySoftwarePresentSubmission {
            transaction,
            surface,
            acquire_fence: None,
            idle_fence: None,
        },
    );
    layout.observe_authority_batch(&presented);

    assert!(layout.resolve_pending().is_some());
    assert!(layout.awaiting_visual_commits.surface_awaiting(surface));
    assert_eq!(layout.layout_epochs.committed_size(surface), Some(launch));
    assert_eq!(layout.layout_epochs.pending_target(surface), Some(target));
    assert!(layout.complete_visual_commit(candidate, target));
    assert_eq!(layout.layout_epochs.committed_size(surface), Some(target));
    assert_eq!(layout.layout_epochs.pending_target(surface), None);
}

#[test]
fn cpu_present_admission_remains_fenced_until_exact_native_retirement() {
    let surface = SurfaceId::new(86, 1);
    let transaction = TransactionId::from_raw(860);
    let admission = TransactionId::from_raw(861);
    let size = Size {
        width: 500,
        height: 500,
    };
    let geometry = Rect {
        x: 0,
        y: 0,
        width: size.width,
        height: size.height,
    };
    let pixels = SurfaceTransaction {
        input_region: None,
        transaction,
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: geometry,
        presentation_extent: Size {
            width: (geometry).width,
            height: (geometry).height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 862 },
            size,
        ),

        damage: Region::single(geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let candidate = pixels.key();
    let intent = sophia_protocol::SurfacePresentationIntent {
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
    };
    let mut layout = PersistentLiveLayout::default();
    layout.admissions.observe_intent(intent);
    assert!(
        layout
            .admissions
            .begin_control(surface, admission, geometry)
    );
    assert!(layout.admissions.acknowledge_control(surface, admission));
    layout.cpu_buffer_sizes.insert(862, size);
    layout.layout_epochs.record_safe_observation(
        candidate,
        size,
        sophia_engine::SurfaceVisualEvidence::PresentedBuffer,
    );
    hold_resize(&mut layout, surface, admission, geometry);
    let pending = layout.pending.as_mut().unwrap();
    pending.admission_surfaces.insert(surface);
    pending.staged_transactions.insert(surface, pixels);

    assert!(layout.resolve_pending().is_some());
    assert_eq!(
        layout.admissions.state(surface),
        sophia_engine::SurfacePresentationAdmissionState::AwaitingRetirement {
            admission_transaction: admission,
            visual_candidate: candidate,
            geometry,
        }
    );
    assert!(
        layout
            .awaiting_visual_commits
            .exact_candidate(candidate, size)
    );
    assert!(layout.complete_visual_commit(candidate, size));
    assert!(layout.complete_admission_retirement(candidate));
    assert_eq!(
        layout.admissions.state(surface),
        sophia_engine::SurfacePresentationAdmissionState::Managed
    );
}
