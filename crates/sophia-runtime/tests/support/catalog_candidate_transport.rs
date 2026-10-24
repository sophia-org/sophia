//! Supplied negotiation/allocation and renderer completion; actual assembly,
//! byte lease and response FIFO. No native presentation or dock launch.
use super::super::super::outbound::OutboundRecord;
use super::{Fixture, GRANT};
use crate::shell_transport::wire::CatalogCandidatePart;
use crate::test_content_isb as isb;
use crate::{ContentAllocationSnapshot, ContentCandidateContext};
use sophia_protocol::shell_files::*;
use sophia_protocol::*;

const OUTPUT: ContentOutputId = ContentOutputId {
    id: 1,
    generation: 1,
};
const ALLOCATION: ContentAllocationId = ContentAllocationId {
    id: 1,
    generation: 1,
};
const RESOURCE: ContentResourceId = ContentResourceId {
    id: 1,
    generation: 1,
};
fn tx(n: u64) -> TransactionId {
    TransactionId::from_raw(n)
}
fn catalog() -> ShellApplicationCatalog {
    ShellApplicationCatalog {
        connection_epoch: GRANT.connection_epoch,
        generation: 4,
        entries: vec![],
    }
}
fn begin() -> CatalogCandidateBegin {
    CatalogCandidateBegin {
        catalog_generation: 4,
        content: ContentCandidateBegin {
            grant: GRANT,
            candidate_generation: 1,
            output: OUTPUT,
            facts_generation: 1,
            pacing_permit: 1,
            interaction_generation: 1,
            surface_count: 1,
            placement_count: 1,
            target_count: 0,
        },
    }
}
fn fixture() -> Fixture {
    let mut f = Fixture::with_control_limit(16);
    let candidates = f.epochs.active_candidates_mut(GRANT).unwrap();
    candidates.grant_permit(tx(1), OUTPUT, 1, 1, 0).unwrap();
    candidates.take_event().unwrap();
    f
}
fn candidate(value: CatalogCandidateBegin) -> CatalogContentCandidate {
    let v = value.content;
    CatalogContentCandidate {
        catalog_generation: value.catalog_generation,
        candidate: ContentCandidate {
            grant: v.grant,
            candidate_generation: v.candidate_generation,
            output: v.output,
            facts_generation: v.facts_generation,
            pacing_permit: v.pacing_permit,
            interaction_generation: v.interaction_generation,
            surfaces: vec![ContentSurface {
                allocation: ALLOCATION,
                scale_generation: 1,
                role: 1,
                edge: 1,
                margins: ContentMargins::default(),
                reservation_extent: 0,
                parent_surface_index: u16::MAX,
                anchor_parent_rect: ContentPixelRect::default(),
            }],
            placements: vec![ContentPlacement {
                resource: RESOURCE,
                surface_index: 0,
                destination_x_px: 0,
                destination_y_px: 0,
            }],
            targets: vec![],
        },
    }
}
fn enqueue_candidate(f: &mut Fixture, value: CatalogCandidateBegin) {
    f.peer.submit(
        &mut f.transport,
        ShellFileKind::CatalogCandidate,
        |header| {
            encode_shell_file_catalog_candidate(
                header,
                &ShellFileCatalogCandidate {
                    transaction: tx(2),
                    candidate: candidate(value),
                },
            )
            .unwrap()
        },
    );
}
/// The same submission under a caller-chosen transaction.
fn enqueue_candidate_at(f: &mut Fixture, value: CatalogCandidateBegin, transaction: TransactionId) {
    f.peer.submit(
        &mut f.transport,
        ShellFileKind::CatalogCandidate,
        |header| {
            encode_shell_file_catalog_candidate(
                header,
                &ShellFileCatalogCandidate {
                    transaction,
                    candidate: candidate(value),
                },
            )
            .unwrap()
        },
    );
}
fn front(f: &mut Fixture) -> CatalogCandidatePart {
    let (transaction, part) = f.transport.peek_catalog_candidate().unwrap().unwrap();
    assert_eq!(transaction, tx(2));
    part
}
fn allocation() -> ContentAllocationSnapshot {
    ContentAllocationSnapshot {
        native_opening: None,
        output: OUTPUT,
        allocation: ALLOCATION,
        scale_generation: 1,
        scale_numerator: 1,
        scale_denominator: 1,
        role: 1,
        edge: 1,
        margins: ContentMargins::default(),
        logical: ContentLogicalRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        pixel: ContentPixelRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        parent: ContentAllocationId::default(),
        anchor_parent_rect: ContentPixelRect::default(),
        allowed_reservation_extent: 0,
    }
}
fn context(allocations: &[ContentAllocationSnapshot]) -> ContentCandidateContext<'_> {
    ContentCandidateContext {
        output: OUTPUT,
        facts_generation: 1,
        interaction_generation: 1,
        allocations,
    }
}

#[test]
fn catalog_wire_assembly_reaches_real_submission_and_exact_response_fifo() {
    let mut f = fixture();
    let resources = f.epochs.resources_mut(GRANT).unwrap();
    isb::accept(
        resources,
        tx(10),
        ContentResourceBegin {
            grant: GRANT,
            resource: RESOURCE,
            width_px: 1,
            height_px: 1,
            rendered_scale_numerator: 1,
            rendered_scale_denominator: 1,
            pixel_format: 1,
            chunk_count: 1,
            total_bytes: 4,
        },
        &[0, 0, 0, 255],
        0,
    );
    while resources.take_event().is_some() {}
    enqueue_candidate(&mut f, begin());
    let allocations = [allocation()];
    f.transport
        .content_limits
        .as_mut()
        .unwrap()
        .max_frames_per_service_tick = 1;
    assert_eq!(
        f.transport
            .service_catalog_candidates(&mut f.epochs, &[], &catalog(), 0)
            .unwrap(),
        1
    );
    assert!(matches!(front(&mut f), CatalogCandidatePart::Chunk(_)));
    f.transport
        .content_limits
        .as_mut()
        .unwrap()
        .max_frames_per_service_tick = 32;
    // Missing or ambiguous current context must leave End and the assembly owned.
    assert!(
        f.transport
            .service_catalog_candidates(&mut f.epochs, &[], &catalog(), 0)
            .is_err()
    );
    assert!(matches!(front(&mut f), CatalogCandidatePart::End(_)));
    assert_eq!(
        f.epochs
            .active_candidates(GRANT)
            .unwrap()
            .assembling_output(1),
        Some(OUTPUT)
    );
    assert!(
        f.transport
            .service_catalog_candidates(&mut f.epochs, &[context(&allocations); 2], &catalog(), 0)
            .is_err()
    );
    assert!(matches!(front(&mut f), CatalogCandidatePart::End(_)));
    assert_eq!(
        f.transport
            .service_catalog_candidates(&mut f.epochs, &[context(&allocations)], &catalog(), 0)
            .unwrap(),
        1
    );
    let mut stale = catalog();
    stale.generation += 1;
    assert!(
        f.transport
            .connection(&mut f.epochs)
            .begin_catalog_submission(1, context(&allocations), &stale, 0)
            .is_err()
    );
    assert_eq!(
        f.epochs
            .active_candidates(GRANT)
            .unwrap()
            .pending_candidate_count(),
        1
    );
    let bundle = f
        .transport
        .connection(&mut f.epochs)
        .begin_catalog_submission(1, context(&allocations), &catalog(), 0)
        .unwrap();
    assert_eq!(
        bundle.resource(RESOURCE).unwrap().cpu_bytes().unwrap(),
        &[0, 0, 0, 255]
    );
    assert_eq!(bundle.persistent_catalog.unwrap().catalog_generation, 4);
    f.transport
        .content_prepared(&mut f.epochs, GRANT, OUTPUT, 1, 1, 1, 0)
        .unwrap();
    f.transport
        .content_presented(&mut f.epochs, GRANT, OUTPUT, 1, 8, 1, 1)
        .unwrap();
    assert_eq!(f.transport.output.records(), 2);
    let first = f.transport.output.front().unwrap().record.clone();
    let OutboundRecord::Content(transaction, ShellContentRecord::CandidateOutcome(value)) = &first
    else {
        panic!("prepared outcome is typed");
    };
    assert_eq!(*transaction, tx(2));
    assert_eq!(value.kind, 1);
    assert_eq!(value.candidate_generation, 1);
    let mut presented = value.clone();
    presented.kind = 2;
    presented.presentation_epoch = 8;
    f.send();
    f.peer.expect(&mut f.transport, &first);
    f.peer.expect(
        &mut f.transport,
        &OutboundRecord::Content(tx(2), ShellContentRecord::CandidateOutcome(presented)),
    );
    f.peer.quiet(&mut f.transport);
    assert!(f.transport.output.front().is_none());
    f.transport.disconnect(&mut f.epochs).unwrap();
    f.epochs.collect();
    assert_eq!(f.epochs.accounting().memory.resident, 4);
    drop(bundle);
    f.epochs.collect();
    assert_eq!(f.epochs.reserved_bytes(), 0);
}

/// Exactly the Prepared (kind 1) and Presented (kind 2, `epoch`) outcomes of
/// `candidate` under `transaction` are queued; the peer receives both.
fn expect_outcomes(f: &mut Fixture, transaction: TransactionId, candidate: u64, epoch: u64) {
    assert_eq!(f.transport.output.records(), 2);
    let prepared = f.transport.output.front().unwrap().record.clone();
    let OutboundRecord::Content(queued, ShellContentRecord::CandidateOutcome(value)) = &prepared
    else {
        panic!("prepared outcome is typed");
    };
    assert_eq!(*queued, transaction);
    assert_eq!((value.kind, value.candidate_generation), (1, candidate));
    let mut presented = value.clone();
    presented.kind = 2;
    presented.presentation_epoch = epoch;
    f.send();
    f.peer.expect(&mut f.transport, &prepared);
    f.peer.expect(
        &mut f.transport,
        &OutboundRecord::Content(transaction, ShellContentRecord::CandidateOutcome(presented)),
    );
    f.peer.quiet(&mut f.transport);
    assert!(f.transport.output.front().is_none());
}
/// A catalog change that leaves the dock's pixels unchanged needs no new
/// upload: a second candidate under a fresh permit, catalog generation and
/// interaction generation places the already-resident resource, is submitted
/// with the same bytes, and reaches Presented while nothing retires.
#[test]
fn a_presented_catalog_resource_accepts_a_second_candidate_without_re_upload() {
    let mut f = fixture();
    let resources = f.epochs.resources_mut(GRANT).unwrap();
    isb::accept(
        resources,
        tx(10),
        ContentResourceBegin {
            grant: GRANT,
            resource: RESOURCE,
            width_px: 1,
            height_px: 1,
            rendered_scale_numerator: 1,
            rendered_scale_denominator: 1,
            pixel_format: 1,
            chunk_count: 1,
            total_bytes: 4,
        },
        &[0, 0, 0, 255],
        0,
    );
    while resources.take_event().is_some() {}
    let allocations = [allocation()];
    enqueue_candidate(&mut f, begin());
    // Begin, Chunk and End are consumed in one tick into one pending candidate.
    f.transport
        .service_catalog_candidates(&mut f.epochs, &[context(&allocations)], &catalog(), 0)
        .unwrap();
    assert_eq!(
        f.epochs
            .active_candidates(GRANT)
            .unwrap()
            .pending_candidate_count(),
        1
    );
    let first = f
        .transport
        .connection(&mut f.epochs)
        .begin_catalog_submission(1, context(&allocations), &catalog(), 0)
        .unwrap();
    f.transport
        .content_prepared(&mut f.epochs, GRANT, OUTPUT, 1, 1, 1, 0)
        .unwrap();
    f.transport
        .content_presented(&mut f.epochs, GRANT, OUTPUT, 1, 8, 1, 1)
        .unwrap();
    drop(first);
    f.epochs.collect();
    expect_outcomes(&mut f, tx(2), 1, 8);
    assert_eq!(f.epochs.accounting().memory.resident, 4);

    // Fresh demand/permit, then the reuse candidate under catalog generation 5.
    let candidates = f.epochs.active_candidates_mut(GRANT).unwrap();
    candidates.grant_permit(tx(3), OUTPUT, 2, 2, 0).unwrap();
    candidates.take_event().unwrap();
    let mut reuse = begin();
    reuse.catalog_generation = 5;
    reuse.content.candidate_generation = 2;
    reuse.content.pacing_permit = 2;
    reuse.content.interaction_generation = 2;
    let mut current = catalog();
    current.generation = 5;
    let mut context = context(&allocations);
    context.interaction_generation = 2;
    enqueue_candidate_at(&mut f, reuse, tx(4));
    f.transport
        .service_catalog_candidates(&mut f.epochs, &[context], &current, 0)
        .unwrap();
    assert_eq!(
        f.epochs
            .active_candidates(GRANT)
            .unwrap()
            .pending_candidate_count(),
        1
    );
    let bundle = f
        .transport
        .connection(&mut f.epochs)
        .begin_catalog_submission(2, context, &current, 0)
        .unwrap();
    assert_eq!(
        bundle.resource(RESOURCE).unwrap().cpu_bytes().unwrap(),
        &[0, 0, 0, 255],
        "the resident bytes are placed again without a new upload"
    );
    assert_eq!(bundle.persistent_catalog.unwrap().catalog_generation, 5);
    f.transport
        .content_prepared(&mut f.epochs, GRANT, OUTPUT, 2, 1, 1, 0)
        .unwrap();
    f.transport
        .content_presented(&mut f.epochs, GRANT, OUTPUT, 2, 9, 1, 1)
        .unwrap();
    expect_outcomes(&mut f, tx(4), 2, 9);
    drop(bundle);
    f.epochs.collect();
    assert_eq!(f.epochs.accounting().memory.resident, 4, "nothing retired");
    assert_eq!(f.epochs.accounting().memory.retiring, 0);
    assert!(
        f.epochs
            .resources_mut(GRANT)
            .unwrap()
            .take_event()
            .is_none()
    );
    f.transport.disconnect(&mut f.epochs).unwrap();
    f.epochs.collect();
    assert_eq!(f.epochs.reserved_bytes(), 0);
}

#[test]
fn candidate_intake_refuses_wrong_family_role_and_grant_before_dequeue() {
    let mut f = fixture();
    let mut value = begin();
    value.content.grant.connection_epoch += 1;
    enqueue_candidate(&mut f, value);
    assert!(matches!(
        f.transport
            .service_catalog_candidates(&mut f.epochs, &[], &catalog(), 0),
        Err(super::ShellTransportError::WrongContentGrant)
    ));
    assert_eq!(f.inbox(), 1);
    assert_eq!(
        f.epochs
            .active_candidates(GRANT)
            .unwrap()
            .assembling_output(1),
        None
    );
    f.transport
        .files_mut()
        .unwrap()
        .export_mut()
        .discard_catalog_candidate_parts();
    f.peer
        .submit(&mut f.transport, ShellFileKind::Candidate, |header| {
            encode_shell_file_candidate(
                header,
                &ShellFileCandidate {
                    transaction: tx(2),
                    candidate: candidate(begin()).candidate,
                },
            )
            .unwrap()
        });
    assert!(matches!(
        f.transport
            .service_catalog_candidates(&mut f.epochs, &[], &catalog(), 0),
        Err(super::ShellTransportError::WrongContentRecord)
    ));
    assert_eq!(f.inbox(), 1);
    f.transport.capabilities = 0;
    assert!(matches!(
        f.transport
            .service_catalog_candidates(&mut f.epochs, &[], &catalog(), 0),
        Err(super::ShellTransportError::MissingCapability)
    ));
    assert_eq!(f.inbox(), 1);
}

#[test]
fn candidate_budget_defers_intake_and_stale_begin_emits_one_terminal() {
    let mut f = fixture();
    enqueue_candidate(&mut f, begin());
    f.transport
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 1;
    assert_eq!(
        f.transport
            .service_catalog_candidates(&mut f.epochs, &[], &catalog(), 0)
            .unwrap(),
        0
    );
    assert_eq!(f.inbox(), 1);
    f.transport
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 16;
    let mut current = catalog();
    current.generation += 1;
    assert_eq!(
        f.transport
            .service_catalog_candidates(&mut f.epochs, &[], &current, 0)
            .unwrap(),
        1
    );
    assert_eq!(f.inbox(), 0);
    assert_eq!(f.transport.output.records(), 1);
    let Some(OutboundRecord::Content(transaction, record)) = f
        .transport
        .output
        .front()
        .map(|queued| queued.record.clone())
    else {
        panic!("the outcome is queued as a typed content record");
    };
    assert_eq!(transaction, tx(2));
    assert!(matches!(record, ShellContentRecord::CandidateOutcome(v) if v.kind == 3));
    assert_eq!(
        f.transport
            .service_catalog_candidates(&mut f.epochs, &[], &current, 0)
            .unwrap(),
        0
    );
    assert_eq!(f.transport.output.records(), 1);
}
