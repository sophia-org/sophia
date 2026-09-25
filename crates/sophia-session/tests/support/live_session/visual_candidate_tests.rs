use super::*;
use crate::live_session::{PendingLiveWmLayout, PersistentLiveLayout};
use sophia_protocol::{
    BufferHandle, SurfaceConstraints, SurfacePresentationIntent, SurfacePresentationIntentKind,
    TransactionCommit, TransactionId, TransactionOutcome,
};
use std::collections::{BTreeMap, BTreeSet};

fn rect(width: i32, height: i32) -> Rect {
    Rect {
        x: 0,
        y: 0,
        width,
        height,
    }
}

#[test]
fn inset_present_content_proves_the_outer_layout_extent_without_scaling() {
    let surface = SurfaceId::new(77, 1);
    let buffer = BufferHandle::from_raw(770);
    let outer = rect(1276, 1422);
    let content = Size {
        width: 1266,
        height: 1412,
    };
    let transaction = SurfaceTransaction {
        input_region: None,
        transaction: TransactionId::from_raw(77),
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: outer,
        // A descendant content window projected onto a larger policy-managed
        // surface: the authority presented into the child, so that is the
        // extent it filled, and the geometry beside it is the toplevel.
        presentation_extent: content,
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::DmaBuf {
                handle: buffer.raw(),
            },
            content,
        ),

        damage: Region::single(outer),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let dma_buf_sizes = BTreeMap::from([(buffer, content)]);

    assert_eq!(
        live_transaction_observed_size(&transaction, &dma_buf_sizes, &BTreeMap::new()),
        Size {
            width: outer.width,
            height: outer.height,
        }
    );
}

#[test]
fn mismatched_present_content_cannot_prove_the_outer_layout_extent() {
    let buffer = BufferHandle::from_raw(771);
    let stale = Size {
        width: 1280,
        height: 1040,
    };
    let transaction = SurfaceTransaction {
        input_region: None,
        transaction: TransactionId::from_raw(771),
        authority: AuthorityKind::SophiaX,
        surface: SurfaceId::new(771, 1),
        namespace: None,
        target_geometry: rect(1276, 1422),
        presentation_extent: Size {
            width: (rect(1276, 1422)).width,
            height: (rect(1276, 1422)).height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::DmaBuf {
                handle: buffer.raw(),
            },
            sophia_protocol::Size {
                width: 1266,
                height: 1412,
            },
        ),

        damage: Region::empty(),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let dma_buf_sizes = BTreeMap::from([(buffer, stale)]);

    assert_eq!(
        live_transaction_observed_size(&transaction, &dma_buf_sizes, &BTreeMap::new()),
        stale
    );
}

/// A client that has not answered its configure has not reached its size.
///
/// The window is already 1920x1080 and the client presents the 1280x1440
/// pixmap it still has. Both facts are now stated: the raster spans 1280x1440,
/// and it was presented into 1920x1080. The gate compares one measurement
/// against another and stays closed, where it once compared a measurement
/// against a declaration that had been filled in from the window.
#[test]
fn a_stale_present_does_not_prove_the_extent_it_was_presented_into() {
    let buffer = BufferHandle::from_raw(773);
    let stale_raster = Size {
        width: 1280,
        height: 1440,
    };
    let presented_into = Size {
        width: 1920,
        height: 1080,
    };
    let transaction = SurfaceTransaction {
        input_region: None,
        transaction: TransactionId::from_raw(773),
        authority: AuthorityKind::SophiaX,
        surface: SurfaceId::new(773, 1),
        namespace: None,
        target_geometry: rect(presented_into.width, presented_into.height),
        presentation_extent: presented_into,
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::DmaBuf {
                handle: buffer.raw(),
            },
            stale_raster,
        ),
        damage: Region::empty(),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let dma_buf_sizes = BTreeMap::from([(buffer, stale_raster)]);

    assert_eq!(
        live_transaction_observed_size(&transaction, &dma_buf_sizes, &BTreeMap::new()),
        stale_raster
    );
    // And the compositor is told what the buffer is, not what it was asked for.
    assert_eq!(
        live_transaction_raster_size(&transaction, &dma_buf_sizes, &BTreeMap::new()),
        stale_raster
    );
}

/// The raster size is the buffer's, even when the buffer satisfies its extent.
///
/// `live_transaction_observed_size` reports the logical extent in that case, on
/// purpose: it answers whether the surface reached its configured size. Reusing
/// it as a raster measurement put the placement back into the committed record
/// and a live session ended comparing it against the buffer -- planned
/// 1920x1080, held 1280x1440, one DMA-BUF.
#[test]
fn raster_size_reports_the_buffer_rather_than_the_configured_extent() {
    let buffer = BufferHandle::from_raw(772);
    let raster = Size {
        width: 1266,
        height: 1412,
    };
    let transaction = SurfaceTransaction {
        input_region: None,
        transaction: TransactionId::from_raw(772),
        authority: AuthorityKind::SophiaX,
        surface: SurfaceId::new(772, 1),
        namespace: None,
        target_geometry: rect(1276, 1422),
        // Presented into the child extent it filled; the toplevel is beside it.
        presentation_extent: raster,
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::DmaBuf {
                handle: buffer.raw(),
            },
            raster,
        ),
        damage: Region::empty(),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let dma_buf_sizes = BTreeMap::from([(buffer, raster)]);

    assert_eq!(
        live_transaction_raster_size(&transaction, &dma_buf_sizes, &BTreeMap::new()),
        raster
    );
    assert_eq!(
        live_transaction_observed_size(&transaction, &dma_buf_sizes, &BTreeMap::new()),
        Size {
            width: 1276,
            height: 1422,
        }
    );
}

#[test]
fn unresolved_x_pixmap_is_not_presented_buffer_evidence() {
    let transaction = SurfaceTransaction {
        input_region: None,
        transaction: TransactionId::from_raw(79),
        authority: AuthorityKind::SophiaX,
        surface: SurfaceId::new(79, 1),
        namespace: None,
        target_geometry: rect(500, 500),
        presentation_extent: Size {
            width: (rect(500, 500)).width,
            height: (rect(500, 500)).height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::XPixmap { pixmap: 0x220001 },
            sophia_protocol::Size {
                width: 500,
                height: 500,
            },
        ),

        damage: Region::single(rect(500, 500)),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let mut batch = crate::live_session::wm_update_coordinator_batch(transaction.transaction);
    batch.transactions.push(transaction.clone());

    assert_eq!(
        live_transaction_visual_evidence(&transaction, &batch),
        sophia_engine::SurfaceVisualEvidence::BackingSnapshot
    );
}

#[test]
fn presented_cpu_snapshot_is_complete_present_evidence() {
    let transaction = SurfaceTransaction {
        input_region: None,
        transaction: TransactionId::from_raw(78),
        authority: AuthorityKind::SophiaX,
        surface: SurfaceId::new(78, 1),
        namespace: None,
        target_geometry: rect(500, 500),
        presentation_extent: Size {
            width: (rect(500, 500)).width,
            height: (rect(500, 500)).height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 780 },
            sophia_protocol::Size {
                width: 500,
                height: 500,
            },
        ),

        damage: Region::single(rect(500, 500)),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let mut batch = crate::live_session::wm_update_coordinator_batch(transaction.transaction);
    batch.transactions.push(transaction.clone());
    batch.software_present_submissions.push(
        sophia_x_authority::XAuthoritySoftwarePresentSubmission {
            transaction: transaction.transaction,
            surface: transaction.surface,
            acquire_fence: None,
            idle_fence: None,
        },
    );

    assert_eq!(
        live_transaction_visual_evidence(&transaction, &batch),
        sophia_engine::SurfaceVisualEvidence::PresentedBuffer
    );
}

#[test]
fn backing_snapshot_cannot_impersonate_same_transaction_present() {
    let surface = SurfaceId::new(80, 1);
    let transaction = TransactionId::from_raw(80);
    let geometry = rect(500, 500);
    let dma_buffer = BufferHandle::from_raw(800);
    let cpu_handle = 801;
    let dma = SurfaceTransaction {
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
            BufferSource::DmaBuf {
                handle: dma_buffer.raw(),
            },
            sophia_protocol::Size {
                width: geometry.width,
                height: geometry.height,
            },
        ),

        damage: Region::single(geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let backing = SurfaceTransaction {
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: cpu_handle },
            dma.raster_extent(),
        ),
        ..dma.clone()
    };
    let mut batch = crate::live_session::wm_update_coordinator_batch(transaction);
    batch.transactions.extend([dma.clone(), backing.clone()]);
    batch
        .present_submissions
        .push(sophia_x_authority::XAuthorityPresentSubmission {
            transaction,
            surface,
            buffer: dma_buffer,
            x_offset: 0,
            y_offset: 0,
            acquire_fence: None,
            idle_fence: None,
        });

    assert_eq!(
        live_transaction_visual_evidence(&dma, &batch),
        sophia_engine::SurfaceVisualEvidence::PresentedBuffer
    );
    assert_eq!(
        live_transaction_visual_evidence(&backing, &batch),
        sophia_engine::SurfaceVisualEvidence::BackingSnapshot
    );

    let mut layout = PersistentLiveLayout::default();
    let extent = Size {
        width: geometry.width,
        height: geometry.height,
    };
    layout.dma_buf_sizes.insert(dma_buffer, extent);
    layout.cpu_buffer_sizes.insert(cpu_handle, extent);
    layout.observe_authority_batch(&batch);
    assert_eq!(
        layout.layout_epochs.safe_observation(surface),
        Some(sophia_engine::SafeSurfaceObservation {
            candidate: Some(dma.key()),
            extent,
            evidence: sophia_engine::SurfaceVisualEvidence::PresentedBuffer,
            sequence: 1,
        })
    );
}

#[test]
fn present_candidate_is_not_replaced_by_later_blank_backing_extent() {
    let surface = SurfaceId::new(81, 1);
    let initial = rect(500, 500);
    let tiled = rect(1276, 1422);
    let mut intent = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(80));
    intent.presentation_intents.push(SurfacePresentationIntent {
        surface,
        kind: SurfacePresentationIntentKind::Request,
        role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
        surface_kind: sophia_protocol::LayoutNodeKind::Toplevel,
        placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
        presentation_owner: None,
        stack_rank: 0,
        geometry: initial,
        constraints: SurfaceConstraints {
            min_size: None,
            max_size: None,
        },
        generation: 1,
    });
    let mut layout = PersistentLiveLayout::default();
    layout.observe_authority_batch(&intent);

    let present_id = TransactionId::from_raw(81);
    let present_buffer = BufferHandle::from_raw(810);
    layout.dma_buf_sizes.insert(
        present_buffer,
        Size {
            width: initial.width,
            height: initial.height,
        },
    );
    let mut present = crate::live_session::wm_update_coordinator_batch(present_id);
    present.transactions.push(SurfaceTransaction {
        input_region: None,
        transaction: present_id,
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: initial,
        presentation_extent: Size {
            width: (initial).width,
            height: (initial).height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::DmaBuf {
                handle: present_buffer.raw(),
            },
            sophia_protocol::Size {
                width: initial.width,
                height: initial.height,
            },
        ),

        damage: Region::single(initial),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    });
    present
        .present_submissions
        .push(sophia_x_authority::XAuthorityPresentSubmission {
            transaction: present_id,
            surface,
            buffer: present_buffer,
            x_offset: 0,
            y_offset: 0,
            acquire_fence: None,
            idle_fence: None,
        });
    layout.observe_authority_batch(&present);

    let backing_handle = 820;
    layout.cpu_buffer_sizes.insert(
        backing_handle,
        Size {
            width: tiled.width,
            height: tiled.height,
        },
    );
    let mut backing = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(82));
    backing.transactions.push(SurfaceTransaction {
        input_region: None,
        transaction: TransactionId::from_raw(82),
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: tiled,
        presentation_extent: Size {
            width: (tiled).width,
            height: (tiled).height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer {
                handle: backing_handle,
            },
            sophia_protocol::Size {
                width: tiled.width,
                height: tiled.height,
            },
        ),

        damage: Region::single(tiled),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 1,
    });
    layout.observe_authority_batch(&backing);

    let selected = layout.layout_epochs.safe_observation(surface).unwrap();
    assert_eq!(
        selected.candidate,
        Some(sophia_protocol::SurfaceTransactionKey {
            transaction: present_id,
            surface,
            target_buffer: BufferSource::DmaBuf {
                handle: present_buffer.raw(),
            },
        })
    );
    assert_eq!(selected.extent.width, initial.width);
    assert_eq!(selected.extent.height, initial.height);
    assert_eq!(
        selected.evidence,
        sophia_engine::SurfaceVisualEvidence::PresentedBuffer
    );
    assert!(
        layout
            .selected_pre_admission_transaction(
                surface,
                Size {
                    width: tiled.width,
                    height: tiled.height,
                },
            )
            .is_none()
    );
    let recovery = layout
        .layout_epochs
        .begin_recovery(
            [(
                surface,
                Size {
                    width: tiled.width,
                    height: tiled.height,
                },
            )],
            [surface],
        )
        .unwrap();
    assert_eq!(recovery[0].size, selected.extent);
}

#[test]
fn a_stronger_pre_admission_candidate_rebases_recovery_and_queues_relayout() {
    // Firefox can publish a small backing snapshot before its first complete
    // Present. The stronger frame must replace that temporary constraint;
    // otherwise policy keeps requesting an extent whose selected pixels no
    // longer exist and the surface remains invisible.
    let surface = SurfaceId::new(81, 1);
    let transaction = TransactionId::from_raw(2318);
    let geometry = rect(1278, 1424);
    let stale = Size {
        width: 200,
        height: 210,
    };
    let dma_buffer = BufferHandle::from_raw(16);
    let frame = SurfaceTransaction {
        input_region: None,
        transaction,
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: geometry,
        presentation_extent: Size {
            width: geometry.width,
            height: geometry.height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::DmaBuf {
                handle: dma_buffer.raw(),
            },
            sophia_protocol::Size {
                width: geometry.width,
                height: geometry.height,
            },
        ),
        damage: Region::single(geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let mut batch = crate::live_session::wm_update_coordinator_batch(transaction);
    batch.transactions.push(frame.clone());
    batch
        .present_submissions
        .push(sophia_x_authority::XAuthorityPresentSubmission {
            transaction,
            surface,
            buffer: dma_buffer,
            x_offset: 0,
            y_offset: 0,
            acquire_fence: None,
            idle_fence: None,
        });

    let mut layout = PersistentLiveLayout::default();
    layout.dma_buf_sizes.insert(
        dma_buffer,
        Size {
            width: geometry.width,
            height: geometry.height,
        },
    );
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
            geometry,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        });
    layout.layout_epochs.record_safe_observation(
        sophia_protocol::SurfaceTransactionKey {
            transaction: TransactionId::from_raw(2317),
            surface,
            target_buffer: BufferSource::CpuBuffer { handle: 15 },
        },
        stale,
        sophia_engine::SurfaceVisualEvidence::BackingSnapshot,
    );
    layout.layout_epochs.set_recovery_extent(surface, stale);
    assert!(layout.surface_requires_admission(surface));
    assert!(!layout.constraint_relayout_required());

    layout.observe_authority_batch(&batch);

    assert!(
        layout.constraint_relayout_required(),
        "the selection must queue the relayout whose commit arms the admission",
    );
    assert_eq!(
        layout.layout_epochs.recovery_extent(surface),
        Some(Size {
            width: geometry.width,
            height: geometry.height,
        }),
        "the stronger candidate must replace the stale admission extent",
    );
}

#[test]
fn a_first_candidate_deferred_from_an_in_flight_layout_queues_recovery() {
    // A physical GLX launch produced its 300x300 first Present after policy had
    // already staged a 1278x1424 placement. Pixel-silent launches are removed
    // from that epoch's requested-size gate, so `pending` existed but could not
    // consume the candidate. Treating any pending epoch as sufficient stranded
    // the candidate permanently: no admission commit and no visible glxgears.
    let surface = SurfaceId::new(82, 1);
    let frame_transaction = TransactionId::from_raw(2319);
    let layout_transaction = TransactionId::from_raw(22);
    let initial = rect(300, 300);
    let target = rect(1278, 1424);
    let dma_buffer = BufferHandle::from_raw(17);
    let frame = SurfaceTransaction {
        input_region: None,
        transaction: frame_transaction,
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: initial,
        presentation_extent: Size {
            width: initial.width,
            height: initial.height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::DmaBuf {
                handle: dma_buffer.raw(),
            },
            Size {
                width: initial.width,
                height: initial.height,
            },
        ),
        damage: Region::single(initial),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let mut batch = crate::live_session::wm_update_coordinator_batch(frame_transaction);
    batch.transactions.push(frame);
    batch
        .present_submissions
        .push(sophia_x_authority::XAuthorityPresentSubmission {
            transaction: frame_transaction,
            surface,
            buffer: dma_buffer,
            x_offset: 0,
            y_offset: 0,
            acquire_fence: None,
            idle_fence: None,
        });

    let mut layout = PersistentLiveLayout::default();
    layout.dma_buf_sizes.insert(
        dma_buffer,
        Size {
            width: initial.width,
            height: initial.height,
        },
    );
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
            geometry: initial,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        });
    layout.layout_epochs.set_pending_target(
        surface,
        Size {
            width: target.width,
            height: target.height,
        },
    );
    layout.pending = Some(PendingLiveWmLayout {
        transaction: layout_transaction,
        layers: vec![LayerSnapshot {
            input_region: None,
            translation: None,
            output: None,
            surface,
            authority_local_id: None,
            namespace: None,
            stack_rank: 0,
            geometry: target,
            source_size: Size {
                width: target.width,
                height: target.height,
            },
            source: BufferSource::None,
            damage: Region::empty(),
            opacity: 1.0,
            crop: None,
            transform: Transform::IDENTITY,
            generation: 1,
            resize_sync: ResizeSyncCapability::ImplicitOnly,
        }],
        // Staging deferred this surface because it had no safe pixels yet.
        requested_sizes: BTreeMap::new(),
        presentation_states: BTreeMap::new(),
        presentation_settlements: BTreeSet::from([surface]),
        configure_deliveries: 1,
        focus: Some(surface),
        deadline: Instant::now() + Duration::from_secs(1),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction: layout_transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![surface],
            },
        },
        moved_surfaces: 1,
        staged_transactions: BTreeMap::new(),
        admission_surfaces: BTreeSet::from([surface]),
        source: Some(crate::live_session::LiveWmProposalSource::Manage(surface)),
        policy_settlement: None,
    });
    assert!(!layout.constraint_relayout_required());

    layout.observe_authority_batch(&batch);

    assert_eq!(
        layout.layout_epochs.recovery_extent(surface),
        Some(Size {
            width: initial.width,
            height: initial.height,
        }),
        "the late candidate must become the bounded admission extent",
    );
    assert!(
        layout.constraint_relayout_required(),
        "the deferred candidate needs a successor layout after the live epoch settles",
    );
    assert!(
        !layout
            .pending
            .as_ref()
            .unwrap()
            .staged_transactions
            .contains_key(&surface),
        "a candidate at 300x300 cannot be smuggled into the older 1278x1424 layout",
    );
}
