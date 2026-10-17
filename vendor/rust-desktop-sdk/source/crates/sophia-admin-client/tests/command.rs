use sophia_admin_client::{
    command::{Custody, Error, Pending},
    protocol::*,
};

fn info() -> Info {
    Info {
        session_low: 1,
        session_high: 0,
        connection: 7,
        command_ms: 10,
        frame_ms: 2,
        idle_ms: 100,
    }
}
fn pending(owner: Owner, name: &str) -> Pending {
    let catalog = decode_catalog(b"v1 catalog 12 4\nv1 command policy 8:next tab\nv1 command session 6:logout\nv1 command session 14:reload-profile\nv1 command session 10:restart-wm\n").unwrap();
    Pending::new(info(), &catalog, Command { owner, name }, 9, 0).unwrap()
}
fn accepted(owner: Owner, name: &str) -> Pending {
    let mut p = pending(owner, name);
    p.issued(0).unwrap();
    p.write_reply(Ok(p.bytes().len() as u32), 1).unwrap();
    p
}
#[test]
fn immutable_write_and_pending_cadence() {
    let mut p = pending(Owner::Policy, "next tab");
    assert_eq!(p.bytes(), b"v1 invoke 7 9 12 policy 8:next tab\n");
    assert_eq!(p.start_status(0), Err(Error::Busy));
    p.issued(0).unwrap();
    assert_eq!(p.issued(0), Err(Error::Busy));
    p.write_reply(Ok(p.bytes().len() as u32), 1).unwrap();
    assert_eq!(p.custody(), Custody::Accepted);
    assert_eq!(p.next_wake(), Some(1));
    assert_eq!(p.start_status(1), Ok(3));
    assert_eq!(p.start_status(1), Err(Error::Busy));
    p.status(b"v1 pending 7 9 12\n", 1).unwrap();
    assert_eq!(p.next_wake(), Some(2));
    assert_eq!(p.start_status(1), Err(Error::Busy));
    assert_eq!(p.start_status(2), Ok(4));
    p.status(b"v1 outcome 7 9 12 committed 2:ok\n", 2).unwrap();
    assert_eq!(p.custody(), Custody::Settled);
    assert_eq!(p.outcome(), Some((Outcome::Committed, "ok")));
    assert_eq!(p.next_wake(), None);
    assert!(!p.invalidate_catalog());
    p.close();
    assert_eq!(p.custody(), Custody::Settled);
    assert_eq!(p.outcome(), Some((Outcome::Committed, "ok")));
}
#[test]
fn refusal_short_write_loss_and_deadlines_preserve_distinctions() {
    let mut p = pending(Owner::Policy, "next tab");
    p.close();
    assert_eq!(p.custody(), Custody::DroppedUnsent);
    let mut p = pending(Owner::Policy, "next tab");
    p.issued(0).unwrap();
    p.close();
    assert_eq!(p.custody(), Custody::Unknown);
    let mut p = accepted(Owner::Policy, "next tab");
    p.close();
    assert_eq!(p.custody(), Custody::Unknown);
    let mut p = pending(Owner::Policy, "next tab");
    p.issued(0).unwrap();
    assert_eq!(p.write_reply(Err(11), 1), Err(Error::Refused(11)));
    assert_eq!(p.custody(), Custody::Refused);
    p.close();
    assert_eq!(p.custody(), Custody::Refused);
    let mut p = pending(Owner::Policy, "next tab");
    p.issued(0).unwrap();
    assert_eq!(p.write_reply(Ok(1), 1), Err(Error::Protocol));
    assert_eq!(p.custody(), Custody::Unknown);
    let mut p = pending(Owner::Policy, "next tab");
    assert_eq!(p.advance(12), Err(Error::Deadline));
    assert_eq!(p.custody(), Custody::DroppedUnsent);
    let mut p = pending(Owner::Policy, "next tab");
    p.issued(0).unwrap();
    assert_eq!(p.advance(2), Err(Error::Deadline));
    assert_eq!(p.custody(), Custody::Unknown);
    let mut p = accepted(Owner::Policy, "next tab");
    p.start_status(1).unwrap();
    assert_eq!(p.status(b"v1 pending 7 9 12\n", 3), Err(Error::Deadline));
    let mut p = accepted(Owner::Policy, "next tab");
    assert_eq!(p.advance(0), Err(Error::ClockRegression));
    assert_eq!(p.error(), None);
    for now in 1..12 {
        p.start_status(now).unwrap();
        p.status(b"v1 pending 7 9 12\n", now).unwrap();
    }
    assert_eq!(p.start_status(12), Err(Error::Deadline));
}
#[test]
fn owner_outcomes_identity_and_catalog_refresh() {
    for (owner, name) in [
        (Owner::Policy, "next tab"),
        (Owner::Session, "logout"),
        (Owner::Session, "reload-profile"),
    ] {
        for outcome in [
            "committed",
            "completed",
            "unchanged",
            "rejected",
            "stale",
            "denied",
            "unavailable",
            "overloaded",
            "timed-out",
            "indeterminate",
        ] {
            let mut p = accepted(owner, name);
            p.start_status(1).unwrap();
            let valid = match outcome {
                "committed" => owner == Owner::Policy,
                "completed" => owner == Owner::Session,
                "unchanged" => name == "reload-profile",
                _ => true,
            };
            let wire = format!("v1 outcome 7 9 12 {outcome} 0:\n");
            assert_eq!(p.status(wire.as_bytes(), 1).is_ok(), valid);
            assert_eq!(
                p.custody(),
                if valid {
                    Custody::Settled
                } else {
                    Custody::Unknown
                }
            );
            if valid {
                assert_eq!(
                    p.invalidate_catalog(),
                    owner == Owner::Session
                        || matches!(outcome, "stale" | "denied" | "indeterminate")
                );
            }
        }
    }
    for wire in [
        "v1 ready 7\n",
        "v1 pending 8 9 12\n",
        "v1 pending 7 10 12\n",
        "v1 pending 7 9 13\n",
        "bad\n",
    ] {
        let mut p = accepted(Owner::Policy, "next tab");
        p.start_status(1).unwrap();
        assert_eq!(p.status(wire.as_bytes(), 1), Err(Error::Protocol));
        assert_eq!(p.custody(), Custody::Unknown);
    }
    let catalog = decode_catalog(b"v1 catalog 12 0\n").unwrap();
    assert!(
        Pending::new(
            info(),
            &catalog,
            Command {
                owner: Owner::Policy,
                name: "next tab"
            },
            9,
            0
        )
        .is_err()
    );
}
