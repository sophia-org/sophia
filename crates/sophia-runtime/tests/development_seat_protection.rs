//! Opt-in compatibility control for the development launcher's inherited
//! Landlock scopes and seccomp filter. The caller installs those restrictions
//! before this test starts. No PAM, Session, device or host-root operation.
use std::path::PathBuf;
use std::time::{Duration, Instant};

use sophia_runtime::{
    ProcessLaunchSpec, ProcessSupervisor, ProtectionDomainRole, ProtectionDomainSpec,
    ProtectionPath, SupervisedProcessKind, SupervisorCommand,
};

#[test]
#[ignore = "requires the development scope/filter entry and qualified private bubblewrap"]
fn an_ordinary_protected_child_runs_under_development_restrictions() {
    let bubblewrap = PathBuf::from(std::env::var_os("SOPHIA_TEST_BUBBLEWRAP").unwrap());
    assert!(bubblewrap.is_absolute());
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    assert!(status.lines().any(|line| line == "NoNewPrivs:\t1"));
    assert!(status.lines().any(|line| line == "Seccomp:\t2"));
    let evidence = std::env::temp_dir().join(format!(
        "sophia-development-protection-{}",
        std::process::id()
    ));
    // Exclusive creation: a stale run refuses rather than overwriting it.
    std::fs::create_dir(&evidence).unwrap();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::SpatialPolicy])
        .unwrap()
        .bubblewrap_path(bubblewrap)
        .path(ProtectionPath::read_write_at(&evidence, "/evidence"))
        .unwrap();
    let script = "set -eu; test ! -e /dev/dri; test ! -e /dev/input; \
                  test ! -e /run/dbus/system_bus_socket; \
                  printf protected > /evidence/result";
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::WindowManager,
        ProcessLaunchSpec::new("/usr/bin/sh")
            .arg("-c")
            .arg(script)
            .protection_domain(domain),
    );
    supervisor
        .apply(SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::WindowManager,
            delay: Duration::ZERO,
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while supervisor.poll().unwrap().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        supervisor.child_id().is_none(),
        "protected child was reaped"
    );
    assert_eq!(
        std::fs::read_to_string(evidence.join("result")).unwrap(),
        "protected"
    );
    std::fs::remove_dir_all(evidence).unwrap();
}
