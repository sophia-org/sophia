#![cfg(test)]
use super::*;

pub(in super::super) fn exhaust_sequence(journal: &mut Journal) {
    journal.next = u64::MAX;
}
pub(in super::super) fn exhaust_tail(journal: &mut Journal) {
    journal.tail = u64::MAX;
}
pub(in super::super) fn position(journal: &Journal) -> (u64, u64, usize, usize) {
    (
        journal.next,
        journal.tail,
        journal.records.len(),
        journal.bytes,
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
