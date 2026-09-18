use sophia_protocol::{
    SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND, SOPHIA_WM_CAPABILITY_ACTIONS,
    SOPHIA_WM_CAPABILITY_BINDINGS, SOPHIA_WM_CAPABILITY_CONFIGURATION,
    SOPHIA_WM_CAPABILITY_INDICATORS, SOPHIA_WM_CAPABILITY_LAUNCH_PLACEMENT,
    SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION, TransactionId, WmV1ClientHello, WmV1ProjectionBegin,
    WmV1ProjectionChunk, WmV1ProjectionEnd, WmV1SnapshotBegin, WmV1SnapshotChunk, WmV1SnapshotEnd,
};
use sophia_runtime::{
    PolicyConnectionState, PolicySnapshotAssembler, PolicyTransferError, QueuedPolicyProjection,
};

#[test]
fn negotiation_selects_the_shared_revision_and_capabilities() {
    let mut connection = PolicyConnectionState::default();
    connection.connect(7).unwrap();
    let welcome = connection
        .negotiate(&WmV1ClientHello {
            minimum_revision: 1,
            maximum_revision: 3,
            capabilities: SOPHIA_WM_CAPABILITY_BINDINGS
                | SOPHIA_WM_CAPABILITY_ACTIONS
                | SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION
                | (1 << 63),
        })
        .unwrap();

    assert_eq!(welcome.selected_revision, 3);
    assert_eq!(welcome.connection_epoch, 7);
    assert_eq!(
        welcome.capabilities,
        SOPHIA_WM_CAPABILITY_BINDINGS | SOPHIA_WM_CAPABILITY_ACTIONS
    );
    assert_eq!(welcome.max_outputs, 16);
    assert_eq!(welcome.max_surfaces, 1024);
    assert_eq!(welcome.max_bindings, 256);
}

#[test]
fn negotiation_rejects_the_incompatible_revision_one_wire() {
    let mut connection = PolicyConnectionState::default();
    connection.connect(7).unwrap();

    assert_eq!(
        connection.negotiate(&WmV1ClientHello {
            minimum_revision: 1,
            maximum_revision: 1,
            capabilities: 0,
        }),
        Err(PolicyTransferError::UnsupportedRevision)
    );
}

#[test]
fn complete_projection_is_admitted_only_while_its_connection_epoch_is_active() {
    let mut connection = negotiated_connection(1);
    let transaction = TransactionId::from_raw(11);
    connection
        .begin_projection(transaction, projection_begin(1))
        .unwrap();
    connection
        .append_projection_chunk(transaction, projection_chunk(1, 0, 1, 1))
        .unwrap();
    connection
        .append_projection_chunk(transaction, projection_chunk(1, 1, 2, 2))
        .unwrap();
    connection
        .finish_projection(transaction, projection_end(1))
        .unwrap();
    connection.disconnect().unwrap();
    connection.connect(2).unwrap();
    connection
        .negotiate(&WmV1ClientHello {
            minimum_revision: 3,
            maximum_revision: 3,
            capabilities: 0,
        })
        .unwrap();

    assert!(matches!(
        connection.settle_queued(),
        Some(QueuedPolicyProjection::Discarded(queued))
            if queued.connection_epoch == 1 && queued.transaction == transaction
    ));
}

#[test]
fn transaction_identity_cannot_be_reused_within_one_epoch() {
    let mut connection = negotiated_connection(3);
    let transaction = TransactionId::from_raw(19);
    connection
        .begin_projection(transaction, projection_begin(3))
        .unwrap();
    connection
        .append_projection_chunk(transaction, projection_chunk(3, 0, 1, 1))
        .unwrap();
    connection
        .append_projection_chunk(transaction, projection_chunk(3, 1, 2, 2))
        .unwrap();
    connection
        .finish_projection(transaction, projection_end(3))
        .unwrap();
    assert!(matches!(
        connection.settle_queued(),
        Some(QueuedPolicyProjection::Admitted(_))
    ));

    assert_eq!(
        connection.begin_projection(transaction, projection_begin(3)),
        Err(PolicyTransferError::ReusedTransaction)
    );
}

#[test]
fn malformed_chunk_does_not_advance_the_transfer() {
    let mut connection = negotiated_connection(5);
    let transaction = TransactionId::from_raw(23);
    connection
        .begin_projection(transaction, projection_begin(5))
        .unwrap();
    let before = connection.clone();

    assert_eq!(
        connection.append_projection_chunk(transaction, projection_chunk(5, 1, 99, 1)),
        Err(PolicyTransferError::DuplicateOrReorderedChunk)
    );
    assert_eq!(connection, before);
}

#[test]
fn control_messages_are_capability_gated_and_cannot_reuse_transactions() {
    let mut connection = PolicyConnectionState::default();
    connection.connect(6).unwrap();
    connection
        .negotiate(&WmV1ClientHello {
            minimum_revision: 3,
            maximum_revision: 3,
            capabilities: SOPHIA_WM_CAPABILITY_CONFIGURATION,
        })
        .unwrap();
    let transaction = TransactionId::from_raw(31);

    connection
        .admit_control_message(transaction, 6, SOPHIA_WM_CAPABILITY_CONFIGURATION)
        .unwrap();
    assert_eq!(
        connection.admit_control_message(transaction, 6, SOPHIA_WM_CAPABILITY_CONFIGURATION,),
        Err(PolicyTransferError::ReusedTransaction)
    );
    assert_eq!(
        connection.admit_control_message(
            TransactionId::from_raw(32),
            6,
            SOPHIA_WM_CAPABILITY_ACTIONS,
        ),
        Err(PolicyTransferError::UnsupportedCapability)
    );
}

#[test]
fn indicator_records_require_negotiation_and_exact_declared_counts() {
    let mut unsupported = negotiated_connection(9);
    let mut begin = projection_begin(9);
    begin.chunk_count = 3;
    begin.indicator_count = 1;
    assert_eq!(
        unsupported.begin_projection(TransactionId::from_raw(50), begin),
        Err(PolicyTransferError::UnsupportedCapability)
    );

    let mut supported = PolicyConnectionState::default();
    supported.connect(10).unwrap();
    supported
        .negotiate(&WmV1ClientHello {
            minimum_revision: 3,
            maximum_revision: 3,
            capabilities: SOPHIA_WM_CAPABILITY_INDICATORS,
        })
        .unwrap();
    let transaction = TransactionId::from_raw(51);
    let mut begin = projection_begin(10);
    begin.chunk_count = 3;
    begin.indicator_count = 1;
    supported.begin_projection(transaction, begin).unwrap();
    supported
        .append_projection_chunk(transaction, projection_chunk(10, 0, 1, 1))
        .unwrap();
    supported
        .append_projection_chunk(transaction, projection_chunk(10, 1, 2, 2))
        .unwrap();
    supported
        .append_projection_chunk(transaction, projection_chunk(10, 2, 3, 1))
        .unwrap();
    let mut end = projection_end(10);
    end.chunk_count = 3;
    supported.finish_projection(transaction, end).unwrap();
    let Some(QueuedPolicyProjection::Admitted(projection)) = supported.settle_queued() else {
        panic!("indicator projection was not admitted");
    };
    assert_eq!(projection.indicator_count, 1);
}

#[test]
fn snapshot_assembler_requires_exact_order_and_declared_totals() {
    let transaction = TransactionId::from_raw(29);
    let mut assembler = PolicySnapshotAssembler::new(8).unwrap();
    assembler
        .begin(
            transaction,
            WmV1SnapshotBegin {
                connection_epoch: 8,
                scene_generation: 13,
                active_output: 1,
                chunk_count: 3,
                output_count: 1,
                surface_count: 2,
                action_count: 1,
                session_operation_count: 0,
            },
        )
        .unwrap();
    for (ordinal, record_kind, item_count) in [(0, 1, 1), (1, 2, 2), (2, 3, 1)] {
        assembler
            .append(
                transaction,
                WmV1SnapshotChunk {
                    connection_epoch: 8,
                    ordinal,
                    record_kind,
                    item_count,
                    data: vec![ordinal as u8 + 1],
                },
            )
            .unwrap();
    }
    let snapshot = assembler
        .finish(
            transaction,
            WmV1SnapshotEnd {
                connection_epoch: 8,
                scene_generation: 13,
                chunk_count: 3,
            },
        )
        .unwrap();

    assert_eq!(snapshot.output_count, 1);
    assert_eq!(snapshot.surface_count, 2);
    assert_eq!(snapshot.action_count, 1);
    assert_eq!(snapshot.chunks.len(), 3);
}

#[test]
fn snapshot_extensions_append_after_the_uncounted_ordinary_prefix() {
    let transaction = TransactionId::from_raw(30);
    let begin = WmV1SnapshotBegin {
        connection_epoch: 8,
        scene_generation: 13,
        active_output: 1,
        chunk_count: 2,
        output_count: 1,
        surface_count: 1,
        action_count: 0,
        session_operation_count: 0,
    };
    let mut unsupported = PolicySnapshotAssembler::new(8).unwrap();
    unsupported.begin(transaction, begin.clone()).unwrap();
    for (ordinal, record_kind) in [(0, 1), (1, 2)] {
        unsupported
            .append(
                transaction,
                WmV1SnapshotChunk {
                    connection_epoch: 8,
                    ordinal,
                    record_kind,
                    item_count: 1,
                    data: vec![1],
                },
            )
            .unwrap();
    }
    assert_eq!(
        unsupported.append(
            transaction,
            WmV1SnapshotChunk {
                connection_epoch: 8,
                ordinal: 2,
                record_kind: SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND,
                item_count: 1,
                data: vec![1],
            },
        ),
        Err(PolicyTransferError::UnsupportedCapability)
    );

    let mut supported =
        PolicySnapshotAssembler::new_with_capabilities(8, SOPHIA_WM_CAPABILITY_LAUNCH_PLACEMENT)
            .unwrap();
    supported.begin(transaction, begin).unwrap();
    for (ordinal, record_kind) in [
        (0, 1),
        (1, 2),
        (2, SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND),
    ] {
        supported
            .append(
                transaction,
                WmV1SnapshotChunk {
                    connection_epoch: 8,
                    ordinal,
                    record_kind,
                    item_count: 1,
                    data: vec![1],
                },
            )
            .unwrap();
    }
    let snapshot = supported
        .finish(
            transaction,
            WmV1SnapshotEnd {
                connection_epoch: 8,
                scene_generation: 13,
                chunk_count: 2,
            },
        )
        .unwrap();
    assert_eq!(snapshot.chunk_count, 2);
    assert_eq!(snapshot.chunks.len(), 3);
    assert_eq!(snapshot.into_wire_transfer().begin.chunk_count, 2);
}

fn negotiated_connection(epoch: u64) -> PolicyConnectionState {
    let mut connection = PolicyConnectionState::default();
    connection.connect(epoch).unwrap();
    connection
        .negotiate(&WmV1ClientHello {
            minimum_revision: 3,
            maximum_revision: 3,
            capabilities: 0,
        })
        .unwrap();
    connection
}

fn projection_begin(epoch: u64) -> WmV1ProjectionBegin {
    WmV1ProjectionBegin {
        connection_epoch: epoch,
        request_id: 17,
        base_generation: 41,
        active_output: 1,
        chunk_count: 2,
        output_count: 1,
        placement_count: 2,
        indicator_count: 0,
        status_count: 0,
    }
}

fn projection_chunk(
    epoch: u64,
    ordinal: u16,
    record_kind: u16,
    item_count: u32,
) -> WmV1ProjectionChunk {
    WmV1ProjectionChunk {
        connection_epoch: epoch,
        ordinal,
        record_kind,
        item_count,
        data: vec![record_kind as u8],
    }
}

fn projection_end(epoch: u64) -> WmV1ProjectionEnd {
    WmV1ProjectionEnd {
        connection_epoch: epoch,
        request_id: 17,
        base_generation: 41,
        chunk_count: 2,
    }
}

#[test]
fn tab_projection_extensions_require_negotiation_and_follow_the_frozen_prefix() {
    use sophia_protocol::{PROJECTION_TAB_GROUP_RECORD_KIND, SOPHIA_WM_CAPABILITY_TAB_GROUPS};
    for enabled in [false, true] {
        let mut connection = PolicyConnectionState::default();
        connection.connect(1).unwrap();
        connection
            .negotiate(&WmV1ClientHello {
                minimum_revision: 3,
                maximum_revision: 3,
                capabilities: if enabled {
                    SOPHIA_WM_CAPABILITY_TAB_GROUPS
                } else {
                    0
                },
            })
            .unwrap();
        let tx = TransactionId::from_raw(90);
        connection
            .begin_projection(tx, projection_begin(1))
            .unwrap();
        let extension = |ordinal| WmV1ProjectionChunk {
            connection_epoch: 1,
            ordinal,
            record_kind: PROJECTION_TAB_GROUP_RECORD_KIND,
            item_count: 1,
            data: vec![0; 48],
        };
        assert!(
            connection
                .append_projection_chunk(tx, extension(0))
                .is_err()
        );
        connection
            .append_projection_chunk(tx, projection_chunk(1, 0, 1, 1))
            .unwrap();
        connection
            .append_projection_chunk(tx, projection_chunk(1, 1, 2, 2))
            .unwrap();
        let appended = connection.append_projection_chunk(tx, extension(2));
        if enabled {
            appended.unwrap();
            connection.finish_projection(tx, projection_end(1)).unwrap();
            assert!(matches!(
                connection.settle_queued(),
                Some(QueuedPolicyProjection::Admitted(_))
            ));
        } else {
            assert_eq!(appended, Err(PolicyTransferError::UnsupportedCapability));
        }
    }
}

#[test]
fn translation_extensions_require_negotiation_and_preserve_prefix_count() {
    use sophia_protocol::{
        PROJECTION_TRANSLATION_GROUP_RECORD_KIND, SOPHIA_WM_CAPABILITY_TRANSLATION_GROUPS,
    };
    for enabled in [false, true] {
        let mut connection = PolicyConnectionState::default();
        connection.connect(1).unwrap();
        connection
            .negotiate(&WmV1ClientHello {
                minimum_revision: 3,
                maximum_revision: 3,
                capabilities: if enabled {
                    SOPHIA_WM_CAPABILITY_TRANSLATION_GROUPS
                } else {
                    0
                },
            })
            .unwrap();
        let tx = TransactionId::from_raw(90);
        connection
            .begin_projection(tx, projection_begin(1))
            .unwrap();
        let extension = |ordinal| WmV1ProjectionChunk {
            connection_epoch: 1,
            ordinal,
            record_kind: PROJECTION_TRANSLATION_GROUP_RECORD_KIND,
            item_count: 1,
            data: vec![0; 32],
        };
        assert!(
            connection
                .append_projection_chunk(tx, extension(0))
                .is_err()
        );
        connection
            .append_projection_chunk(tx, projection_chunk(1, 0, 1, 1))
            .unwrap();
        connection
            .append_projection_chunk(tx, projection_chunk(1, 1, 2, 2))
            .unwrap();
        let appended = connection.append_projection_chunk(tx, extension(2));
        if enabled {
            appended.unwrap();
            connection.finish_projection(tx, projection_end(1)).unwrap();
            let Some(QueuedPolicyProjection::Admitted(assembled)) = connection.settle_queued()
            else {
                panic!("missing admitted transfer")
            };
            let transfer = assembled.into_wire_transfer();
            assert_eq!(transfer.begin.chunk_count, 2);
            assert_eq!(transfer.end.chunk_count, 2);
            assert_eq!(transfer.chunks.len(), 3);
        } else {
            assert_eq!(appended, Err(PolicyTransferError::UnsupportedCapability));
        }
    }
}

#[test]
fn launch_origin_extensions_require_negotiation_and_preserve_the_prefix() {
    use sophia_protocol::*;
    for enabled in [false, true] {
        let mut connection = PolicyConnectionState::default();
        connection.connect(1).unwrap();
        connection
            .negotiate(&WmV1ClientHello {
                minimum_revision: 3,
                maximum_revision: 3,
                capabilities: if enabled {
                    SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN
                } else {
                    0
                },
            })
            .unwrap();
        let tx = TransactionId::from_raw(90);
        connection
            .begin_projection(tx, projection_begin(1))
            .unwrap();
        let extension = |ordinal| {
            encode_wm_launch_contexts(
                &[PolicyLaunchContext {
                    surface: SurfaceId::new(0, 1),
                    epoch: 1,
                    token: 7,
                }],
                1,
                ordinal,
            )
            .unwrap()
            .remove(0)
        };
        assert!(
            connection
                .append_projection_chunk(tx, extension(0))
                .is_err()
        );
        connection
            .append_projection_chunk(tx, projection_chunk(1, 0, 1, 1))
            .unwrap();
        connection
            .append_projection_chunk(tx, projection_chunk(1, 1, 2, 2))
            .unwrap();
        let appended = connection.append_projection_chunk(tx, extension(2));
        if enabled {
            appended.unwrap();
            connection.finish_projection(tx, projection_end(1)).unwrap();
        } else {
            assert_eq!(appended, Err(PolicyTransferError::UnsupportedCapability));
        }
    }
}

#[test]
fn snapshot_launch_origins_are_gated_bounded_and_transactional() {
    use sophia_protocol::*;
    for capabilities in [0, SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN] {
        let mut assembler =
            PolicySnapshotAssembler::new_with_capabilities(8, capabilities).unwrap();
        let transaction = TransactionId::from_raw(41);
        assembler
            .begin(
                transaction,
                WmV1SnapshotBegin {
                    connection_epoch: 8,
                    scene_generation: 13,
                    active_output: 1,
                    chunk_count: 2,
                    output_count: 1,
                    surface_count: 1,
                    action_count: 0,
                    session_operation_count: 0,
                },
            )
            .unwrap();
        let extension = |ordinal| WmV1SnapshotChunk {
            connection_epoch: 8,
            ordinal,
            record_kind: SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND,
            item_count: 1,
            data: encode_wm_launch_context_records(&[PolicyLaunchContext {
                surface: SurfaceId::new(0, 1),
                epoch: 8,
                token: 9,
            }])
            .unwrap(),
        };
        assert!(assembler.append(transaction, extension(0)).is_err());
        for (ordinal, record_kind) in [(0, 1), (1, 2)] {
            assembler
                .append(
                    transaction,
                    WmV1SnapshotChunk {
                        connection_epoch: 8,
                        ordinal,
                        record_kind,
                        item_count: 1,
                        data: vec![1],
                    },
                )
                .unwrap();
        }
        if capabilities == 0 {
            assert_eq!(
                assembler.append(transaction, extension(2)),
                Err(PolicyTransferError::UnsupportedCapability)
            );
        } else {
            let mut oversized = extension(2);
            oversized.item_count = 1025;
            oversized.data = vec![0; 1025 * 24];
            assert!(assembler.append(transaction, oversized).is_err());
            let mut malformed = extension(2);
            malformed.data.pop();
            assert!(assembler.append(transaction, malformed).is_err());
            assembler.append(transaction, extension(2)).unwrap();
            let transfer = assembler
                .finish(
                    transaction,
                    WmV1SnapshotEnd {
                        connection_epoch: 8,
                        scene_generation: 13,
                        chunk_count: 2,
                    },
                )
                .unwrap()
                .into_wire_transfer();
            assert_eq!(transfer.begin.chunk_count, 2);
            assert_eq!(transfer.chunks.len(), 3);
        }
    }
}

#[test]
fn output_launch_context_requires_negotiated_origin_and_a_bounded_extension_tail() {
    use sophia_protocol::*;
    for capabilities in [
        0,
        SOPHIA_WM_CAPABILITY_OUTPUT_LAUNCH_CONTEXT,
        SOPHIA_WM_CAPABILITY_OUTPUT_LAUNCH_CONTEXT | SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN,
    ] {
        let mut connection = PolicyConnectionState::default();
        connection.connect(1).unwrap();
        let welcome = connection
            .negotiate(&WmV1ClientHello {
                minimum_revision: 3,
                maximum_revision: 3,
                capabilities,
            })
            .unwrap();
        let enabled = capabilities & SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN != 0;
        assert_eq!(
            welcome.capabilities & SOPHIA_WM_CAPABILITY_OUTPUT_LAUNCH_CONTEXT != 0,
            enabled
        );
        let tx = TransactionId::from_raw(90);
        connection
            .begin_projection(tx, projection_begin(1))
            .unwrap();
        let extension = |ordinal| WmV1ProjectionChunk {
            connection_epoch: 1,
            ordinal,
            record_kind: PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND,
            item_count: 1,
            data: vec![1; 32],
        };
        assert!(
            connection
                .append_projection_chunk(tx, extension(0))
                .is_err()
        );
        connection
            .append_projection_chunk(tx, projection_chunk(1, 0, 1, 1))
            .unwrap();
        connection
            .append_projection_chunk(tx, projection_chunk(1, 1, 2, 2))
            .unwrap();
        if enabled {
            let mut oversized = extension(2);
            oversized.item_count = 17;
            oversized.data = vec![1; 17 * 32];
            assert!(connection.append_projection_chunk(tx, oversized).is_err());
            connection
                .append_projection_chunk(tx, extension(2))
                .unwrap();
            connection.finish_projection(tx, projection_end(1)).unwrap();
            assert!(matches!(
                connection.settle_queued(),
                Some(QueuedPolicyProjection::Admitted(_))
            ));
        } else {
            assert_eq!(
                connection.append_projection_chunk(tx, extension(2)),
                Err(PolicyTransferError::UnsupportedCapability)
            );
        }
    }
}
