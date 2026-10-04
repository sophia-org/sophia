//! The Nix store binding and the configured Bubblewrap executable. These drive
//! the argument builder with the store present and absent, independent of
//! the host; the real read-only control is the opt-in smoke in
//! tests/supervisor/supervisor_process.rs.
#![cfg(test)]
use super::*;

const STORE: &str = "/nix/store";

fn shell_domain() -> ProtectionDomainSpec {
    ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell]).unwrap()
}

fn arguments(
    domain: &ProtectionDomainSpec,
    program: &str,
    store: Option<&str>,
) -> Result<Vec<String>, ProtectionDomainLaunchError> {
    let launch = ProcessLaunchSpec::new(program);
    bubblewrap_arguments(&launch, domain, Path::new(program), store.map(Path::new)).map(|args| {
        args.into_iter()
            .map(|arg| arg.into_string().unwrap())
            .collect()
    })
}

fn position(args: &[String], window: &[&str]) -> Option<usize> {
    args.windows(window.len())
        .position(|candidate| candidate == window)
}

#[test]
fn an_absent_store_leaves_the_domain_exactly_as_before() {
    let domain = shell_domain()
        .path(ProtectionPath::read_only("/run/sophia-test"))
        .unwrap();
    let args = arguments(&domain, "/usr/bin/true", None).unwrap();
    assert!(!args.iter().any(|arg| arg.starts_with("/nix")));
    // Without a store, a grant beneath /nix is an ordinary grant.
    let nix = shell_domain()
        .path(ProtectionPath::read_only("/nix/store/fixture"))
        .unwrap();
    assert!(arguments(&nix, "/usr/bin/true", None).is_ok());
}

#[test]
fn a_present_store_is_bound_read_only_before_any_grant() {
    let domain = shell_domain()
        .path(ProtectionPath::read_write("/run/sophia-test"))
        .unwrap();
    let args = arguments(&domain, "/usr/bin/true", Some(STORE)).unwrap();
    let store = position(&args, &["--ro-bind", STORE, STORE]).expect("store binding");
    assert_eq!(
        position(&args, &["--dir", "/nix", "--dir", STORE]),
        Some(store - 4)
    );
    // Nothing else names the store, and it follows the fixed layout.
    assert_eq!(args.iter().filter(|arg| arg.as_str() == STORE).count(), 3);
    assert!(store > position(&args, &["--dir", "/home"]).unwrap());
    assert!(store < position(&args, &["--bind", "/run/sophia-test", "/run/sophia-test"]).unwrap());
    // Removing the store's seven arguments (two directories and the binding)
    // leaves exactly the store-absent list.
    let mut without = args.clone();
    without.drain(store - 4..store + 3);
    assert_eq!(without, arguments(&domain, "/usr/bin/true", None).unwrap());
}

#[test]
fn no_grant_may_replace_or_shadow_a_present_store() {
    for destination in [
        "/nix",
        STORE,
        "/nix/store/fixture",
        "/nix/store/fixture/bin",
    ] {
        for path in [
            ProtectionPath::read_only_at("/run/sophia-test", destination),
            ProtectionPath::read_write_at("/run/sophia-test", destination),
        ] {
            let domain = shell_domain().path(path).unwrap();
            assert_eq!(
                arguments(&domain, "/usr/bin/true", Some(STORE)),
                Err(ProtectionDomainLaunchError::InvalidBinding(PathBuf::from(
                    destination
                ))),
                "{destination}"
            );
        }
    }
    // A neighbour that only shares a prefix of the name is not the store.
    let neighbour = shell_domain()
        .path(ProtectionPath::read_write("/nixos"))
        .unwrap();
    assert!(arguments(&neighbour, "/usr/bin/true", Some(STORE)).is_ok());
}

#[test]
fn a_program_in_the_store_runs_from_the_store_binding() {
    let program = "/nix/store/0000-fixture/bin/fixture";
    let args = arguments(&shell_domain(), program, Some(STORE)).unwrap();
    assert_eq!(args.iter().filter(|arg| arg.as_str() == program).count(), 1);
    assert_eq!(args[args.len() - 1], program);
    // Outside /usr and the store, the program is still bound on its own.
    let program = "/opt/fixture/bin/fixture";
    let args = arguments(&shell_domain(), program, Some(STORE)).unwrap();
    assert!(position(&args, &["--ro-bind", program, program]).is_some());
}

#[test]
fn the_configured_bubblewrap_must_be_an_absolute_executable_file() {
    use std::os::unix::fs::PermissionsExt as _;
    let program = std::env::current_exe().unwrap();
    let directory =
        std::env::temp_dir().join(format!("sophia-bubblewrap-config-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir(&directory).unwrap();
    let plain = directory.join("not-executable");
    std::fs::write(&plain, b"#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();
    for bubblewrap in [
        PathBuf::from("bwrap"),
        PathBuf::from("usr/bin/bwrap"),
        directory.clone(),
        plain,
        directory.join("missing"),
    ] {
        let launch = ProcessLaunchSpec::new(&program)
            .protection_domain(shell_domain().bubblewrap_path(&bubblewrap));
        assert_eq!(
            spawn_bubblewrap(
                SupervisedProcessKind::Shell,
                &launch,
                launch.protection_domain.as_ref().unwrap(),
            )
            .err(),
            Some(ProtectionDomainLaunchError::InvalidBubblewrap(bubblewrap))
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
    assert_eq!(
        shell_domain().bubblewrap_executable(),
        Path::new(DEFAULT_BUBBLEWRAP_PATH)
    );
}
