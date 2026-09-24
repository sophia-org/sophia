#![cfg(test)]

use super::{
    TERMINATION_TIMEOUT, UNIX_SOCKET_PATH_MAX_BYTES, WorkloadSubreaper, direct_children,
    kitty_socket_path, terminate_owned_processes,
};
use crate::desktop_comparison::capture_owner::ProcStat;
use std::collections::BTreeMap;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[test]
fn runtime_socket_path_stays_within_the_linux_limit() {
    let namespace = Path::new("/run/user/4294967295/sophia-desktop-comparison/workload-4294967295");
    let socket = kitty_socket_path(namespace, 15).expect("comparison socket should fit");
    assert!(socket.as_os_str().as_bytes().len() <= UNIX_SOCKET_PATH_MAX_BYTES);
}

#[test]
fn excessive_socket_path_is_refused_before_kitty_launch() {
    let namespace = PathBuf::from("/tmp").join("x".repeat(UNIX_SOCKET_PATH_MAX_BYTES));
    let error = kitty_socket_path(&namespace, 0).expect_err("long socket must be refused");
    assert!(error.contains("Linux permits at most 107"));
}

#[test]
fn direct_child_ownership_excludes_only_the_same_preexisting_identity() {
    let stat = |ppid, start_ticks| ProcStat {
        ppid,
        start_ticks,
        cpu_ticks: 0,
        minor_faults: 0,
        major_faults: 0,
        threads: 1,
    };
    let processes = BTreeMap::from([(10, stat(4, 100)), (11, stat(4, 110)), (12, stat(3, 120))]);
    let excluded = BTreeMap::from([(10, 100), (11, 109)]);

    assert_eq!(
        direct_children(&processes, 4, &excluded),
        BTreeMap::from([(11, 110)])
    );
}

#[test]
fn subreaper_contains_and_terminates_an_orphaned_workload_child() {
    const CHILD_ENV: &str = "SOPHIA_SUBREAPER_REGRESSION_CHILD";
    if let Some(marker) = std::env::var_os(CHILD_ENV) {
        let subreaper = WorkloadSubreaper::arm(Path::new("/proc"))
            .expect("test process should become a child subreaper");
        let status = Command::new("sh")
            .args(["-c", "sleep 30 &"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("test should launch an orphaning workload");
        assert!(status.success());

        let deadline = Instant::now() + Duration::from_secs(2);
        let adopted = loop {
            let adopted = subreaper
                .adopted_processes()
                .expect("test should inspect adopted workload processes");
            if !adopted.is_empty() {
                break adopted;
            }
            assert!(
                Instant::now() < deadline,
                "orphaned workload child was not adopted"
            );
            thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(adopted.len(), 1);
        terminate_owned_processes(&mut [], &BTreeMap::new(), &subreaper)
            .expect("adopted workload child should terminate within the bound");
        assert!(
            subreaper
                .adopted_processes()
                .expect("test should verify subreaper drain")
                .is_empty()
        );
        std::fs::write(marker, b"passed\n").expect("child test should publish completion");
        return;
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow epoch")
        .as_nanos();
    let marker = std::env::temp_dir().join(format!(
        "sophia-subreaper-regression-{}-{nonce}",
        std::process::id()
    ));
    let module = module_path!()
        .strip_prefix("sophia_conformance::")
        .unwrap_or(module_path!());
    let test = format!("{module}::subreaper_contains_and_terminates_an_orphaned_workload_child");
    let status = Command::new(std::env::current_exe().expect("test executable should resolve"))
        .args(["--exact", &test])
        .env(CHILD_ENV, &marker)
        .status()
        .expect("isolated subreaper regression should run");
    assert!(status.success());
    assert_eq!(
        std::fs::read(&marker).expect("isolated regression should publish completion"),
        b"passed\n"
    );
    std::fs::remove_file(marker).expect("subreaper test marker should be removed");
    assert_eq!(TERMINATION_TIMEOUT, Duration::from_secs(2));
}
