use super::*;

#[test]
fn lookup_distinguishes_eviction_unissued_and_uncertain_custody() {
    let mut ledger = Ledger::default();
    for ticket in [0, 1, u64::MAX] {
        assert_eq!(
            ledger.lookup(Ticket(ticket)),
            Err(CustodyLookupError::Unissued)
        );
    }
    ledger.issue(LEDGER_CAPACITY);
    ledger.set(1, Custody::Unknown);
    ledger.set(2, Custody::Submitted);
    assert_eq!(ledger.lookup(Ticket(1)), Ok(Custody::Unknown));
    assert_eq!(ledger.lookup(Ticket(2)), Ok(Custody::Submitted));
    ledger.issue(1);
    assert_eq!(
        ledger.lookup(Ticket(1)),
        Err(CustodyLookupError::Unavailable)
    );
    assert_eq!(ledger.lookup(Ticket(2)), Ok(Custody::Submitted));
    assert_eq!(ledger.lookup(Ticket(257)), Ok(Custody::Queued));
    assert_eq!(
        ledger.lookup(Ticket(258)),
        Err(CustodyLookupError::Unissued)
    );
    assert_eq!(ledger.lookup(Ticket(0)), Err(CustodyLookupError::Unissued));
    ledger.set(1, Custody::Submitted);
    assert_eq!(
        ledger.lookup(Ticket(1)),
        Err(CustodyLookupError::Unavailable)
    );
}

#[test]
fn tickets_rise_and_an_evicted_ticket_is_unavailable_never_aliased() {
    let mut ledger = Ledger::default();
    let first = ledger.issue(2);
    assert_eq!(first.tickets().collect::<Vec<_>>(), [Ticket(1), Ticket(2)]);
    ledger.set(2, Custody::Submitted);
    assert_eq!(ledger.get(Ticket(2)), Some(Custody::Submitted));
    for _ in 0..LEDGER_CAPACITY {
        ledger.issue(1);
    }
    assert_eq!(ledger.get(Ticket(1)), None);
    assert_eq!(ledger.get(Ticket(2)), None);
    ledger.set(2, Custody::Unknown);
    let settled = ledger.issue(1).first;
    ledger.set(settled.0, Custody::Submitted);
    ledger.set(settled.0, Custody::Refused(22));
    ledger.set(settled.0, Custody::Unknown);
    assert_eq!(ledger.get(settled), Some(Custody::Submitted));
    ledger.issue(1);
    let newest = Ticket(ledger.peek() - 1);
    assert_eq!(ledger.get(newest), Some(Custody::Queued));
    assert_eq!(ledger.get(Ticket(ledger.peek())), None);
}
