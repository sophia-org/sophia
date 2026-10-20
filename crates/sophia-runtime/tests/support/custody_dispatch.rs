//! Real pidfds pin the proposed dispatch contract, including its intentional
//! difference from caller-fresh polling. These are not native performance tests.
use super::{Probe, ProtectionDomainRole, ready_probe};
use sophia_runtime::{CustodyDispatch, CustodyDispatchLimit, LaunchCustody, ProcessSupervisor};
use std::time::{Duration, Instant};

const TEST_AGE: Duration = Duration::from_secs(5);

fn custody(
    supervisor: &mut ProcessSupervisor,
    reservation: &sophia_runtime::LaunchReservation,
) -> LaunchCustody {
    supervisor
        .launch_custody(reservation, ProtectionDomainRole::MetadataShell)
        .unwrap()
        .unwrap()
}

struct Resume(rustix::process::Pid);
impl Drop for Resume {
    fn drop(&mut self) {
        let _ = rustix::process::kill_process(self.0, rustix::process::Signal::CONT);
    }
}

#[test]
fn death_mid_dispatch_is_applied_at_the_next_boundary_without_wrapper_reap() {
    let probe = Probe::build();
    let (mut supervisor, _owner, reservation) = ready_probe(&probe, 1);
    let one = custody(&mut supervisor, &reservation);
    let another_handle = custody(&mut supervisor, &reservation);
    let registered = [one.clone()];
    let wrapper = rustix::process::Pid::from_raw(supervisor.child_id().unwrap() as i32).unwrap();
    let peer = rustix::process::Pid::from_raw(supervisor.peer_id().unwrap() as i32).unwrap();
    assert_ne!(wrapper, peer);
    rustix::process::kill_process(wrapper, rustix::process::Signal::STOP).unwrap();
    let resume = Resume(wrapper);
    let mut dispatch = CustodyDispatch::observe(&registered, TEST_AGE, 4).unwrap();
    assert_eq!(dispatch.check(&another_handle), Ok(true));
    rustix::process::kill_process(peer, rustix::process::Signal::KILL).unwrap();
    {
        // Only establish that the exit has happened. This wait does not apply
        // an observation or reap either process on behalf of the experiment.
        let mut fds = Vec::new();
        one.visit_poll_fds(|fd| fds.push(fd));
        assert!(
            rustix::event::poll(
                &mut fds,
                Some(&rustix::event::Timespec {
                    tv_sec: 2,
                    tv_nsec: 0
                })
            )
            .unwrap()
                > 0
        );
        assert!(fds[0].revents().is_empty(), "wrapper is still alive");
        assert!(!fds[1].revents().is_empty());
    }
    assert!(
        !one.is_live().unwrap(),
        "old per-call contract observes exit now"
    );
    assert_eq!(
        dispatch.check(&one),
        Ok(true),
        "dispatch contract intentionally permits bounded in-flight work"
    );
    let mut next = CustodyDispatch::observe(&registered, TEST_AGE, 2).unwrap();
    assert_eq!(next.check(&one), Ok(false));
    assert_eq!(
        dispatch.check(&another_handle),
        Ok(false),
        "revocation reaches existing dispatches and independently issued handles"
    );
    assert!(
        supervisor.child_id().is_some(),
        "no supervisor reap was needed"
    );
    let mut count = 0;
    one.visit_poll_fds(|_| count += 1);
    assert_eq!(
        count, 0,
        "revoked identity must not leave persistent wake readiness"
    );
    drop(resume);
    supervisor.terminate().unwrap();
}

#[test]
fn busy_dispatches_observe_exit_and_withdrawal_is_immediate_and_launch_scoped() {
    let probe = Probe::build();
    let (mut first, _owner1, reservation1) = ready_probe(&probe, 1);
    let (mut second, _owner2, reservation2) = ready_probe(&probe, 2);
    let one = custody(&mut first, &reservation1);
    let two = custody(&mut second, &reservation2);
    let registered = [one.clone(), two.clone()];
    let only_one = [one.clone()];
    let mut restricted = CustodyDispatch::observe(&only_one, TEST_AGE, 2).unwrap();
    assert_eq!(
        restricted.check(&two),
        Err(CustodyDispatchLimit::Unregistered)
    );
    let mut before = CustodyDispatch::observe(&registered, TEST_AGE, 4).unwrap();
    assert_eq!(before.check(&one), Ok(true));
    assert_eq!(before.check(&two), Ok(true));
    first.request_termination().unwrap();
    assert_eq!(
        before.check(&one),
        Ok(false),
        "explicit withdrawal needs no subsequent kernel poll"
    );
    assert_eq!(
        before.check(&two),
        Ok(true),
        "another launch retains its own authority"
    );
    let peer = rustix::process::Pid::from_raw(second.peer_id().unwrap() as i32).unwrap();
    rustix::process::kill_process(peer, rustix::process::Signal::KILL).unwrap();
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        // There is deliberately no blocking owner wait or supervisor poll.
        let mut dispatch = CustodyDispatch::observe(&registered, TEST_AGE, 2).unwrap();
        assert_eq!(dispatch.check(&one), Ok(false));
        if dispatch.check(&two) == Ok(false) {
            break;
        }
        assert!(
            Instant::now() < end,
            "busy dispatch never applied exit readiness"
        );
        std::thread::yield_now();
    }
    first.terminate().unwrap();
    second.terminate().unwrap();
}

#[test]
fn dispatch_limits_require_a_new_observation_and_invalid_registration_refuses() {
    let probe = Probe::build();
    let (mut supervisor, _owner, reservation) = ready_probe(&probe, 1);
    let one = custody(&mut supervisor, &reservation);
    let registered = [one.clone()];
    let mut dispatch = CustodyDispatch::observe(&registered, TEST_AGE, 1).unwrap();
    assert_eq!(dispatch.check(&one), Ok(true));
    assert_eq!(dispatch.check(&one), Err(CustodyDispatchLimit::Exhausted));
    let mut dispatch = dispatch.into_owned();
    assert_eq!(dispatch.check(&one), Err(CustodyDispatchLimit::Exhausted));
    let mut next = CustodyDispatch::observe(&registered, TEST_AGE, 1).unwrap();
    assert_eq!(next.check(&one), Ok(true));
    assert_eq!(dispatch.check(&one), Err(CustodyDispatchLimit::Exhausted));
    let mut expired = CustodyDispatch::observe(&registered, Duration::from_millis(1), 2).unwrap();
    std::thread::sleep(Duration::from_millis(2));
    assert_eq!(expired.check(&one), Err(CustodyDispatchLimit::Expired));
    let mut expired = expired.into_owned();
    assert_eq!(expired.check(&one), Err(CustodyDispatchLimit::Expired));
    for result in [
        CustodyDispatch::observe(&[], TEST_AGE, 1),
        CustodyDispatch::observe(&registered, Duration::ZERO, 1),
        CustodyDispatch::observe(&registered, TEST_AGE, 0),
    ] {
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
    }
    let duplicate = [one.clone(), custody(&mut supervisor, &reservation)];
    assert_eq!(
        CustodyDispatch::observe(&duplicate, TEST_AGE, 1)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidInput
    );
    assert!(
        one.is_live().unwrap(),
        "invalid local configuration did not revoke the launch"
    );
    supervisor.terminate().unwrap();
}

fn thread_ns() -> u64 {
    let time = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
    u64::try_from(time.tv_sec).unwrap() * 1_000_000_000 + u64::try_from(time.tv_nsec).unwrap()
}

#[test]
#[ignore = "explicit release-mode mechanism timing; not a native acceptance result"]
#[allow(clippy::assertions_on_constants)]
fn measure_dispatch_against_fresh_checks() {
    use std::hint::black_box;
    assert!(!cfg!(debug_assertions), "run timing with --release");
    let probe = Probe::build();
    let (mut supervisor, _owner, reservation) = ready_probe(&probe, 1);
    let one = custody(&mut supervisor, &reservation);
    let registered = [one.clone()];
    const DISPATCHES: usize = 4096;
    for checks in [1, 8, 32] {
        let measure = |aggregate: bool| {
            let start = thread_ns();
            for _ in 0..DISPATCHES {
                if aggregate {
                    let mut dispatch = CustodyDispatch::observe(
                        black_box(&registered),
                        TEST_AGE,
                        black_box(checks),
                    )
                    .unwrap();
                    for _ in 0..checks {
                        assert!(black_box(dispatch.check(black_box(&one)).unwrap()));
                    }
                } else {
                    for _ in 0..checks {
                        assert!(black_box(black_box(&one).is_live().unwrap()));
                    }
                }
            }
            thread_ns() - start
        };
        // Identical process identities. Include observation allocation, poll,
        // identity lookup, age and count checks, and drop in the timed region.
        for _ in 0..2 {
            black_box(measure(false));
            black_box(measure(true));
        }
        let mut fresh = [0; 8];
        let mut aggregate = [0; 8];
        for round in 0..8 {
            if round % 2 == 0 {
                fresh[round] = measure(false);
                aggregate[round] = measure(true);
            } else {
                aggregate[round] = measure(true);
                fresh[round] = measure(false);
            }
            println!(
                "custody_dispatch_timing checks={checks} dispatches={DISPATCHES} round={round} fresh_ns={} aggregate_ns={}",
                fresh[round], aggregate[round]
            );
        }
        fresh.sort_unstable();
        aggregate.sort_unstable();
        let operations = (DISPATCHES * checks) as f64;
        println!(
            "custody_dispatch_summary checks={checks} dispatches={DISPATCHES} fresh_median_ns={} aggregate_median_ns={} fresh_ns_per_check={:.2} aggregate_ns_per_check={:.2}",
            fresh[4],
            aggregate[4],
            fresh[4] as f64 / operations,
            aggregate[4] as f64 / operations
        );
    }
    supervisor.terminate().unwrap();
}

#[test]
fn wrapper_exit_revokes_only_its_registered_launch() {
    let probe = Probe::build();
    let (mut first, _owner1, reservation1) = ready_probe(&probe, 1);
    let (mut second, _owner2, reservation2) = ready_probe(&probe, 2);
    let one = custody(&mut first, &reservation1);
    let two = custody(&mut second, &reservation2);
    let registered = [one.clone(), two.clone()];
    let wrapper = rustix::process::Pid::from_raw(first.child_id().unwrap() as i32).unwrap();
    rustix::process::kill_process(wrapper, rustix::process::Signal::KILL).unwrap();
    {
        let mut fds = Vec::new();
        one.visit_poll_fds(|fd| fds.push(fd));
        // Only wait for the wrapper; its peer's fate is a containment policy.
        assert!(
            rustix::event::poll(
                &mut fds[..1],
                Some(&rustix::event::Timespec {
                    tv_sec: 2,
                    tv_nsec: 0
                })
            )
            .unwrap()
                > 0
        );
    }
    let mut dispatch = CustodyDispatch::observe(&registered, TEST_AGE, 2).unwrap();
    assert_eq!(dispatch.check(&one), Ok(false));
    assert_eq!(dispatch.check(&two), Ok(true));
    first.terminate().unwrap();
    second.terminate().unwrap();
}

#[test]
fn replacement_cannot_reuse_the_previous_dispatch_and_cross_thread_withdrawal_is_seen() {
    use std::io::Read;
    let probe = Probe::build();
    let (mut supervisor, _owner, reservation) = ready_probe(&probe, 1);
    let one = custody(&mut supervisor, &reservation);
    let registered = [one.clone()];
    std::thread::scope(|scope| {
        let (ready, wait_ready) = std::sync::mpsc::sync_channel(0);
        let (withdrawn, wait_withdrawn) = std::sync::mpsc::sync_channel(0);
        let registered = &registered;
        let one = &one;
        let worker = scope.spawn(move || {
            let mut dispatch = CustodyDispatch::observe(registered, TEST_AGE, 2).unwrap();
            assert_eq!(dispatch.check(one), Ok(true));
            ready.send(()).unwrap();
            wait_withdrawn.recv_timeout(Duration::from_secs(2)).unwrap();
            assert_eq!(dispatch.check(one), Ok(false));
        });
        wait_ready.recv_timeout(Duration::from_secs(2)).unwrap();
        supervisor.request_termination().unwrap();
        withdrawn.send(()).unwrap();
        worker.join().unwrap();
    });
    let mut old = CustodyDispatch::observe(&registered, TEST_AGE, 2).unwrap();
    supervisor.terminate().unwrap();
    let (mut owner, channel) = sophia_runtime::LaunchChannel::reserve(2).unwrap();
    let replacement = channel.reservation();
    supervisor.start_with_launch_channel(channel).unwrap();
    owner
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut ready = [0; 6];
    owner.read_exact(&mut ready).unwrap();
    assert_eq!(&ready, b"READY\n");
    let two = custody(&mut supervisor, &replacement);
    assert_eq!(old.check(&two), Err(CustodyDispatchLimit::Unregistered));
    assert_eq!(old.check(&one), Ok(false));
    let current = [two.clone()];
    let mut next = CustodyDispatch::observe(&current, TEST_AGE, 1).unwrap();
    assert_eq!(next.check(&two), Ok(true));
    supervisor.terminate().unwrap();
}
