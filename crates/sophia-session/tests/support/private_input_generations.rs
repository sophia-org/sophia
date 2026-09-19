//! Supplied authority batches against the real Engine commit owner. Component
//! evidence only; the public Session acceptance control supplies the wire path.

use super::*;
use sophia_engine::HeadlessEngine;
use sophia_protocol::*;
use sophia_x_authority::*;

fn admission() -> ClientAdmissionContext {
    ClientAdmissionContext::new(
        ClientAdmissionId::from_raw(1),
        NamespaceContext::new(
            NamespaceId::from_raw(1),
            NamespaceProfile::Confined,
            NamespaceCapabilities::NONE,
        )
        .unwrap(),
        ClientAuthProvenance::new(ClientAuthenticationMethod::PeerCredentials, 1).unwrap(),
    )
    .unwrap()
}

fn live() -> Vec<PrivateAdmittedConnection> {
    vec![PrivateAdmittedConnection {
        client: XServerFrontendClientId::from_raw(1),
        admission: admission().client_id,
        namespace: NamespaceId::from_raw(1),
        connection_generation: 1,
        closed: false,
        lifecycle_open: true,
        grants: 1,
    }]
}

fn batch(id: u64, previous: u64) -> XAuthorityObservedTransactionBatch {
    let transaction = TransactionId::from_raw(id);
    let surface = SurfaceId::new(1, 1);
    let mut response = XAuthorityResponsePacket::accepted(transaction);
    response.transactions.push(SurfaceTransaction {
        transaction,
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: Some(NamespaceId::from_raw(1)),
        target_geometry: Rect {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        },
        input_region: None,
        content: SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: id },
            Size {
                width: 8,
                height: 8,
            },
        ),
        presentation_extent: Size {
            width: 8,
            height: 8,
        },
        damage: Region::empty(),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: previous,
    });
    let mut batch = XAuthorityObservedTransactionBatch::from_dispatch_result(&XDispatchResult {
        response: Some(response),
        outputs: Vec::new(),
        metadata_candidates: Vec::new(),
    })
    .unwrap();
    batch.client = Some(live()[0].client);
    batch.admission = Some(admission());
    batch
        .surface_routes
        .push(XAuthoritySurfaceRouteObservation {
            surface,
            client: live()[0].client,
            admission: Some(admission()),
        });
    batch
}

#[test]
fn source_generation_is_separate() {
    let engine = HeadlessEngine::default();
    let mut committed = Vec::new();
    let mut ledger = GenerationLedger::default();
    for (id, source, predecessor) in [(10, 1, 0), (11, 2, 1)] {
        let batch = batch(id, source);
        let intake = ledger.prepare(&batch, &committed, &live()).unwrap();
        assert_eq!(
            intake.transactions[0].previous_committed_generation,
            predecessor
        );
        let commit = intake.commit(&engine, &mut committed);
        assert_eq!(commit.outcome, TransactionOutcome::Committed);
        assert_eq!(commit.applied_surfaces, vec![SurfaceId::new(1, 1)]);
        ledger.record(&batch, &[commit]);
        assert_eq!(committed[0].committed_generation, predecessor + 1);
        assert_eq!(
            committed[0].buffer(),
            BufferSource::CpuBuffer { handle: id }
        );
        assert!(matches!(
            ledger.prepare(&batch, &committed, &live()),
            Err(TransactionOutcome::RejectedStaleSurface)
        ));
    }
}

#[test]
fn rejected_source_is_not_reprepared_but_new_source_can_commit() {
    let engine = HeadlessEngine::default();
    let mut committed = Vec::new();
    let mut ledger = GenerationLedger::default();
    let mut rejected = batch(10, 1);
    rejected.transactions[0].readiness = SurfaceTransactionReadiness::Failed;
    let intake = ledger.prepare(&rejected, &committed, &live()).unwrap();
    let commit = intake.commit(&engine, &mut committed);
    assert_eq!(commit.outcome, TransactionOutcome::RejectedStaleSurface);
    ledger.record(&rejected, &[commit]);
    assert!(committed.is_empty());
    rejected.transactions[0].readiness = SurfaceTransactionReadiness::Ready;
    assert!(ledger.prepare(&rejected, &committed, &live()).is_err());
    let next = ledger.prepare(&batch(11, 2), &committed, &live()).unwrap();
    assert_eq!(
        next.commit(&engine, &mut committed).outcome,
        TransactionOutcome::Committed
    );
}

#[test]
fn stale_admission_and_duplicate_surface_refuse_the_whole_batch() {
    let ledger = GenerationLedger::default();
    let original = batch(10, 1);
    let mut rows = live();
    rows[0].admission = ClientAdmissionId::from_raw(2);
    assert!(ledger.prepare(&original, &[], &rows).is_err());
    let mut duplicate = original;
    duplicate
        .transactions
        .push(duplicate.transactions[0].clone());
    assert!(ledger.prepare(&duplicate, &[], &live()).is_err());
}

#[test]
fn a_removed_incarnation_cannot_be_reprepared() {
    let engine = HeadlessEngine::default();
    let mut committed = Vec::new();
    let mut ledger = GenerationLedger::default();
    let original = batch(10, 1);
    let commit = ledger
        .prepare(&original, &committed, &live())
        .unwrap()
        .commit(&engine, &mut committed);
    ledger.record(&original, &[commit]);
    let mut removal = batch(11, 2);
    removal.transactions.clear();
    removal.removed_surfaces.push(SurfaceId::new(1, 1));
    let commit = ledger
        .prepare(&removal, &committed, &live())
        .unwrap()
        .commit(&engine, &mut committed);
    ledger.record(&removal, &[commit]);
    assert!(committed.is_empty());
    assert!(ledger.prepare(&batch(12, 3), &committed, &live()).is_err());
}

#[test]
fn engine_still_rejects_an_overtaken_prepared_candidate() {
    let engine = HeadlessEngine::default();
    let ledger = GenerationLedger::default();
    let mut committed = Vec::new();
    let intake = ledger.prepare(&batch(10, 1), &committed, &live()).unwrap();
    let prepared =
        engine.prepare_surface_transactions(intake.transaction, &intake.transactions, &committed);
    let other = ledger.prepare(&batch(11, 2), &committed, &live()).unwrap();
    assert_eq!(
        other.commit(&engine, &mut committed).outcome,
        TransactionOutcome::Committed
    );
    let result = engine.apply_prepared_surface_commit(prepared, &mut committed);
    assert_eq!(result.outcome, TransactionOutcome::RejectedStaleSurface);
    assert_eq!(
        committed[0].buffer(),
        BufferSource::CpuBuffer { handle: 11 }
    );
}

#[test]
fn a_rejected_update_does_not_replace_the_committed_mapping_source() {
    let engine = HeadlessEngine::default();
    let mut ledger = GenerationLedger::default();
    let mut committed = Vec::new();
    let first = batch(10, 1);
    let commit = ledger
        .prepare(&first, &committed, &live())
        .unwrap()
        .commit(&engine, &mut committed);
    ledger.record(&first, &[commit]);
    let surface = first.transactions[0].surface;
    assert_eq!(
        ledger.committed_transaction(surface),
        Some(first.transaction)
    );

    let mut failed = batch(11, 2);
    failed.transactions[0].readiness = SurfaceTransactionReadiness::Failed;
    let commit = ledger
        .prepare(&failed, &committed, &live())
        .unwrap()
        .commit(&engine, &mut committed);
    assert_eq!(commit.outcome, TransactionOutcome::RejectedStaleSurface);
    ledger.record(&failed, &[commit]);
    assert_eq!(
        ledger.committed_transaction(surface),
        Some(first.transaction)
    );
    assert!(ledger.prepare(&failed, &committed, &live()).is_err());
    assert_eq!(
        committed[0].buffer(),
        BufferSource::CpuBuffer { handle: 10 }
    );
}
