//! The extracted Bubblewrap command and the exclusive application role. The
//! argument builder is private, so these sit beside it: an existing role's
//! command must be exactly the argument list it always was, with nothing
//! added to the host environment Bubblewrap itself inherits, and no
//! supervised process may be launched into an application domain.
#![cfg(test)]
use super::*;

const EXISTING_ROLES: [ProtectionDomainRole; 7] = [
    ProtectionDomainRole::SpatialPolicy,
    ProtectionDomainRole::MetadataShell,
    ProtectionDomainRole::MetadataBroker,
    ProtectionDomainRole::PortalBroker,
    ProtectionDomainRole::ApplicationFrontend,
    ProtectionDomainRole::OutputAuthority,
    ProtectionDomainRole::LockProvider,
];

const SUPERVISED: [SupervisedProcessKind; 7] = [
    SupervisedProcessKind::WindowManager,
    SupervisedProcessKind::PortalBroker,
    SupervisedProcessKind::MetadataBroker,
    SupervisedProcessKind::Shell,
    SupervisedProcessKind::OutputAuthority,
    SupervisedProcessKind::LockProvider,
    SupervisedProcessKind::SophiaXAuthority,
];

#[test]
fn every_existing_role_builds_exactly_its_argument_list() {
    assert!(
        Path::new(DEFAULT_BUBBLEWRAP_PATH).is_file(),
        "command compatibility proof requires Bubblewrap"
    );
    let program = Path::new("/usr/bin/true");
    let grant = std::env::current_exe().unwrap();
    let grant = grant.parent().unwrap();
    for role in EXISTING_ROLES {
        let domain = ProtectionDomainSpec::bubblewrap([role])
            .unwrap()
            .path(ProtectionPath::read_only(grant))
            .unwrap();
        let launch = ProcessLaunchSpec::new(program)
            .arg("--fixture")
            .env("SOPHIA_FIXTURE", "1");
        let command = bubblewrap_command(&launch, &domain, |_| {}).unwrap();
        let expected = bubblewrap_arguments(&launch, &domain, program, visible_store()).unwrap();
        assert_eq!(command.get_program(), Path::new(DEFAULT_BUBBLEWRAP_PATH));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            expected.iter().map(OsString::as_os_str).collect::<Vec<_>>(),
            "{role:?}"
        );
        // The launch environment reaches the domain only through --setenv;
        // Bubblewrap's own environment is the host's, untouched.
        assert_eq!(command.get_envs().count(), 0, "{role:?}");
        assert_eq!(command.get_current_dir(), None, "{role:?}");
    }
}

#[test]
fn no_supervised_process_launches_into_an_application_domain() {
    let domain =
        ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::ConfinedXApplication]).unwrap();
    // A program that does not exist: the role is refused before the program
    // is even looked at, as it was before the command was extracted.
    let launch = ProcessLaunchSpec::new("/nonexistent-sophia-application");
    for process in SUPERVISED {
        match spawn_bubblewrap(process, &launch, &domain) {
            Err(ProtectionDomainLaunchError::RoleMismatch {
                process: refused, ..
            }) => {
                assert_eq!(refused, process);
            }
            Err(other) => panic!("{process:?} reached {other:?} past the role check"),
            Ok(_) => panic!("{process:?} launched into an application domain"),
        }
    }
}

#[test]
fn the_application_role_composes_with_nothing() {
    for other in EXISTING_ROLES {
        assert_eq!(
            ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::ConfinedXApplication, other]),
            Err(ProtectionDomainSpecError::ExclusiveRole {
                exclusive: ProtectionDomainRole::ConfinedXApplication,
                conflicting: other,
            })
        );
    }
    assert!(ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::ConfinedXApplication]).is_ok());
}
