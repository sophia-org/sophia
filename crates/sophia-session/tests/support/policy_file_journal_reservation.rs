#![cfg(test)]
use super::*;

pub(in super::super) fn exhaust_sequence(journal: &mut Journal) {
    journal.0 = Custody::starting_at(journal.0.epoch(), u64::MAX, journal.0.size());
}
pub(in super::super) fn exhaust_tail(journal: &mut Journal) {
    let next = journal.0.next_sequence();
    journal.0 = Custody::starting_at(journal.0.epoch(), next, u64::MAX);
}
pub(in super::super) fn position(journal: &Journal) -> (u64, u64, usize, usize) {
    let position = journal.0.position();
    (
        position.next_sequence,
        position.tail,
        position.records,
        position.bytes,
    )
}

#[test]
fn dropping_a_prepared_record_spends_no_bytes_or_sequence() {
    let mut journal = Journal::new(9);
    let before = position(&journal);
    let record = journal
        .prepare_encoded(WmFileKind::Negotiated, |header| {
            encode_wm_file_negotiated(header, 0).map_err(|_| Errno::EINVAL)
        })
        .unwrap();
    drop(record);
    assert_eq!(position(&journal), before);
    let record = journal
        .prepare_encoded(WmFileKind::Negotiated, |header| {
            encode_wm_file_negotiated(header, 0).map_err(|_| Errno::EINVAL)
        })
        .unwrap();
    assert_eq!(record.commit(), 1);
    assert_eq!(position(&journal), (2, 40, 1, 40));
}
