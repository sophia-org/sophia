use super::*;
use std::ffi::OsStr;

#[test]
fn shell_work_statistics_forward_only_the_exact_opt_in() {
    let spec = ProcessLaunchSpec::new("/usr/bin/true")
        .arg("--serve")
        .env("SOPHIA_SHELL_9P_SOCKET", "/private/shell.sock")
        .protection_domain(
            ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell]).unwrap(),
        );
    for value in [None, Some(""), Some("0"), Some("true"), Some("01"), Some("1 ")] {
        assert_eq!(
            with_work_statistics(spec.clone(), value.map(OsStr::new)),
            spec,
        );
    }
    assert_eq!(
        with_work_statistics(spec.clone(), Some(OsStr::new("1"))),
        spec.env(WORK_STATS_ENV, "1"),
    );
}
