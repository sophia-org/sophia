use super::{LiveSeatDisableOutcome, try_disable};

#[test]
fn pending_lease_never_calls_disable_and_last_close_allows_one_ack() {
    let mut calls = 0;
    for leases in [2, 1, 1] {
        assert_eq!(
            try_disable(leases, || {
                calls += 1;
                Ok(())
            })
            .unwrap(),
            LiveSeatDisableOutcome::Pending { leases }
        );
    }
    assert_eq!(calls, 0);
    assert_eq!(
        try_disable(0, || {
            calls += 1;
            Ok(())
        })
        .unwrap(),
        LiveSeatDisableOutcome::Acknowledged
    );
    assert_eq!(calls, 1);
}

#[test]
fn disable_backend_failure_does_not_become_acknowledgement() {
    assert_eq!(
        try_disable(0, || Err("backend refused".into())),
        Err("backend refused".into())
    );
}
