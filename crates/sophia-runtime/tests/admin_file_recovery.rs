#[path = "support/admin_files.rs"]
mod support;
use sophia_runtime::AdminClient;
use support::*;

#[test]
fn replacing_a_service_closes_old_fids_and_cancels_queued_tickets() {
    let f = Fixture::new();
    let old_path = f.service.socket_path().to_owned();
    let mut old = f.peer();
    assert_eq!(old.invoke(1, 7)[4], 119);
    let ticket = f.ticket();
    opened(&mut old.stream, 3, b"status");
    drop(f);
    let f = Fixture::new();
    assert!(!old_path.exists());
    assert!(receive(&mut old.stream).is_err());
    assert!(ticket.cancelled());
    assert!(!ticket.claim());
    let mut fresh = f.peer();
    assert_ne!(fresh.info.session_id, old.info.session_id);
    assert_eq!(fresh.info.connection, old.info.connection);
    assert_eq!(
        fresh.status(),
        AdminStatus::Ready {
            connection: fresh.info.connection
        }
    );
    assert!(f.service.try_request().is_none());
}

#[test]
fn old_dispatched_completion_cannot_settle_a_new_instances_matching_request() {
    let f = Fixture::new();
    let mut old = f.peer();
    assert_eq!(old.invoke(1, 7)[4], 119);
    let prior = f.ticket();
    claim(&prior);
    drop(f);
    let f = Fixture::new();
    assert!(prior.dispatched());
    assert!(receive(&mut old.stream).is_err());
    let mut fresh = f.peer();
    assert_eq!(fresh.invoke(1, 7)[4], 119);
    let successor = f.ticket();
    assert_eq!(
        (prior.connection, prior.request),
        (successor.connection, successor.request)
    );
    prior.finish(ControlOutcome::Committed);
    assert_eq!(
        fresh.status(),
        AdminStatus::Pending {
            connection: successor.connection,
            request: 1,
            generation: 7
        }
    );
    claim(&successor);
    successor.finish(ControlOutcome::Unchanged);
    assert_eq!(fresh.outcome(ControlOutcome::Unchanged).request, 1);
}

#[test]
fn file_client_discovers_validates_settlement_and_reuses_connection() {
    let f = Fixture::new();
    let path = f.service.socket_path().to_owned();
    std::thread::scope(|scope| {
        let client = scope.spawn(|| {
            let mut client = AdminClient::connect(&path).unwrap();
            assert_eq!(client.commands().unwrap(), catalog(7));
            for expected in [
                ControlOutcome::Committed,
                ControlOutcome::Rejected,
                ControlOutcome::Indeterminate,
            ] {
                assert_eq!(
                    client.invoke(action()).unwrap(),
                    (7, expected, String::new())
                );
                assert!(!client.invocation_pending());
            }
            assert_eq!(client.request_id(), 3);
        });
        for outcome in [
            ControlOutcome::Committed,
            ControlOutcome::Rejected,
            ControlOutcome::Indeterminate,
        ] {
            let ticket = f.ticket();
            claim(&ticket);
            ticket.finish(outcome);
        }
        client.join().unwrap();
    });
}

#[test]
fn lost_reply_remains_uncertain_and_the_file_client_never_reconnects() {
    let f = Fixture::new();
    let path = f.service.socket_path().to_owned();
    let mut client = AdminClient::connect(&path).unwrap();
    assert_eq!(client.commands().unwrap(), catalog(7));
    std::thread::scope(|scope| {
        let invoked = scope.spawn(move || {
            assert!(client.invoke(action()).is_err());
            assert!(client.invocation_pending());
            assert!(client.invoke(action()).is_err());
            assert!(client.commands().is_err());
            assert!(client.invocation_pending());
        });
        let ticket = f.ticket();
        claim(&ticket);
        drop(f);
        let f = Fixture::new();
        invoked.join().unwrap();
        ticket.finish(ControlOutcome::Committed);
        assert!(f.service.try_request().is_none());
    });
}

#[test]
fn invalid_success_from_owner_is_uncertain_and_closes_the_client() {
    let f = Fixture::new();
    let path = f.service.socket_path().to_owned();
    std::thread::scope(|scope| {
        let client = scope.spawn(|| {
            let mut client = AdminClient::connect(&path).unwrap();
            assert!(client.invoke(action()).is_err());
            assert!(client.invocation_pending());
            assert!(client.commands().is_err());
        });
        let ticket = f.ticket();
        claim(&ticket);
        // Policy actions require Committed; Completed belongs to Session.
        ticket.finish(ControlOutcome::Completed);
        client.join().unwrap();
    });
}
