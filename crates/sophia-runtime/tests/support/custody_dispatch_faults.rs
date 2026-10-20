//! Private observer fault seam. Public callers can only observe real pidfds.
use super::{CustodyDispatch, LaunchCustody};
use std::{
    process::{Child, Command},
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

struct Process(Child);
impl Process {
    fn new() -> (Self, LaunchCustody) {
        let process = Self(Command::new("/usr/bin/sleep").arg("30").spawn().unwrap());
        let pid = rustix::process::Pid::from_raw(process.0.id() as i32).unwrap();
        // Host-namespace custody pins the child in both roles. The test only
        // injects observer errors; protected-spawn identity has separate tests.
        let custody = LaunchCustody::new(
            Arc::new(AtomicBool::new(true)),
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

#[test]
fn interrupted_observation_retries_but_fatal_observer_failure_revokes_the_set() {
    let (mut first, one) = Process::new();
    let (mut second, two) = Process::new();
    let registered = [one.clone(), two.clone()];
    let age = Duration::from_secs(5);
    let mut previous = CustodyDispatch::observe(&registered, age, 4).unwrap();
    let interrupted = CustodyDispatch::observe_with(&registered, age, 4, |fds| {
        assert_eq!(fds.len(), 4);
        Err(rustix::io::Errno::INTR)
    })
    .unwrap_err();
    assert_eq!(interrupted.kind(), std::io::ErrorKind::Interrupted);
    assert!(one.is_live().unwrap() && two.is_live().unwrap());
    assert_eq!(previous.check(&one), Ok(true));
    let mut retried = CustodyDispatch::observe(&registered, age, 2).unwrap();
    assert_eq!(retried.check(&one), Ok(true));
    let failed = CustodyDispatch::observe_with(&registered, age, 4, |fds| {
        assert_eq!(fds.len(), 4);
        Err(rustix::io::Errno::NOMEM)
    })
    .unwrap_err();
    assert_eq!(
        failed.raw_os_error(),
        Some(rustix::io::Errno::NOMEM.raw_os_error())
    );
    assert_eq!(previous.check(&one), Ok(false));
    assert_eq!(previous.check(&two), Ok(false));
    assert_eq!(retried.check(&two), Ok(false));
    assert!(!one.is_live().unwrap() && !two.is_live().unwrap());
    assert!(first.0.try_wait().unwrap().is_none() && second.0.try_wait().unwrap().is_none());
}
