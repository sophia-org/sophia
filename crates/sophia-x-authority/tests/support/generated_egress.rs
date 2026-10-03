use super::*;

fn batch(transaction: u64) -> XAuthorityObservedTransactionBatch {
    XAuthorityObservedTransactionBatch {
        client: None,
        admission: None,
        surface_routes: Vec::new(),
        transaction: TransactionId::from_raw(transaction),
        transactions: Vec::new(),
        surface_presentations: Vec::new(),
        presentation_intents: Vec::new(),
        removed_surfaces: Vec::new(),
        surface_output_reservations: Vec::new(),
        cpu_buffer_updates: Vec::new(),
        raster_responses: Vec::new(),
        dma_buf_registrations: Vec::new(),
        fence_registrations: Vec::new(),
        present_submissions: Vec::new(),
        software_present_submissions: Vec::new(),
        released_dma_bufs: Vec::new(),
        released_fences: Vec::new(),
        protocol_errors: Vec::new(),
        expected_protocol_errors: Vec::new(),
        metadata: Vec::new(),
        selection_owner_change: false,
        selection_conversion: false,
    }
}

#[test]
fn both_generators_keep_custody_and_progress_under_backpressure() {
    for first in [XGeneratedEgressKind::Raster, XGeneratedEgressKind::Present] {
        let (sender, receiver) = sync_channel(1);
        let egress = XAuthorityOrderedEgress::new(
            sender,
            Arc::new(AtomicBool::new(false)),
            Arc::new(|_| {}),
        );
        let mut pending = XGeneratedEgress {
            first,
            ..Default::default()
        };
        let mut ticket = 1;
        // Continuously ready producers, with a receiver that drains only one
        // item per pass. The other producer retains exactly one unsent batch.
        let mut delivered = Vec::new();
        let mut owners = BTreeMap::new();
        for _ in 0..32 {
            for kind in pending.admission_order() {
                if pending.vacant(kind) {
                    owners.insert(ticket, kind);
                    pending.insert(
                        kind,
                        XAuthorityBoundedEgressEnvelope::new(
                            TransactionId::from_raw(ticket),
                            Some(batch(ticket)),
                        ),
                    );
                    ticket += 1;
                }
            }
            assert!(pending.try_submit(&egress).unwrap());
            let item = receiver.try_recv().unwrap();
            delivered.push(owners[&item.transaction.raw()]);
            assert!(receiver.try_recv().is_err());
            assert_eq!(pending.slots.iter().flatten().count(), 1);
        }
        assert_eq!(delivered.len(), 32);
        assert!(delivered.chunks_exact(2).all(|pair| pair[0] != pair[1]));
        // Cancelling never drops the envelope. All outstanding work is still
        // available to the collection guard after the wait is cancelled.
        egress.cancel();
        assert!(pending.cancel(&egress).is_empty());
        assert!(!pending.pending());
        let retained = pending.take().collect::<Vec<_>>();
        assert_eq!(retained.len(), 1);
        assert!(retained[0].cancelled && retained[0].batch.is_some());
    }
}

#[test]
fn generated_egress_delivers_in_ticket_order_and_does_not_hole_on_rejection() {
    let (sender, receiver) = sync_channel(2);
    let egress =
        XAuthorityOrderedEgress::new(sender, Arc::new(AtomicBool::new(false)), Arc::new(|_| {}));
    let mut pending = XGeneratedEgress::default();
    // Present execution was rejected: its fresh ticket still advances before
    // the raster response. Reverse slot order must not change ticket order.
    pending.insert(
        XGeneratedEgressKind::Raster,
        XAuthorityBoundedEgressEnvelope::new(TransactionId::from_raw(2), Some(batch(2))),
    );
    pending.insert(
        XGeneratedEgressKind::Present,
        XAuthorityBoundedEgressEnvelope::new(TransactionId::from_raw(1), None),
    );
    assert!(pending.try_submit(&egress).unwrap());
    assert!(!pending.pending());
    assert_eq!(receiver.try_recv().unwrap().transaction.raw(), 2);
    assert!(receiver.try_recv().is_err());
    assert_eq!(egress.report().unwrap().tickets_advanced, 2);
}

#[test]
fn the_private_shelf_reserves_both_envelopes_before_exposure() {
    let store = PrivateSettlementOwner::with_capacity(1);
    let instance = store.reserve_failure_slot().unwrap();
    for ticket in [1, 2] {
        store.retain_unresolved_egress(
            instance,
            XAuthorityBoundedEgressEnvelope::new(
                TransactionId::from_raw(ticket),
                Some(batch(ticket)),
            ),
        );
    }
    store.release_failure_slot();
    assert_eq!(store.unresolved_egress(), Some(2));
    assert_eq!(store.unresolved_egress_capacity(), Some(2));
    assert!(matches!(
        store.reserve_failure_slot(),
        Err(AdmissionRefusal::Saturated)
    ));
    assert_eq!(store.unresolved_egress_obligations().unwrap().len(), 2);
}
