//! The output-unplug harness's waits and guest endpoint
//! (tools/qemu_unplug_endpoint.sh), driven by bash with stand-in processes in
//! place of QEMU and the serial logger: sleep, `sh -c 'exit N'`, a FIFO reader.
//! No QEMU, device, display, network or guest.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static NEXT: AtomicUsize = AtomicUsize::new(0);
/// Far above any bound the helper is given here (stop, KILL and logger graces of
/// one second, deadlines of at most five).
const DRIVER_BOUND: Duration = Duration::from_secs(60);

fn helper() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/qemu_unplug_endpoint.sh")
}

struct Run {
    stdout: String,
    evidence: Vec<String>,
    status: Option<i32>,
    elapsed: Duration,
    dir: PathBuf,
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Runs SCRIPT under the harness's own shell options with the helper sourced,
/// in a fresh directory holding the evidence file E. The stop and logger
/// graces are one second so the bounded paths finish quickly.
fn run(script: &str) -> Run {
    let dir = std::env::temp_dir().join(format!(
        "sophia-unplug-endpoint-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let evidence = dir.join("evidence.log");
    std::fs::write(&evidence, "").unwrap();
    let full = format!(
        "set -euo pipefail\nsource '{}'\nE='{}'\nUNPLUG_STOP_GRACE_S=1\nUNPLUG_LOGGER_GRACE_S=1\nUNPLUG_KILL_GRACE_S=1\n{script}\n",
        helper().display(),
        evidence.display()
    );
    // The driver itself is bounded, so a wait that regresses into blocking
    // fails this test instead of hanging the gate.
    let stdout_path = dir.join("stdout.log");
    let started = Instant::now();
    let mut child = Command::new("bash")
        .arg("-c")
        .arg(full)
        .current_dir(&dir)
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", dir.join("bin").display()),
        )
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&stdout_path).unwrap())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() > DRIVER_BOUND {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the bash driver outlived {DRIVER_BOUND:?}: {script}");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let elapsed = started.elapsed();
    let stdout = std::fs::read_to_string(&stdout_path).unwrap();
    let evidence = std::fs::read_to_string(&evidence)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    Run {
        stdout,
        evidence,
        status: status.code(),
        elapsed,
        dir,
    }
}

fn wait_outcome(setup: &str, steps: u32) -> (String, Duration) {
    let result = run(&format!(
        "{setup}\nunplug_wait_for \"$E\" {steps} \"$p\" grep -q '^marker$' \"$E\"\nkill \"$p\" 2>/dev/null || true\n"
    ));
    assert_eq!(result.status, Some(0), "{}", result.stdout);
    (result.stdout.trim().to_owned(), result.elapsed)
}

#[test]
fn each_wait_ends_in_exactly_one_named_outcome() {
    let live = "sleep 30 & p=$!";
    let ended = "sh -c 'exit 0' & p=$!; sleep 0.3";
    for (label, setup, steps, expected) in [
        (
            "marker",
            format!("echo marker >> \"$E\"; {live}"),
            20,
            "ready",
        ),
        (
            "guest failure record",
            format!(
                "echo 'sophia_qemu_guest schema=1 status=failed reason=unplug_session_exit' >> \"$E\"; {live}"
            ),
            20,
            "guest_failed",
        ),
        (
            "host failure record",
            format!("echo 'sophia_qemu_unplug schema=1 status=failed reason=x' >> \"$E\"; {live}"),
            20,
            "guest_failed",
        ),
        // A failure already recorded wins over a marker present beside it.
        (
            "failure and marker together",
            format!(
                "echo marker >> \"$E\"; echo 'sophia_qemu_guest schema=1 status=failed reason=x' >> \"$E\"; {live}"
            ),
            20,
            "guest_failed",
        ),
        ("process gone", ended.to_owned(), 20, "guest_exited"),
        // Without a failure, a marker that arrived with the exit still counts.
        (
            "marker and exit together",
            format!("echo marker >> \"$E\"; {ended}"),
            20,
            "ready",
        ),
        ("nothing within the bound", live.to_owned(), 10, "timeout"),
    ] {
        let (outcome, _) = wait_outcome(&setup, steps);
        assert_eq!(outcome, expected, "{label}");
    }
    // The bound is honoured: ten polls of 0.05 s, not an open wait.
    let (_, elapsed) = wait_outcome(live, 10);
    assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
}

/// A stand-in that ignores TERM writes ignoring.ready once its trap is set, and
/// the script waits for it (inside the driver's bound) before any signal can
/// reach it, so TERM never arrives before the trap.
const AWAIT_IGNORING: &str = "while [[ ! -e ignoring.ready ]]; do sleep 0.05; done\n";

fn collect(qemu: &str, logger: &str, deadline_s: u32) -> Run {
    let await_ready = if format!("{qemu}{logger}").contains("ignoring.ready") {
        AWAIT_IGNORING
    } else {
        ""
    };
    run(&format!(
        "{qemu} & q=$!\n{logger} & l=$!\n{await_ready}rc=0\nunplug_collect_endpoint \"$E\" \"$q\" \"$l\" \"$((SECONDS + {deadline_s}))\" || rc=$?\necho \"rc=$rc\"\n"
    ))
}

#[test]
fn a_clean_end_of_both_processes_is_the_only_guest_exited() {
    let result = collect("sh -c 'exit 0'", "sh -c 'exit 0'", 5);
    assert_eq!(
        result.evidence,
        ["sophia_qemu_unplug schema=1 status=guest_exited qemu_exit=0"]
    );
    assert!(result.stdout.ends_with("rc=0\n"), "{}", result.stdout);
    // A real FIFO logger that ends when the writer closes.
    let result = run(
        "mkfifo serial\n( sleep 0.2; echo line ) > serial & q=$!\ncat serial > /dev/null & l=$!\nrc=0\nunplug_collect_endpoint \"$E\" \"$q\" \"$l\" \"$((SECONDS + 5))\" || rc=$?\necho \"rc=$rc\"\n",
    );
    assert_eq!(
        result.evidence,
        ["sophia_qemu_unplug schema=1 status=guest_exited qemu_exit=0"]
    );
}

#[test]
fn a_non_zero_end_is_recorded_with_its_real_statuses() {
    let result = collect("sh -c 'exit 3'", "sh -c 'exit 0'", 5);
    assert_eq!(
        result.evidence,
        ["sophia_qemu_unplug schema=1 status=failed reason=guest_exit qemu_exit=3 logger_exit=0"]
    );
    assert!(result.stdout.ends_with("rc=1\n"));
    let result = collect("sh -c 'exit 0'", "sh -c 'exit 1'", 5);
    assert_eq!(
        result.evidence,
        ["sophia_qemu_unplug schema=1 status=failed reason=guest_exit qemu_exit=0 logger_exit=1"]
    );
}

#[test]
fn a_stopped_process_is_named_and_never_reads_as_guest_exited() {
    // QEMU outlives its deadline: TERM ends it.
    let result = collect("sleep 30", "sh -c 'exit 0'", 1);
    assert_eq!(
        result.evidence,
        [
            "sophia_qemu_unplug schema=1 status=failed reason=host_timeout",
            "sophia_qemu_unplug schema=1 status=guest_stopped qemu_exit=143 logger_exit=0 qemu_signal=TERM logger_signal=none",
        ]
    );
    assert!(
        result.elapsed < Duration::from_secs(6),
        "{:?}",
        result.elapsed
    );
    // QEMU ignores TERM: KILL after the stop grace.
    let result = collect(
        "bash -c 'trap \"\" TERM; : > ignoring.ready; while :; do sleep 0.1; done'",
        "sh -c 'exit 0'",
        1,
    );
    assert_eq!(
        result.evidence,
        [
            "sophia_qemu_unplug schema=1 status=failed reason=host_timeout",
            "sophia_qemu_unplug schema=1 status=guest_stopped qemu_exit=137 logger_exit=0 qemu_signal=KILL logger_signal=none",
        ]
    );
    // Only the logger had to be stopped: QEMU's clean end is kept, the stop is the logger's.
    let result = collect("sh -c 'exit 0'", "sleep 30", 5);
    assert_eq!(
        result.evidence,
        [
            "sophia_qemu_unplug schema=1 status=guest_stopped qemu_exit=0 logger_exit=143 qemu_signal=none logger_signal=TERM"
        ]
    );
    assert!(result.stdout.ends_with("rc=1\n"));
    for result in [
        collect("sleep 30", "sh -c 'exit 0'", 1),
        collect("sh -c 'exit 0'", "sleep 30", 5),
    ] {
        assert!(
            !result
                .evidence
                .iter()
                .any(|line| line.contains("status=guest_exited"))
        );
    }
}

/// A stand-in cargo that records every call, the harness globals, and a cleanup.
const HARNESS: &str = "mkdir -p bin\nprintf '#!/bin/sh\\necho \"$@\" >> \"%s/cargo.calls\"\\n' \"$PWD\" > bin/cargo\nchmod +x bin/cargo\nEVIDENCE_FILE=$E\nROOT_DIR=$PWD\nUNPLUG_MODE=input-return\nUNPLUG_HOST_BOUND_S=5\nVNC_SOCKET=$PWD/display.sock\nQMP_SOCKET=$PWD/qmp.sock\nSERIAL_FIFO=$PWD/serial.fifo\nDISPLAY_BUS_SOCKET=$PWD/display-bus.sock\ntouch \"$VNC_SOCKET\" \"$QMP_SOCKET\" \"$SERIAL_FIFO\" \"$DISPLAY_BUS_SOCKET\"\n";

#[test]
fn the_series_153_shape_names_the_guest_failure_and_still_records_a_clean_end() {
    // The guest reports its failure and powers off; QEMU then exits 0.
    let result = run(&format!(
        "{HARNESS}echo 'sophia_qemu_guest schema=1 status=failed reason=unplug_session_exit scenario=output-unplug exit_status=1' >> \"$E\"\nsh -c 'sleep 0.3; exit 0' & QEMU_PID=$!\nsh -c 'sleep 0.3; exit 0' & LOGGER_PID=$!\noutcome=$(unplug_wait_for \"$E\" 800 \"$QEMU_PID\" grep -qE '^sophia_qemu_unplug schema=1 status=input_baseline_ready device=[0-9]+$' \"$E\")\n[[ \"$outcome\" == ready ]] || unplug_failed \"baseline_ready_$outcome\"\necho unreachable\n"
    ));
    assert_eq!(result.status, Some(1), "{}", result.stdout);
    assert_eq!(
        result.evidence[1..],
        [
            "sophia_qemu_unplug schema=1 status=failed reason=baseline_ready_guest_failed",
            "sophia_qemu_unplug schema=1 status=guest_exited qemu_exit=0",
        ]
    );
    assert!(!result.stdout.contains("unreachable"));
    assert!(
        !result.dir.join("cargo.calls").exists(),
        "no verifier on a failure path"
    );
}

#[test]
fn every_failure_keeps_its_record_first_and_exits_one_without_a_verifier() {
    for (qemu, logger, last) in [
        (
            "sh -c 'exit 0'",
            "sh -c 'exit 0'",
            "sophia_qemu_unplug schema=1 status=guest_exited qemu_exit=0",
        ),
        (
            "sh -c 'exit 3'",
            "sh -c 'exit 0'",
            "sophia_qemu_unplug schema=1 status=failed reason=guest_exit qemu_exit=3 logger_exit=0",
        ),
        (
            "sleep 30",
            "sh -c 'exit 0'",
            "sophia_qemu_unplug schema=1 status=guest_stopped qemu_exit=143 logger_exit=0 qemu_signal=TERM logger_signal=none",
        ),
    ] {
        let result = run(&format!(
            "{HARNESS}UNPLUG_HOST_BOUND_S=1\n{qemu} & QEMU_PID=$!\n{logger} & LOGGER_PID=$!\nunplug_failed head_disable\n"
        ));
        assert_eq!(result.status, Some(1));
        assert_eq!(
            result.evidence[0],
            "sophia_qemu_unplug schema=1 status=failed reason=head_disable"
        );
        assert_eq!(result.evidence.last().unwrap(), last);
        assert!(!result.dir.join("cargo.calls").exists());
    }
}

#[test]
fn the_normal_end_runs_the_verifier_only_after_guest_exited() {
    let result = run(&format!(
        "{HARNESS}sh -c 'exit 0' & QEMU_PID=$!\nsh -c 'exit 0' & LOGGER_PID=$!\nunplug_finish\n"
    ));
    assert_eq!(result.status, Some(0), "{}", result.stdout);
    assert_eq!(
        result.evidence,
        ["sophia_qemu_unplug schema=1 status=guest_exited qemu_exit=0"]
    );
    let calls = std::fs::read_to_string(result.dir.join("cargo.calls")).unwrap();
    assert_eq!(
        calls,
        format!(
            "xtask conformance verify output-unplug input-return {}\n",
            result.dir.join("evidence.log").display()
        )
    );
    let result = run(&format!(
        "{HARNESS}sh -c 'exit 0' & QEMU_PID=$!\nsh -c 'exit 1' & LOGGER_PID=$!\nunplug_finish\n"
    ));
    assert_eq!(result.status, Some(1));
    assert_eq!(
        result.evidence,
        ["sophia_qemu_unplug schema=1 status=failed reason=guest_exit qemu_exit=0 logger_exit=1"]
    );
    assert!(!result.dir.join("cargo.calls").exists());
}

#[test]
fn a_simulated_failed_to_reap_process_is_recorded_unreaped_and_never_waited_for() {
    // SIMULATED failed-to-reap path: a process that has not terminated by the
    // post-KILL deadline. This test shell's own kill() withholds KILL and
    // delegates every other call to the builtin, and the stand-in ignores TERM,
    // so it is still present when the KILL grace ends; the helper must record
    // it as unreaped and not wait for it. The test ends it with the builtin.
    let result = run(
        "kill() { if [[ \"$1\" == -KILL ]]; then return 0; fi; builtin kill \"$@\"; }\nbash -c 'trap \"\" TERM; : > ignoring.ready; while :; do sleep 0.1; done' & q=$!\nsh -c 'exit 0' & l=$!\nwhile [[ ! -e ignoring.ready ]]; do sleep 0.05; done\nrc=0\nunplug_collect_endpoint \"$E\" \"$q\" \"$l\" \"$((SECONDS + 1))\" || rc=$?\necho \"rc=$rc q=$q\"\nbuiltin kill -KILL \"$q\"\n",
    );
    let q = result.stdout.trim().rsplit("q=").next().unwrap().to_owned();
    assert_eq!(
        result.evidence,
        [
            "sophia_qemu_unplug schema=1 status=failed reason=host_timeout".to_owned(),
            format!(
                "sophia_qemu_unplug schema=1 status=guest_stopped qemu_exit=unreaped logger_exit=0 qemu_signal=KILL logger_signal=none qemu_pid={q}"
            ),
        ]
    );
    assert!(result.stdout.contains("rc=1 "), "{}", result.stdout);
    assert!(
        result.elapsed < Duration::from_secs(10),
        "{:?}",
        result.elapsed
    );
}

#[test]
fn the_display_bus_is_ended_within_bounds_and_named_as_itself() {
    for (bus, last) in [
        (
            "sleep 30",
            "sophia_qemu_unplug schema=1 status=display_bus_ended display_bus_exit=143 display_bus_signal=TERM",
        ),
        (
            "bash -c 'trap \"\" TERM; : > ignoring.ready; while :; do sleep 0.1; done'",
            "sophia_qemu_unplug schema=1 status=display_bus_ended display_bus_exit=137 display_bus_signal=KILL",
        ),
    ] {
        let result = run(&format!(
            "{HARNESS}{bus} & DISPLAY_BUS_PID=$!\n{}QEMU_PID=\"\"\nunplug_cleanup\n",
            if bus.contains("ignoring.ready") {
                AWAIT_IGNORING
            } else {
                ""
            }
        ));
        assert_eq!(result.status, Some(0), "{}", result.stdout);
        assert_eq!(result.evidence, [last]);
        assert!(
            result.elapsed < Duration::from_secs(8),
            "{:?}",
            result.elapsed
        );
        for socket in [
            "display.sock",
            "qmp.sock",
            "serial.fifo",
            "display-bus.sock",
        ] {
            assert!(!result.dir.join(socket).exists(), "{socket} left behind");
        }
    }
}

#[test]
fn an_unreaped_display_bus_refuses_the_verifier_after_a_clean_guest() {
    // SIMULATED failed-to-reap bus, as above: kill() withholds KILL. QEMU and
    // the logger end cleanly, yet the run must not reach the verifier.
    let result = run(&format!(
        "{HARNESS}kill() {{ if [[ \"$1\" == -KILL ]]; then return 0; fi; builtin kill \"$@\"; }}\nbash -c 'trap \"\" TERM; : > ignoring.ready; while :; do sleep 0.1; done' & DISPLAY_BUS_PID=$!\nwhile [[ ! -e ignoring.ready ]]; do sleep 0.05; done\nbus=$DISPLAY_BUS_PID\ntrap 'builtin kill -KILL \"$bus\" 2>/dev/null || true' EXIT\nsh -c 'exit 0' & QEMU_PID=$!\nsh -c 'exit 0' & LOGGER_PID=$!\necho \"bus=$bus\"\nunplug_finish\n"
    ));
    assert_eq!(result.status, Some(1), "{}", result.stdout);
    let bus = result
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("bus="))
        .unwrap()
        .to_owned();
    assert_eq!(
        result.evidence,
        [
            "sophia_qemu_unplug schema=1 status=guest_exited qemu_exit=0".to_owned(),
            format!(
                "sophia_qemu_unplug schema=1 status=display_bus_ended display_bus_exit=unreaped display_bus_signal=KILL display_bus_pid={bus}"
            ),
        ]
    );
    assert!(
        !result.dir.join("cargo.calls").exists(),
        "no verifier after an unreaped bus"
    );
}

#[test]
fn an_unexpected_exit_records_a_stopped_guest_not_a_clean_one() {
    // The harness's EXIT trap with the guest still running.
    let result = run(&format!(
        "{HARNESS}sleep 30 & QEMU_PID=$!\nsh -c 'exit 0' & LOGGER_PID=$!\nunplug_cleanup\n"
    ));
    assert_eq!(
        result.evidence,
        [
            "sophia_qemu_unplug schema=1 status=failed reason=harness_exit",
            "sophia_qemu_unplug schema=1 status=guest_stopped qemu_exit=143 logger_exit=0 qemu_signal=TERM logger_signal=none",
        ]
    );
}
