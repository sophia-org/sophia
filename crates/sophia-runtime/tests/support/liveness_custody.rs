// Direct `LaunchCustody` cases: an executed poll counts once in the current
// cell whatever it answers; the inactive early return counts nothing.
use std::{
    process::{Child, Command},
    sync::{Arc, atomic::AtomicBool},
};

use super::LaunchCustody;
use crate::liveness_counts::{self, Boundary, Entry, Field};

struct Process(Child);

impl Process {
    fn custody(active: bool) -> (Self, LaunchCustody) {
        let process = Self(Command::new("/usr/bin/sleep").arg("30").spawn().unwrap());
        let pid = rustix::process::Pid::from_raw(process.0.id() as i32).unwrap();
        let custody = LaunchCustody::new(
            Arc::new(AtomicBool::new(active)),
            rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty()).unwrap(),
            rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty()).unwrap(),
        );
        (process, custody)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn polls(entry: Entry, boundary: Boundary) -> u64 {
    liveness_counts::snapshot().get(entry, boundary).get(Field::CustodyPolls)
}

#[test]
fn an_active_custody_poll_counts_once_in_the_cell_it_runs_in() {
    let (_process, custody) = Process::custody(true);
    let outside = polls(Entry::Outside, Boundary::Other);
    let post_turn = polls(Entry::Turn, Boundary::PostTurn);
    assert!(custody.is_live().unwrap());
    assert_eq!(polls(Entry::Outside, Boundary::Other), outside + 1);
    {
        let _turn = liveness_counts::enter(Entry::Turn);
        let _post = liveness_counts::within(Boundary::PostTurn);
        assert!(custody.is_live().unwrap());
    }
    assert_eq!(polls(Entry::Turn, Boundary::PostTurn), post_turn + 1);
    assert_eq!(polls(Entry::Outside, Boundary::Other), outside + 1);
}

#[test]
fn an_inactive_custody_answers_without_a_poll() {
    let (_process, custody) = Process::custody(false);
    let before = liveness_counts::snapshot();
    assert!(!custody.is_live().unwrap());
    assert_eq!(liveness_counts::snapshot(), before);
}

#[test]
fn an_exited_process_is_a_counted_poll_that_refuses_custody() {
    let (mut process, custody) = Process::custody(true);
    process.0.kill().unwrap();
    process.0.wait().unwrap();
    let before = polls(Entry::Outside, Boundary::Other);
    assert!(!custody.is_live().unwrap());
    assert_eq!(polls(Entry::Outside, Boundary::Other), before + 1);
}
