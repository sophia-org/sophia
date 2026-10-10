use sophia_engine::{
    LayoutConstraintError, LayoutEpochCoordinator, SurfaceAdmissionState, SurfaceVisualEvidence,
};
use sophia_protocol::{
    BufferSource, LayoutTransaction, Rect, Size, SurfaceId, SurfacePlacement, SurfaceSizeRequest,
    SurfaceTransactionKey, TransactionId, Transform,
};

fn fixture() -> (LayoutEpochCoordinator, SurfaceTransactionKey, Rect, Size) {
    let candidate = SurfaceTransactionKey {
        transaction: TransactionId::from_raw(7),
        surface: SurfaceId::new(91, 1),
        target_buffer: BufferSource::DmaBuf { handle: 44 },
    };
    let extent = Size {
        width: 2542,
        height: 1398,
    };
    let bounds = Rect {
        x: 2560,
        y: 24,
        width: 1920,
        height: 1056,
    };
    let mut coordinator = LayoutEpochCoordinator::default();
    coordinator.record_safe_observation(candidate, extent, SurfaceVisualEvidence::PresentedBuffer);
    coordinator.set_recovery_extent(candidate.surface, extent);
    coordinator.set_admission(candidate.surface, SurfaceAdmissionState::PendingLayout);
    (coordinator, candidate, bounds, extent)
}

fn proposal(surface: SurfaceId, geometry: Rect) -> LayoutTransaction {
    LayoutTransaction {
        transaction: TransactionId::from_raw(8),
        requested_sizes: vec![SurfaceSizeRequest {
            surface,
            size: Size {
                width: geometry.width,
                height: geometry.height,
            },
        }],
        focus: Some(surface),
        render_positions: vec![SurfacePlacement {
            surface,
            geometry,
            z_index: 0,
            crop: None,
            transform: Transform::IDENTITY,
        }],
        timeout_msec: 500,
    }
}

#[test]
fn exact_admission_pixels_keep_their_extent_at_the_assigned_output_origin() {
    let (coordinator, candidate, bounds, extent) = fixture();
    let reconciled = coordinator
        .reconcile_admission_transaction(&proposal(candidate.surface, bounds), bounds, candidate)
        .unwrap();
    assert_eq!(reconciled.transaction.requested_sizes[0].size, extent);
    assert_eq!(
        reconciled.transaction.render_positions[0].geometry,
        Rect {
            width: extent.width,
            height: extent.height,
            ..bounds
        }
    );
    assert_eq!(coordinator.committed_size(candidate.surface), None);
}

#[test]
fn a_foreign_candidate_cannot_widen_an_admission() {
    let (coordinator, candidate, bounds, _) = fixture();
    for foreign in [
        SurfaceTransactionKey {
            transaction: TransactionId::from_raw(6),
            ..candidate
        },
        SurfaceTransactionKey {
            surface: SurfaceId::new(91, 2),
            ..candidate
        },
        SurfaceTransactionKey {
            target_buffer: BufferSource::DmaBuf { handle: 45 },
            ..candidate
        },
    ] {
        let reconciled = coordinator
            .reconcile_admission_transaction(&proposal(candidate.surface, bounds), bounds, foreign)
            .unwrap();
        assert_eq!(reconciled.transaction.render_positions[0].geometry, bounds);
    }
}

#[test]
fn managed_or_unobserved_recovery_extents_still_yield_to_output_bounds() {
    for mutation in 0..3 {
        let (mut coordinator, candidate, bounds, _) = fixture();
        match mutation {
            0 => coordinator.set_admission(candidate.surface, SurfaceAdmissionState::Managed),
            1 => {
                coordinator.reject_safe_observation(candidate);
            }
            _ => coordinator.set_recovery_extent(
                candidate.surface,
                Size {
                    width: 2540,
                    height: 1398,
                },
            ),
        }
        let reconciled = coordinator
            .reconcile_admission_transaction(
                &proposal(candidate.surface, bounds),
                bounds,
                candidate,
            )
            .unwrap();
        assert_eq!(reconciled.transaction.render_positions[0].geometry, bounds);
    }
}

#[test]
fn admission_authority_does_not_widen_a_sibling_surface() {
    let (mut coordinator, candidate, bounds, extent) = fixture();
    let sibling = SurfaceId::new(92, 1);
    coordinator.set_recovery_extent(sibling, extent);
    coordinator.set_admission(sibling, SurfaceAdmissionState::PendingLayout);
    let mut transaction = proposal(candidate.surface, bounds);
    let other = proposal(sibling, bounds);
    transaction.requested_sizes.extend(other.requested_sizes);
    transaction.render_positions.extend(other.render_positions);
    let reconciled = coordinator
        .reconcile_admission_transaction(&transaction, bounds, candidate)
        .unwrap();
    assert_eq!(reconciled.transaction.requested_sizes[0].size, extent);
    assert_eq!(reconciled.transaction.render_positions[1].geometry, bounds);
}

#[test]
fn admission_clipping_does_not_authorize_coordinate_overflow() {
    let (coordinator, candidate, mut bounds, _) = fixture();
    bounds.x = i32::MAX - bounds.width;
    assert_eq!(
        coordinator.reconcile_admission_transaction(
            &proposal(candidate.surface, bounds),
            bounds,
            candidate,
        ),
        Err(LayoutConstraintError::GeometryOverflow {
            surface: candidate.surface
        })
    );
}
