use super::*;
#[test]
fn phases_partition_cpu_and_pass_boundaries_reset_without_counting_sleep() {
    let mut r = Recorder::new(0);
    r.begin_at(Stamp {
        wall: 0,
        after: 0,
        cpu: 10,
    });
    r.phase_at(Phase::Components, 12);
    r.phase_at(Phase::Other, 20);
    r.phase_at(Phase::Authority, 22);
    r.phase_at(Phase::Native, 23);
    r.phase_at(Phase::Authority, 26);
    r.begin_at(Stamp {
        wall: 1_000_000,
        after: 1_000_000,
        cpu: 30,
    });
    assert_eq!(r.rows[0].cpu, [4, 8, 3, 5]);
    assert_eq!(r.rows[0].cpu.iter().sum::<u64>(), 20);
    r.close(Stamp {
        wall: 2_000_000,
        after: 2_000_000,
        cpu: 33,
    });
    assert_eq!(r.rows[1].cpu, [3, 0, 0, 0]);
    r.close(Stamp {
        wall: 3_000_000,
        after: 3_000_000,
        cpu: 33,
    });
    assert_eq!(r.rows.len(), 2);
}
#[test]
fn bounded_storage_reports_every_omitted_pass() {
    let mut r = Recorder::new(0);
    for i in 0..LIMIT as u64 + 3 {
        r.begin_at(Stamp {
            wall: i,
            after: i,
            cpu: i,
        });
    }
    r.close(Stamp {
        wall: LIMIT as u64 + 3,
        after: LIMIT as u64 + 3,
        cpu: LIMIT as u64 + 3,
    });
    assert_eq!(r.rows.len(), LIMIT);
    assert_eq!(r.overflow, 3);
}

#[test]
fn measured_error_restores_enclosing_phase() {
    let mut r = Recorder::new(0);
    r.begin_pass([0, 0], 0, [0, 0]);
    r.phase(Phase::Authority);
    let result: Result<(), ()> = r.measure(Phase::Native, || Err(()));
    assert!(result.is_err());
    assert_eq!(r.phase as usize, Phase::Authority as usize);
    r.close(stamp());
    assert_eq!(
        r.rows[0].cpu.iter().sum::<u64>(),
        r.rows[0].end.cpu - r.rows[0].start.cpu
    );
}

#[test]
#[ignore = "isolated process installs the global evidence sink"]
fn emitted_record_and_footer_exercise_real_formatter() {
    crate::install_session_output(crate::SessionOutput::new(
        |line| println!("{line}"),
        |line| eprintln!("{line}"),
    ))
    .unwrap();
    let mut r = Recorder::new(7);
    r.begin_pass([1, 2], 4, [3, 1]);
    assert_eq!((r.cause, r.turns), (4, [3, 1]));
    r.begin_pass([1, 2], 8, [4, 2]);
    // The wait that ended at the second loop top closes the first row.
    assert_eq!(r.rows[0].cause, 8);
    r.measure(Phase::Components, || std::hint::black_box(1));
    r.finish(true, 8);
}
