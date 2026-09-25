//! Capture the actual reservation owner's output before evidence reduction.
use super::*;
use sophia_session::diagnostics::reduced_record;
use sophia_session::{SessionOutput, install_session_output};
use std::cell::RefCell;

thread_local! {
    static RECORDS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn capture(line: &str) {
    RECORDS.with(|records| records.borrow_mut().push(line.to_owned()));
}

fn field<'a>(line: &'a str, key: &str) -> &'a str {
    line.split_whitespace()
        .find_map(|entry| {
            let (name, value) = entry.split_once('=')?;
            (name == key).then_some(value)
        })
        .unwrap()
}

#[test]
fn actual_reservations_emit_reduced_and_count_refused_inventory_once() {
    install_session_output(SessionOutput::new(|_| {}, capture)).unwrap();
    let mut h = Harness::new();
    let mut pins = Vec::new();
    for epoch in 1..=16 {
        let key = h.owner.reserve_attempt(0).unwrap();
        assert_eq!(key.grant.connection_epoch, epoch);
        let mut client = h.connect(key);
        pins.push(upload(&mut h, key, &mut client, 1));
        h.owner.close(key).unwrap();
    }
    let before = h.owner.accounting();
    assert_eq!(
        h.owner.reserve_attempt(0),
        Err(ComponentConnectionError::Transport(
            ShellTransportError::ContentStore(ContentStoreError::Budget)
        ))
    );
    assert_eq!(h.owner.accounting(), before);
    let records = RECORDS.with(|records| std::mem::take(&mut *records.borrow_mut()));
    assert_eq!(records.len(), 16); // Cold admission is silent; fifteen reduced, one refusal.
    for (index, record) in records.iter().enumerate() {
        assert_eq!(reduced_record(record), Some(record.clone()));
        assert_eq!(field(record, "cause"), "content_budget");
        assert_eq!(
            field(record, "budget_constraint"),
            if index == 15 { "epochs" } else { "bytes" }
        );
        assert_eq!(field(record, "epoch_capacity"), "16");
        assert_eq!(field(record, "active_capacity"), "3");
        assert_eq!(field(record, "slot"), "0");
        assert_eq!(field(record, "role"), "bar");
        assert_eq!(
            field(record, "connection_epoch").parse::<usize>().unwrap(),
            index + 2
        );
        assert_eq!(
            field(record, "own_retired_epochs")
                .parse::<usize>()
                .unwrap(),
            index + 1
        );
        assert_eq!(
            field(record, "own_retired_bytes").parse::<usize>().unwrap(),
            4 * (index + 1)
        );
        assert_eq!(field(record, "source_capacity_bytes"), "67108864");
        assert_eq!(field(record, "required_bytes"), "16777216");
        assert_eq!(field(record, "required_backing_bytes"), "12582912");
        assert_eq!(
            field(record, "status"),
            if index == 15 {
                "admission_refused"
            } else {
                "admitted_reduced"
            }
        );
    }
    // No idle collection record and no invented release: only dropping the
    // exact source consumer reopens an epoch slot. Attempt 17 remains burned.
    h.owner.collect();
    assert!(RECORDS.with(|records| records.borrow().is_empty()));
    drop(pins.pop());
    h.owner.collect();
    let fresh = h.owner.reserve_attempt(0).unwrap();
    assert_eq!(fresh.grant.connection_epoch, 18);
    assert_eq!(
        h.owner.accounting().active_epochs + h.owner.accounting().retired_epochs,
        16
    );
    let records = RECORDS.with(|records| std::mem::take(&mut *records.borrow_mut()));
    assert_eq!(records.len(), 1);
    assert_eq!(field(&records[0], "status"), "admitted_reduced");
    assert_eq!(field(&records[0], "connection_epoch"), "18");
}
