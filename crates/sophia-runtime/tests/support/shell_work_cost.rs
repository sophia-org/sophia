#![cfg(test)]

use super::*;
use std::cell::Cell;

#[test]
fn disabled_state_reads_no_clock_or_counters() {
    let mut s = State::new(false);
    assert_eq!(s.read(|| panic!("disabled clock")), None);
    s.call(Call::Custody);
    s.decode(0, true);
    s.admission(0, false);
    s.class(0, false);
    s.permit();
    assert!(!s.job(4096, true));
    assert_eq!((s.reads, s.jobs, s.decoded()), (0, 0, 0));
    assert!(s.calls.iter().all(|c| c.custody == 0));
    assert_eq!(s.classes[0].decoded, 0);
    assert_eq!(s.server_frame_permits, 0);
    let saved = COST.with(|cost| cost.replace(State::new(false)));
    boundary(Boundary::FileCheck, || {
        count(Call::FileCheck);
        turn(false, || {
            assert!(decode(0, || result(Stage::Copy, 8, || Ok::<(), ()>(()))).is_ok());
            unit(Stage::Collect, || {});
        });
    });
    let disabled = COST.with(|cost| cost.replace(saved));
    assert_eq!(disabled.reads, 0);
    assert_eq!(disabled.turns, [0, 0]);
    assert!(disabled.stages.iter().all(|s| s.calls == 0));
}

#[test]
fn regression_and_overflow_are_visible_and_invalidate_the_snapshot() {
    let mut s = State::new(true);
    assert_eq!(s.read(|| 100), Some(100));
    assert_eq!(s.read(|| 90), Some(90));
    s.finish(Stage::Read, Outcome::Error, 8, 100, 90);
    assert_eq!(s.stages[Stage::Read as usize].cpu, 0);
    assert_eq!(s.regressions, 1);
    assert!(!s.valid());
    let mut s = State::new(true);
    s.reads = u64::MAX;
    s.read(|| 1);
    assert_eq!(s.reads, u64::MAX);
    assert_eq!(s.saturations, 1);
    assert!(!s.valid());
    s.decoded_total = u64::MAX;
    s.decode(1, true);
    assert_eq!(s.decoded(), u64::MAX);
    assert_eq!(s.saturations, 2);
    assert!(record::rows(&s, 1, 1, 1, false, false)[0].contains("valid=0"));
}

#[test]
fn nested_measurements_are_inclusive_and_outcomes_are_not_frames() {
    let mut s = State::new(true);
    s.read(|| 10);
    s.read(|| 20);
    s.read(|| 30);
    s.read(|| 50);
    s.finish(Stage::Read, Outcome::Ok, 8, 20, 30);
    s.finish(Stage::Copy, Outcome::Ok, 8, 10, 50);
    s.finish(Stage::Settle, Outcome::Empty, 0, 50, 50);
    s.finish(Stage::Copy, Outcome::Error, 8, 50, 60);
    assert_eq!(s.stages[Stage::Read as usize].cpu, 10);
    let copy = &s.stages[Stage::Copy as usize];
    assert_eq!(
        (copy.calls, copy.errors, copy.cpu, copy.bytes, copy.ok_bytes),
        (2, 1, 50, 16, 8)
    );
    assert_eq!(s.stages[Stage::Settle as usize].empty, 1);
}

#[test]
fn every_emitted_field_survives_exact_parser_including_maximum_u64() {
    let mut s = State::new(true);
    s.read(|| u64::MAX);
    s.finish(Stage::Copy, Outcome::Ok, u64::MAX, 0, u64::MAX);
    for worker in [false, true] {
        for final_record in [false, true] {
            for row in record::rows(&s, u64::MAX, u64::MAX, u64::MAX, worker, final_record) {
                assert!(row.len() < 4096);
                assert!(valid_record(&row), "{row}");
                assert!(!valid_record(&format!("{row} schema=1")));
                assert!(!valid_record(&row.replacen("schema=1", "schema=2", 1)));
                assert!(!valid_record(&format!("{row} unknown=1")));
                for field in row.split_whitespace().skip(1) {
                    assert!(
                        !valid_record(&row.replacen(&format!(" {field}"), "", 1)),
                        "missing {field}"
                    );
                }
            }
        }
    }
}

#[test]
fn tags_and_timers_restore_after_unwind_and_success_and_failure() {
    let rows = test_snapshot(|| {
        boundary(Boundary::BeforeTurn, || {
            count(Call::ExportPoll);
            let outcome = std::panic::catch_unwind(|| {
                boundary(Boundary::ImageCheck, || {
                    unit(Stage::Collect, || panic!("test unwind"));
                })
            });
            assert!(outcome.is_err());
            count(Call::ExportPoll);
        });
        count(Call::Custody);
        assert_eq!(result(Stage::Reserve, 0, || Err::<(), _>(7)), Err(7));
        assert_eq!(option::<()>(Stage::Settle, || None), None);
    });
    assert!(
        rows.iter()
            .any(|r| r.contains("boundary=before_turn export_polls=2"))
    );
    assert!(
        rows.iter()
            .any(|r| r.contains("boundary=other export_polls=0 file_checks=0 custody_checks=1"))
    );
    let collect = rows.iter().find(|r| r.contains(" stage=collect ")).unwrap();
    assert!(collect.contains("aborted=1"));
    assert!(
        rows.iter()
            .any(|r| r.contains("stage=reserve ") && r.contains("errors=1"))
    );
    assert!(
        rows.iter()
            .any(|r| r.contains("stage=settle ") && r.contains("empty=1"))
    );
}

#[test]
fn logical_records_distinguish_decode_from_admission_and_empty_flush() {
    let rows = test_snapshot(|| {
        turn(false, || {
            assert!(decode(0, || Ok::<_, ()>(())).is_ok());
            assert!(admission(0, || Err::<(), _>(())).is_err());
            assert!(decode(1, || Err::<(), _>(())).is_err());
            assert!(decode(2, || Ok::<_, ()>(())).is_ok());
        });
        turn(true, || {});
    });
    let s = &rows[0];
    assert!(s.contains("service_turns=1 service_empty=0 service_records=2"));
    assert!(s.contains("flush_turns=1 flush_empty=1 flush_records=0"));
    assert!(s.contains("staged_attempts=1 staged_errors=0 ring_attempts=1 ring_errors=1 ack_attempts=1 ack_errors=0"));
    assert!(s.contains("staged_writes=1 staged_refused=1"));
}

#[test]
fn worker_cadence_counts_completed_validation_including_failures() {
    let mut s = State::new(true);
    for n in 1..=513 {
        assert_eq!(s.job(8, n != 256), n % 256 == 0);
    }
    assert_eq!((s.jobs, s.job_errors, s.job_bytes), (513, 1, 4104));
    let reads = Cell::new(0);
    s.read(|| {
        reads.set(reads.get() + 1);
        100
    });
    assert_eq!(reads.get(), 1);
}

pub(crate) fn test_snapshot(run: impl FnOnce()) -> Vec<String> {
    struct Restore(Option<State>);
    impl Drop for Restore {
        fn drop(&mut self) {
            COST.with(|s| *s.borrow_mut() = self.0.take().expect("saved thread state"));
        }
    }
    let old = COST.with(|s| s.replace(State::new(true)));
    let _restore = Restore(Some(old));
    run();
    COST.with(|s| {
        let mut s = s.borrow_mut();
        let now = s.read(cpu).unwrap();
        s.snapshot();
        record::rows(&s, now, 1, 1, false, true)
    })
}

#[test]
fn classes_keep_decode_admission_and_service_flush_denominators_separate() {
    use sophia_protocol::shell_files::ShellFileKind;
    let rows = test_snapshot(|| {
        turn(false, || {
            record_class(ShellFileKind::FrameDemand, false);
            record_class(ShellFileKind::FrameDemand, true);
            record_class(ShellFileKind::FrameDemand, false); // Decoded, no custody.
            record_class(ShellFileKind::ResourceRetire, false);
            record_class(ShellFileKind::ResourceRetire, true);
            published(ShellFileKind::Submitted);
            published(ShellFileKind::FramePermit);
        });
        turn(true, || {
            record_class(ShellFileKind::Candidate, false);
            record_class(ShellFileKind::Candidate, true);
        });
        turn(false, || {});
        record_class(ShellFileKind::Negotiate, false); // Outside any service turn.
    });
    for (class, expected) in [
        (
            "frame_demand",
            "decoded=2 admitted=1 service_records=2 service_turns=1 flush_records=0 flush_turns=0",
        ),
        (
            "resource_retire",
            "decoded=1 admitted=1 service_records=1 service_turns=1 flush_records=0 flush_turns=0",
        ),
        (
            "candidate",
            "decoded=1 admitted=1 service_records=0 service_turns=0 flush_records=1 flush_turns=1",
        ),
        (
            "other",
            "decoded=1 admitted=0 service_records=0 service_turns=0 flush_records=0 flush_turns=0",
        ),
    ] {
        let row = rows
            .iter()
            .find(|r| r.contains(&format!(" class={class} ")))
            .unwrap();
        assert!(row.contains(expected), "{row}");
        assert!(valid_record(row));
        assert!(!valid_record(
            &row.replace("direction=incoming", "direction=outgoing")
        ));
        assert!(!valid_record(
            &row.replace(&format!("class={class}"), "class=unknown")
        ));
    }
    assert!(rows[0].contains("server_frame_permits=1"));
    let mut s = State::new(true);
    s.classes[0].decoded = u64::MAX;
    s.class(0, false);
    s.server_frame_permits = u64::MAX;
    s.permit();
    assert_eq!(s.saturations, 2);
    assert!(!s.valid());
}
