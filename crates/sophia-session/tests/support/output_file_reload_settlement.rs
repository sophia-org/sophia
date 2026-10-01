//! A reloaded profile's output topology is Session's own transaction. It is
//! admitted with the output owner's epoch and settled locally; a connected
//! output file client receives no outcome for it and keeps its connection.
//! Physical preparation and its failure are supplied, as in the parent rig.
use super::*;

#[test]
fn reload_settlement_is_local_and_keeps_a_connected_observer_served() {
    let mut rig = rig("reload-connected");
    let mut observer = rig.peer();
    let (epoch, original, _) = observer.negotiate();
    assert_eq!((epoch, &original), (EPOCH, &rig.snapshot));
    rig.fixture.wm.poll_output_authority().unwrap();

    let reload = rig.apply();
    assert!(rig.public().admit_reloaded_output_topology(reload).unwrap());
    let transaction = rig
        .public()
        .take_output_topology_effect()
        .unwrap()
        .transaction;
    let public = rig.public();
    let authority = public.output_authority.as_mut().unwrap();
    authority
        .fail(OutputTopologyTransactionFailure::Preparation)
        .unwrap();
    let settlement = authority.settle_terminal().unwrap();
    assert_eq!(settlement.transaction, transaction);
    public.finish_output_settlement(settlement).unwrap();
    assert!(!public.output_candidate_active());
    assert_eq!(public.published_output_snapshot(), Some(original));

    // Nothing reaches the observer for Session's transaction, and the file
    // service survives Session's next output turn.
    observer.quiet();
    rig.fixture.wm.poll_output_authority().unwrap();
    assert!(rig.public().output_service.is_some());
    assert!(rig.public().output_pending_connection_epoch.is_none());
    assert!(rig.public().output_cancel_requested.is_none());
    assert_eq!(rig.authority().connection_epoch(), EPOCH);
    assert!(rig.authority().active_transaction().is_none());

    // The observer's own proposal is still admitted on the same connection.
    observer.propose(11, rig.apply());
    rig.admitted(11);
    observer.close();
}
