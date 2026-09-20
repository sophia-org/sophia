// The release a departing source owes: what its custody says before anything
// runs, and that a suppressed release says the opposite on its own ground.

/// The admitted delivery of a capsule, for controls that only ever assemble
/// admitted capsules. Production reads `admitted_delivery`, which does not
/// presume; this presumes, and says so when it is wrong.
#[cfg(unix)]
impl XAuthorityOrderedDelivery {
    pub(crate) fn delivery(&self) -> XAuthorityInputDeliveryId {
        self.admitted_delivery()
            .expect("this control assembled an admitted capsule")
    }
}

/// A custody made for an event nobody admitted owes that event until it is
/// enqueued; one made with neither cell owes nothing.
#[test]
fn an_unadmitted_custody_owes_an_event_and_a_suppressed_one_does_not() {
    let suppressed = PrivateDeliveryCustody::new(1, None);
    assert!(!suppressed.owes_event());
    assert!(!suppressed.handover_unfinished());
    assert!(!suppressed.owes_handover());
    assert!(suppressed.writer_outcome().is_none());

    let mut owed = PrivateDeliveryCustody::unadmitted(1);
    assert!(owed.owes_event());
    assert!(owed.handover_unfinished() && owed.owes_handover());
    assert!(owed.handover_permitted());
    assert!(owed.completion.is_none() && owed.unadmitted.is_some());
    assert!(owed.writer_outcome().is_none());

    // ENQUEUED ENDS THE HANDOVER, as for a requested event.
    owed.dispatch = PrivateDispatchPhase::Enqueued;
    assert!(!owed.handover_unfinished());
    assert!(!owes_after_termination(PrivateDeliveryCustody::unadmitted(2)));

    // AN INDETERMINATE HANDOVER IS UNFINISHED BUT NOT REPEATABLE: it stays in
    // the ordering comparison and is not offered again.
    let mut begun = PrivateDeliveryCustody::unadmitted(3);
    begun.dispatch = PrivateDispatchPhase::Indeterminate;
    assert!(begun.handover_unfinished() && !begun.handover_permitted());

    // THE ADMITTED SHAPE ANSWERS THE SAME QUESTIONS THE SAME WAY, so the
    // ordering sites cannot tell the two apart, which is the point.
    let admitted = PrivateDeliveryCustody::new(4, Some(Arc::default()));
    assert!(admitted.owes_event() && admitted.handover_unfinished());
}

fn owes_after_termination(mut custody: PrivateDeliveryCustody) -> bool {
    custody.recipient_termination = true;
    custody.handover_unfinished()
}

/// The writer answers an unadmitted custody through its own cell, once, and
/// the custody reads it back the way it reads an admitted answer.
#[test]
fn an_unadmitted_custody_is_answered_through_its_own_cell_once() {
    let (deliveries, _receipts) = std::sync::mpsc::channel();
    let recovery = InputRecovery::new(4, Some(deliveries), Arc::default());
    let client = XServerFrontendClientId::from_raw(7);
    let custody = PrivateDeliveryCustody::unadmitted(1);
    let finalizer = custody
        .finalizer(&recovery, None, client)
        .expect("an unadmitted custody has something to answer for it");
    assert_eq!(
        finalizer.finalize(XAuthorityInputDeliveryOutcome::Flushed),
        PrivateAdjudication::Answered
    );
    assert_eq!(
        custody.writer_outcome(),
        Some(XAuthorityInputDeliveryOutcome::Flushed)
    );
    // A second answer for one write is a contradiction; the first stands.
    assert_eq!(
        finalizer.finalize(XAuthorityInputDeliveryOutcome::WriteFailed),
        PrivateAdjudication::AlreadyAnswered
    );
    assert_eq!(
        custody.writer_outcome(),
        Some(XAuthorityInputDeliveryOutcome::Flushed)
    );
    // Settled only once nothing is pending and no attempt is out.
    assert!(custody.writer_settled());

    // A suppressed custody has nothing to answer through.
    let suppressed = PrivateDeliveryCustody::new(2, None);
    assert!(suppressed.finalizer(&recovery, None, client).is_none());
    // An admitted custody without its delivery has nothing either: the
    // finalizer would name an admission it cannot find.
    let admitted = PrivateDeliveryCustody::new(3, Some(Arc::default()));
    assert!(admitted.finalizer(&recovery, None, client).is_none());
}
