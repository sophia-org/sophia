use super::super::outbound::{Admitted, OutboundRecord};
use super::*;
use sophia_protocol::{ContentGrant, ContentOutputFacts, ShellContentRecord, TransactionId};

#[path = "shell_typed_outbox_transport.rs"]
mod transport;

fn record(transaction: u64, control: bool, charge: usize) -> Admitted {
    Admitted {
        record: OutboundRecord::Content(
            TransactionId::from_raw(transaction),
            ShellContentRecord::OutputFacts(ContentOutputFacts {
                grant: ContentGrant::default(),
                facts_generation: 1,
                outputs: Vec::new(),
            }),
        ),
        control,
        charge,
    }
}

fn transaction(queued: &Queued) -> u64 {
    match &queued.record {
        OutboundRecord::Content(transaction, _) => transaction.raw(),
        _ => unreachable!("only content records are queued here"),
    }
}

#[test]
fn records_leave_whole_in_admission_order_with_their_exact_charges() {
    let mut outbox = ShellOutbox::default();
    outbox.push(record(1, true, 120));
    outbox.push(record(2, false, 680));
    outbox.push(record(3, true, 56));
    assert_eq!(outbox.records(), 3);
    assert_eq!(outbox.controls(), 2);
    assert_eq!(outbox.bulk_bytes(), 680);
    assert_eq!(outbox.charged(), 856);
    let first = outbox.front().unwrap().sequence;
    assert_eq!(transaction(outbox.front().unwrap()), 1);
    let released = outbox.pop_front();
    assert_eq!(transaction(&released), 1);
    assert_eq!(outbox.controls(), 1);
    assert_eq!(outbox.charged(), 736);
    let released = outbox.pop_front();
    assert_eq!(transaction(&released), 2);
    assert!(released.sequence > first);
    assert_eq!(outbox.bulk_bytes(), 0);
    assert_eq!(transaction(&outbox.pop_front()), 3);
    assert_eq!(outbox.records(), 0);
    assert_eq!(outbox.charged(), 0);
    assert_eq!(outbox.controls(), 0);
}

#[test]
fn a_wire_lane_stamp_takes_the_next_position_in_the_single_order() {
    let mut outbox = ShellOutbox::default();
    outbox.push(record(1, true, 8));
    let lane = outbox.next_sequence();
    outbox.push(record(2, true, 8));
    let first = outbox.pop_front().sequence;
    let second = outbox.pop_front().sequence;
    assert!(first < lane && lane < second);
}

#[test]
fn disconnect_clears_the_exact_retained_inventory() {
    let mut outbox = ShellOutbox::default();
    outbox.push(record(1, true, 72));
    outbox.push(record(2, false, 40));
    outbox.clear();
    assert!(outbox.front().is_none());
    assert_eq!(outbox.records(), 0);
    assert_eq!(outbox.charged(), 0);
    assert_eq!(outbox.bulk_bytes(), 0);
    assert_eq!(outbox.controls(), 0);
    outbox.push(record(3, true, 16));
    assert_eq!(transaction(outbox.front().unwrap()), 3);
}
