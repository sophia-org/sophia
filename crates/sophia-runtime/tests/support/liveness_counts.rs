// Device-free proof of the counting grid: the default context, exclusive
// innermost labels, complete restoration on return and unwind, saturation,
// thread isolation, entry conservation and grid completeness. Deltas are
// taken against a snapshot, so a shared test thread cannot skew them.
use super::*;

fn delta(before: &Snapshot, after: &Snapshot, entry: Entry, boundary: Boundary, field: Field) -> u64 {
    after.get(entry, boundary).get(field) - before.get(entry, boundary).get(field)
}

#[test]
fn outside_any_entry_a_count_lands_in_outside_other() {
    let before = snapshot();
    count(Field::CustodyPolls);
    let after = snapshot();
    assert_eq!(delta(&before, &after, Entry::Outside, Boundary::Other, Field::CustodyPolls), 1);
    assert_eq!(after.total(Field::CustodyPolls) - before.total(Field::CustodyPolls), 1);
    assert_eq!(after.total(Field::Entries), before.total(Field::Entries));
}

#[test]
fn an_entry_counts_itself_once_and_labels_nested_work_by_the_innermost_boundary() {
    let before = snapshot();
    {
        let _turn = enter(Entry::Turn);
        count(Field::ExportPolls);
        {
            let _pre = within(Boundary::PreTurn);
            count(Field::CustodyPolls);
            let _check = within(Boundary::FileCheck);
            count(Field::FileChecks);
        }
        count(Field::AcceptAttempts);
    }
    count(Field::ImageAlivePolls);
    let after = snapshot();
    let d = |entry, boundary, field| delta(&before, &after, entry, boundary, field);
    assert_eq!(d(Entry::Turn, Boundary::Other, Field::Entries), 1);
    assert_eq!(d(Entry::Turn, Boundary::Other, Field::ExportPolls), 1);
    assert_eq!(d(Entry::Turn, Boundary::PreTurn, Field::CustodyPolls), 1);
    assert_eq!(d(Entry::Turn, Boundary::FileCheck, Field::FileChecks), 1);
    assert_eq!(d(Entry::Turn, Boundary::PreTurn, Field::FileChecks), 0, "outer label took a nested count");
    assert_eq!(d(Entry::Turn, Boundary::Other, Field::AcceptAttempts), 1, "boundary not restored");
    assert_eq!(d(Entry::Outside, Boundary::Other, Field::ImageAlivePolls), 1, "entry not restored");
    for field in Field::ALL {
        assert_eq!(after.total(field) - before.total(field), 1, "{} not partitioned", field.name());
    }
}

#[test]
fn a_nested_entry_restores_the_complete_prior_context() {
    let before = snapshot();
    {
        let _turn = enter(Entry::Turn);
        let _post = within(Boundary::PostTurn);
        {
            let _flush = enter(Entry::Flush);
            count(Field::CustodyPolls);
        }
        count(Field::CustodyPolls);
    }
    let after = snapshot();
    assert_eq!(delta(&before, &after, Entry::Flush, Boundary::Other, Field::CustodyPolls), 1);
    assert_eq!(delta(&before, &after, Entry::Flush, Boundary::Other, Field::Entries), 1);
    assert_eq!(delta(&before, &after, Entry::Turn, Boundary::PostTurn, Field::CustodyPolls), 1);
    assert_eq!(delta(&before, &after, Entry::Turn, Boundary::Other, Field::CustodyPolls), 0);
}

#[test]
fn unwinding_through_both_guards_restores_the_prior_context() {
    let before = snapshot();
    let unwound = std::panic::catch_unwind(|| {
        let _turn = enter(Entry::Turn);
        let _check = within(Boundary::FileCheck);
        panic!("unwind through the entry and boundary guards");
    });
    assert!(unwound.is_err());
    count(Field::CustodyPolls);
    let after = snapshot();
    assert_eq!(delta(&before, &after, Entry::Outside, Boundary::Other, Field::CustodyPolls), 1);
    assert_eq!(delta(&before, &after, Entry::Turn, Boundary::Other, Field::Entries), 1);
}

#[test]
fn counters_saturate_instead_of_wrapping() {
    // Its own thread, so the saturated cell does not outlive the test.
    let saturated = std::thread::spawn(|| {
        let _flush = enter(Entry::Flush);
        let _check = within(Boundary::ImageCheck);
        add(Field::ImageAlivePolls, u64::MAX - 1);
        count(Field::ImageAlivePolls);
        count(Field::ImageAlivePolls);
        snapshot()
    })
    .join()
    .unwrap();
    assert_eq!(saturated.get(Entry::Flush, Boundary::ImageCheck).get(Field::ImageAlivePolls), u64::MAX);
    assert_eq!(saturated.total(Field::ImageAlivePolls), u64::MAX);
}

#[test]
fn counts_stay_on_the_thread_that_made_them() {
    let before = snapshot();
    let other = std::thread::spawn(|| {
        let _turn = enter(Entry::Turn);
        count(Field::CustodyPolls);
        snapshot()
    })
    .join()
    .unwrap();
    assert_eq!(other.get(Entry::Turn, Boundary::Other).get(Field::CustodyPolls), 1);
    assert_eq!(other.total(Field::Entries), 1);
    assert_eq!(snapshot(), before);
}

#[test]
fn entries_are_conserved_and_the_grid_has_every_cell_once() {
    let before = snapshot();
    for _ in 0..3 {
        let _turn = enter(Entry::Turn);
        let _server = within(Boundary::ServerTurn);
    }
    for _ in 0..2 {
        let _flush = enter(Entry::Flush);
    }
    let after = snapshot();
    assert_eq!(after.total(Field::Entries) - before.total(Field::Entries), 5);
    assert_eq!(delta(&before, &after, Entry::Turn, Boundary::Other, Field::Entries), 3);
    assert_eq!(delta(&before, &after, Entry::Flush, Boundary::Other, Field::Entries), 2);
    for (entry, boundary, counts) in after.rows() {
        if boundary != Boundary::Other || entry == Entry::Outside {
            assert_eq!(counts.get(Field::Entries), before.get(entry, boundary).get(Field::Entries));
        }
    }
    let cells: Vec<(&str, &str)> = after.rows().map(|(entry, boundary, _)| (entry.name(), boundary.name())).collect();
    assert_eq!(cells.len(), Entry::ALL.len() * Boundary::ALL.len());
    let unique: std::collections::BTreeSet<&(&str, &str)> = cells.iter().collect();
    assert_eq!(unique.len(), cells.len());
}
