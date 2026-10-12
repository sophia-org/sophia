//! Launch-owner scheduling and cancellation, with no display or native device.
#![cfg(test)]
use super::*;
use crate::live_session::tests::lock_provider_factotum as factotum;
use sophia_runtime::{
    ProcessLaunchSpec, ProtectionDomainRole, ProtectionDomainSpec, SupervisedProcessKind,
};

fn startup(launches: &LaunchCoordinator, delay: Duration, now: Instant) -> Startup {
    let supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::WindowManager,
        ProcessLaunchSpec::new("/usr/bin/sleep")
            .arg("20")
            .protection_domain(
                ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::SpatialPolicy]).unwrap(),
            ),
    );
    Startup::new(
        supervisor,
        SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::WindowManager,
            delay,
        },
        launches.endpoint(),
        Recipe {
            policy: LaunchPolicy {
                principal: 3,
                namespace: 2,
                audience: 3,
                role: ProtectionDomainRole::SpatialPolicy,
            },
            uname: b"wm".to_vec(),
            aname: b"spatial-policy".to_vec(),
        },
        now,
    )
    .unwrap()
}

#[test]
fn unavailable_factotum_never_spawns_and_has_a_bounded_startup_deadline() {
    let mut launches = LaunchCoordinator::new(Default::default(), factotum::namespaces());
    let wake = sophia_wake::Wake::new().unwrap();
    let now = Instant::now();
    let mut startup = startup(&launches, Duration::ZERO, now);
    assert!(
        startup
            .poll(&mut launches, wake.notifier(), now)
            .unwrap()
            .is_none()
    );
    assert!(startup.supervisor.as_ref().unwrap().child_id().is_none());
    assert!(startup.registration.is_none());
    let error = startup
        .poll(
            &mut launches,
            wake.notifier(),
            now + Duration::from_secs(10),
        )
        .err()
        .unwrap();
    assert_eq!(error, "authenticated role launch deadline");
    assert!(startup.supervisor.is_none() && startup.spawning.is_none());
}

#[test]
fn restart_backoff_and_cancellation_do_not_spend_a_launch_early() {
    let mut auth = factotum::Fixture::new();
    let wake = sophia_wake::Wake::new().unwrap();
    let now = Instant::now();
    let mut startup = startup(&auth.coordinator, Duration::from_secs(1), now);
    assert!(
        startup
            .poll(&mut auth.coordinator, wake.notifier(), now)
            .unwrap()
            .is_none()
    );
    assert!(
        startup.registration.is_none(),
        "restart backoff must precede launch registration"
    );
    startup.cancel(
        &mut auth.coordinator,
        now,
        "cancelled during backoff".into(),
    );
    assert_eq!(
        startup
            .poll(&mut auth.coordinator, wake.notifier(), now)
            .err()
            .unwrap(),
        "cancelled during backoff"
    );
    assert!(startup.supervisor.is_none());
    assert!(auth.coordinator.ready());
}

#[test]
fn cancellation_reclaims_the_outstanding_spawn_before_replacement() {
    let mut auth = factotum::Fixture::new();
    let wake = sophia_wake::Wake::new().unwrap();
    let mut startup = startup(&auth.coordinator, Duration::ZERO, Instant::now());
    let deadline = Instant::now() + Duration::from_secs(10);
    while startup.spawning.is_none() {
        assert!(Instant::now() < deadline, "protected spawn starts");
        auth.tick();
        assert!(
            startup
                .poll(&mut auth.coordinator, wake.notifier(), Instant::now())
                .unwrap()
                .is_none()
        );
    }
    let registration = startup.registration.unwrap();
    startup.cancel(
        &mut auth.coordinator,
        Instant::now(),
        "cancelled outstanding spawn".into(),
    );
    assert_eq!(auth.coordinator.phase(registration), Some(Phase::Retiring));
    loop {
        assert!(
            Instant::now() < deadline,
            "cancelled spawn returns ownership"
        );
        auth.tick();
        match startup.poll(&mut auth.coordinator, wake.notifier(), Instant::now()) {
            Ok(None) => {}
            Ok(Some(_)) => panic!("cancelled launch must never publish an authenticated worker"),
            Err(error) => {
                assert_eq!(error, "cancelled outstanding spawn");
                break;
            }
        }
    }
    assert!(
        startup.spawning.is_none() && startup.supervisor.is_none() && startup.lease.is_none(),
        "cancelled launch must reclaim its spawn before allowing replacement"
    );
}
