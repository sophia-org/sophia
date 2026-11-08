// Emitter-shaped rows through the capture filter: every grid cell's row is
// admitted unchanged, the field order is fixed, and a foreign word or a
// non-numeric counter is dropped from the same row shape.
use super::*;
use crate::diagnostics::reduced_record;

#[test]
fn a_row_has_the_fixed_field_order() {
    let counts = Counts([1, 2, 3, 4, 5, 6]);
    assert_eq!(
        row(6880, 23_837_342_899, Entry::Turn, Boundary::PreTurn, counts),
        "sophia_liveness_counts schema=1 owner_tid=6880 observed_monotonic_usec=23837342899 \
         entry=turn boundary=pre_turn entries=1 export_polls=2 file_checks=3 custody_polls=4 \
         image_alive_polls=5 accept_attempts=6"
    );
}

#[test]
fn every_cell_row_passes_the_capture_filter_unchanged() {
    let snapshot = liveness_counts::snapshot();
    let mut rows = 0;
    for (entry, boundary, _) in snapshot.rows() {
        let counts = Counts([0, u64::MAX, 7, 8, 9, 10]);
        let row = row(1, 2, entry, boundary, counts);
        assert_eq!(reduced_record(&row), Some(row.clone()), "{row}");
        rows += 1;
    }
    assert_eq!(rows, Entry::ALL.len() * Boundary::ALL.len());
}

#[test]
fn foreign_words_and_non_numeric_counters_are_dropped() {
    let admitted = row(1, 2, Entry::Flush, Boundary::ImageAccept, Counts::default());
    for (from, to) in [
        ("entry=flush", "entry=publication"),
        ("boundary=image_accept", "boundary=accept"),
        ("custody_polls=0", "custody_polls=-1"),
        ("entries=0", "entries=18446744073709551616"),
        ("owner_tid=1", "owner_tid=secret"),
    ] {
        let tampered = admitted.replace(from, to);
        assert_ne!(tampered, admitted);
        assert_eq!(
            reduced_record(&tampered),
            Some(admitted.replace(&format!(" {from}"), "")),
            "{to}"
        );
    }
}
