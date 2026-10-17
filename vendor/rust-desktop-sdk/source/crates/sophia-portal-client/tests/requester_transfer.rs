use sophia_portal_client::requester::{
    Error,
    transfer::{State, Transfer},
};
use sophia_portal_protocol::requester::{Event, Grant, Outcome, TransferKind, TransferRequest};
fn request() -> TransferRequest<'static> {
    TransferRequest {
        transfer: 11,
        source: 2,
        target: 3,
        kind: TransferKind::Clipboard,
        mime: Some("text/plain"),
        byte_size: 123,
        generation: 5,
        deadline_msec: 900,
    }
}
fn grant() -> Grant {
    Grant {
        transfer: 11,
        source: 2,
        target: 3,
        kind: TransferKind::Clipboard,
        source_generation: 5,
        broker_generation: 17,
        deadline_msec: 900,
    }
}
#[test]
fn every_grant_field_must_match_before_exposure() {
    for field in 0..7 {
        let mut t = Transfer::new(request(), 17).unwrap();
        let mut g = grant();
        match field {
            0 => g.transfer += 1,
            1 => g.source += 1,
            2 => g.target += 1,
            3 => g.kind = TransferKind::ScreenCapture,
            4 => g.source_generation += 1,
            5 => g.broker_generation += 1,
            _ => g.deadline_msec += 1,
        }
        assert_eq!(t.decision(Event::Allowed(g)), Err(Error::Protocol));
        assert_eq!(t.state(), State::Pending);
        assert_eq!(t.grant(), None);
        t.decision(Event::Allowed(grant())).unwrap();
        assert_eq!(t.grant(), Some(grant()));
        assert_eq!(
            t.decision(Event::Denied { transfer: 11 }),
            Err(Error::Protocol)
        );
        assert_eq!(t.state(), State::Allowed);
    }
}
#[test]
fn domain_denial_and_disconnect_do_not_invent_outcomes() {
    let mut r = request();
    r.transfer = 0;
    r.generation = 0;
    r.deadline_msec = 0;
    let mut t = Transfer::new(r, 17).unwrap();
    t.decision(Event::Denied { transfer: 0 }).unwrap();
    assert_eq!(t.state(), State::Denied);
    t.close();
    assert!(t.disconnected());
    assert_eq!(t.state(), State::Denied);
    assert!(t.grant().is_none());
    for end in [false, true] {
        let mut t = Transfer::new(request(), 17).unwrap();
        t.decision(Event::Allowed(grant())).unwrap();
        if end {
            t.end_issued().unwrap();
        }
        t.close();
        assert!(t.grant().is_none());
        assert_eq!(
            t.state(),
            if end {
                State::EndIssued
            } else {
                State::Allowed
            }
        );
        assert_eq!(t.outcome(11, Outcome::Completed), Err(Error::Closed));
    }
}
#[test]
fn completion_requires_end_and_terminal_outcomes_are_single_use() {
    for outcome in [Outcome::Completed, Outcome::Revoked, Outcome::Expired] {
        let mut t = Transfer::new(request(), 17).unwrap();
        t.decision(Event::Allowed(grant())).unwrap();
        assert_eq!(t.outcome(11, Outcome::Completed), Err(Error::Protocol));
        assert_eq!(t.outcome(12, Outcome::Revoked), Err(Error::Protocol));
        if outcome == Outcome::Completed {
            t.end_issued().unwrap();
            assert_eq!(t.end_issued(), Err(Error::Busy));
        }
        t.outcome(11, outcome).unwrap();
        assert!(t.grant().is_none());
        assert_eq!(
            t.state(),
            match outcome {
                Outcome::Completed => State::Completed,
                Outcome::Revoked => State::Revoked,
                Outcome::Expired => State::Expired,
            }
        );
        assert_eq!(t.outcome(11, outcome), Err(Error::Protocol));
        assert_eq!(t.end_issued(), Err(Error::Busy));
    }
}
