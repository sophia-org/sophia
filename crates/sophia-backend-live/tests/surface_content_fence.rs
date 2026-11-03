use sophia_backend_live::{
    LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888, LiveCpuBufferSource, LiveCpuBufferUpdate,
    LiveProductionAuthorityGroup, LiveProductionCpuBufferUpdate,
};
use sophia_engine::{
    AuthorityTransactionIntake, HeadlessEngine, ProductionSessionCoordinator,
    SurfaceContentAdmission, SurfaceContentStream,
};
use sophia_protocol::{
    AuthorityKind, BufferSource, CommittedSurfaceState, Rect, Region, Size, SurfaceContentFidelity,
    SurfaceContentSet, SurfaceContentVariant, SurfaceId, SurfaceRasterTransform,
    SurfaceTransaction, SurfaceTransactionReadiness, TransactionId, TransactionOutcome,
};
use std::sync::Arc;

fn group(transaction: u64, surface: SurfaceId) -> LiveProductionAuthorityGroup {
    let transaction = TransactionId::from_raw(transaction);
    LiveProductionAuthorityGroup {
        transaction,
        transactions: vec![SurfaceTransaction {
            input_region: None,
            transaction,
            authority: AuthorityKind::SophiaX,
            surface,
            namespace: None,
            target_geometry: Rect {
                x: 2,
                y: 16,
                width: 1276,
                height: 1422,
            },
            presentation_extent: Size {
                width: 1276,
                height: 1422,
            },
            content: sophia_protocol::SurfaceContentSet::singleton(
                BufferSource::CpuBuffer {
                    handle: transaction.raw(),
                },
                sophia_protocol::Size {
                    width: 1276,
                    height: 1422,
                },
            ),

            damage: Region::empty(),
            readiness: SurfaceTransactionReadiness::Ready,
            timeout_msec: 250,
            previous_committed_generation: 1,
        }],
        cpu_buffer_updates: Vec::new(),
        removed_surfaces: Vec::new(),
        present_submissions: Vec::new(),
        software_present_submissions: Vec::new(),
    }
}

#[test]
fn production_group_matches_cpu_updates_to_every_content_variant() {
    let surface = SurfaceId::new(79, 1);
    let transaction = TransactionId::from_raw(798);
    let logical = Size {
        width: 80,
        height: 40,
    };
    let mut authority = group(transaction.raw(), surface);
    authority.transactions[0].target_geometry.width = logical.width;
    authority.transactions[0].target_geometry.height = logical.height;
    authority.transactions[0].content = SurfaceContentSet::new(
        logical,
        vec![
            SurfaceContentVariant {
                variant: 1,
                source: BufferSource::CpuBuffer { handle: 41 },
                pixel_size: logical,
                density_millis: 1_000,
                transform: SurfaceRasterTransform::Normal,
                fidelity: SurfaceContentFidelity::AuthorityRaster,
                damage: Region::single(Rect {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 40,
                }),
            },
            SurfaceContentVariant {
                variant: 2,
                source: BufferSource::CpuBuffer { handle: 42 },
                pixel_size: Size {
                    width: 60,
                    height: 30,
                },
                density_millis: 750,
                transform: SurfaceRasterTransform::Normal,
                fidelity: SurfaceContentFidelity::AuthorityRaster,
                damage: Region::single(Rect {
                    x: 0,
                    y: 0,
                    width: 60,
                    height: 30,
                }),
            },
        ],
    )
    .unwrap();
    for (handle, size) in [
        (41, logical),
        (
            42,
            Size {
                width: 60,
                height: 30,
            },
        ),
    ] {
        authority
            .cpu_buffer_updates
            .push(LiveProductionCpuBufferUpdate::new(
                transaction,
                surface,
                LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
                    pixel_version: None,
                    handle,
                    size,
                    stride: u32::try_from(size.width * 4).unwrap(),
                    format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                    generation: 1,
                    bytes: Arc::new(vec![
                        0;
                        usize::try_from(size.width * size.height * 4).unwrap()
                    ]),
                }),
            ));
    }

    authority.validate().unwrap();
}
#[test]
fn production_group_rejects_every_mismatched_cpu_update_owner_field() {
    let surface = SurfaceId::new(79, 1);
    let transaction = TransactionId::from_raw(798);
    let update = LiveProductionCpuBufferUpdate::new(
        transaction,
        surface,
        LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
            pixel_version: None,
            handle: transaction.raw(),
            size: Size {
                width: 1,
                height: 1,
            },
            stride: 4,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            generation: 1,
            bytes: Arc::new(vec![0; 4]),
        }),
    );
    let malformed = [
        {
            let mut update = update.clone();
            update.identity.transaction = TransactionId::from_raw(799);
            update
        },
        {
            let mut update = update.clone();
            update.identity.surface = SurfaceId::new(80, 1);
            update
        },
        {
            let mut update = update.clone();
            update.identity.handle = transaction.raw().saturating_add(1);
            update
        },
        {
            let mut update = update;
            update.identity.generation = 2;
            update
        },
    ];

    for update in malformed {
        let mut authority = group(transaction.raw(), surface);
        authority.cpu_buffer_updates.push(update);
        assert!(authority.validate().is_err());
    }
}

#[test]
fn in_flight_present_defers_only_later_work_for_the_same_surface() {
    let firefox = SurfaceId::new(80, 1);
    let kitty = SurfaceId::new(81, 1);
    let mut fence = SurfaceContentStream::default();
    let owner = group(799, firefox).transactions[0].key();
    fence.begin(owner).unwrap();

    let mut later_firefox = group(800, firefox);
    later_firefox
        .cpu_buffer_updates
        .push(LiveProductionCpuBufferUpdate::new(
            later_firefox.transaction,
            firefox,
            LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
                pixel_version: None,
                handle: 800,
                size: sophia_protocol::Size {
                    width: 1,
                    height: 1,
                },
                stride: 4,
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                generation: 2,
                bytes: Arc::new(vec![1, 2, 3, 4]),
            }),
        ));
    later_firefox.validate().unwrap();
    let unrelated_kitty = group(801, kitty);
    assert_eq!(
        fence.admit(later_firefox.clone(), [firefox], []).unwrap(),
        SurfaceContentAdmission::Deferred { superseded: None }
    );
    assert_eq!(
        fence.admit(unrelated_kitty.clone(), [kitty], []).unwrap(),
        SurfaceContentAdmission::Ready(unrelated_kitty)
    );

    assert_eq!(fence.deferred_len(), 1);
    let released = fence.finish(owner).unwrap();
    assert_eq!(released, vec![later_firefox]);
    assert_eq!(released[0].cpu_buffer_updates.len(), 1);
    assert_eq!(fence.owner(firefox), None);
}

#[test]
fn surface_removal_can_invalidate_an_in_flight_present_without_deadlock() {
    let firefox = SurfaceId::new(82, 1);
    let transaction = TransactionId::from_raw(802);
    let mut removal = LiveProductionAuthorityGroup {
        transaction,
        transactions: Vec::new(),
        cpu_buffer_updates: Vec::new(),
        removed_surfaces: vec![firefox],
        present_submissions: Vec::new(),
        software_present_submissions: Vec::new(),
    };
    let mut fence = SurfaceContentStream::default();
    fence
        .begin(group(801, firefox).transactions[0].key())
        .unwrap();

    assert!(matches!(
        fence.admit(removal.clone(), [], [firefox]).unwrap(),
        SurfaceContentAdmission::Ready(_)
    ));
    removal
        .transactions
        .push(group(802, SurfaceId::new(83, 1)).transactions.remove(0));
    assert!(matches!(
        fence
            .admit(removal, [SurfaceId::new(83, 1)], [firefox])
            .unwrap(),
        SurfaceContentAdmission::Ready(_)
    ));
}

#[test]
fn shutdown_discards_the_owned_backlog_and_resets_the_fence() {
    let firefox = SurfaceId::new(84, 1);
    let mut fence = SurfaceContentStream::default();
    fence
        .begin(group(802, firefox).transactions[0].key())
        .unwrap();
    fence.admit(group(803, firefox), [firefox], []).unwrap();

    assert_eq!(fence.discard(), 1);
    assert_eq!(fence.owner(firefox), None);
    assert_eq!(fence.deferred_len(), 0);
    assert_eq!(fence.discard(), 0);
}

#[test]
fn later_same_surface_authority_cannot_stale_the_retiring_resize_present() {
    let firefox = SurfaceId::new(85, 1);
    let initial_geometry = Rect {
        x: 2,
        y: 16,
        width: 1280,
        height: 1040,
    };
    let target_geometry = Rect {
        x: 2,
        y: 16,
        width: 1276,
        height: 1422,
    };
    let mut production = ProductionSessionCoordinator::new(HeadlessEngine::default())
        .with_committed_surfaces(vec![CommittedSurfaceState {
            surface: firefox,
            committed_generation: 1,
            geometry: initial_geometry,
            content: sophia_protocol::SurfaceContentSet::singleton(
                BufferSource::CpuBuffer { handle: 804 },
                sophia_protocol::Size {
                    width: initial_geometry.width,
                    height: initial_geometry.height,
                },
            ),
            damage: Region::empty(),
        }]);
    let resize_transaction = TransactionId::from_raw(805);
    let prepared_resize = production.prepare_present_transaction(&SurfaceTransaction {
        input_region: None,
        transaction: resize_transaction,
        authority: AuthorityKind::SophiaX,
        surface: firefox,
        namespace: None,
        target_geometry,
        presentation_extent: sophia_protocol::Size {
            width: target_geometry.width,
            height: target_geometry.height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::DmaBuf { handle: 805 },
            sophia_protocol::Size {
                width: target_geometry.width,
                height: target_geometry.height,
            },
        ),

        damage: Region::empty(),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 1,
    });

    let mut fence = SurfaceContentStream::default();
    let owner = group(805, firefox).transactions[0].key();
    fence.begin(owner).unwrap();
    let later_group = group(806, firefox);
    fence.admit(later_group, [firefox], []).unwrap();

    let retirement = production
        .settle_prepared_retirement(prepared_resize, |commit| {
            Ok::<_, &'static str>(commit.outcome)
        })
        .unwrap();
    assert_eq!(retirement.commit.outcome, TransactionOutcome::Committed);
    assert_eq!(production.committed_surfaces()[0].committed_generation, 2);
    assert_eq!(production.committed_surfaces()[0].geometry, target_geometry);

    let mut released = fence.finish(owner).unwrap();
    released[0].transactions[0].previous_committed_generation = 2;
    let later_commit = production.commit_authority_batches(&[AuthorityTransactionIntake::new(
        released[0].transaction,
        released[0].transactions.clone(),
    )]);
    assert_eq!(later_commit[0].outcome, TransactionOutcome::Committed);
    assert_eq!(production.committed_surfaces()[0].committed_generation, 3);
}
