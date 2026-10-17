//! Opt-in native broker process qualification. No desktop, display or device is
//! opened. The test uses real supervisor evidence and Session's owner/driver.
use sophia_protocol::{
    AttentionState, MetadataDisclosure, NamespaceProfile, SurfaceId,
    broker_files::{BrokerFileLimits, BrokerFileRequest as Request},
};
use sophia_runtime::{
    ProcessLaunchSpec, ProcessSupervisor, ProtectionDomainRole, ProtectionDomainSpec,
    ProtectionPath, SupervisedProcessKind, SupervisorCommand,
    broker_files::{BrokerFileListener, SOPHIA_BROKER_9P_SOCKET_ENV},
};
use sophia_session::{
    broker_file_driver::BrokerFileDriver,
    broker_file_owner::{BrokerCompletion, BrokerFileOwner},
};
use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[test]
#[ignore = "requires SOPHIA_TEST_EXECUTABLE and SOPHIA_TEST_BUBBLEWRAP absolute paths"]
fn protected_native_broker_preserves_all_operations_and_atomic_lowering() {
    let executable = PathBuf::from(std::env::var_os("SOPHIA_TEST_EXECUTABLE").unwrap());
    let bubblewrap = PathBuf::from(std::env::var_os("SOPHIA_TEST_BUBBLEWRAP").unwrap());
    assert!(executable.is_absolute() && bubblewrap.is_absolute());
    let directory = std::env::temp_dir().join(format!(
        "sophia-native-broker-process-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let listener =
        BrokerFileListener::bind(directory, rustix::process::geteuid().as_raw()).unwrap();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataBroker])
        .unwrap()
        .bubblewrap_path(bubblewrap)
        .path(ProtectionPath::read_only(
            listener.socket_path().parent().unwrap(),
        ))
        .unwrap();
    let spec = ProcessLaunchSpec::new(executable)
        .arg("metadata-broker-files-serve")
        .env(SOPHIA_BROKER_9P_SOCKET_ENV, listener.socket_path())
        .env("SOPHIA_BROKER_DEFAULT_DISCLOSURE", "class-only")
        .process_group()
        .protection_domain(domain);
    let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::MetadataBroker, spec);
    supervisor
        .apply(SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::MetadataBroker,
            delay: Duration::ZERO,
        })
        .unwrap();
    let evidence = supervisor.protection_evidence().unwrap().clone();
    let service = listener
        .accept(
            BrokerFileOwner::new(1).unwrap(),
            &evidence,
            1,
            1,
            0,
            BrokerFileLimits::default(),
            Duration::from_secs(3),
        )
        .unwrap();
    let mut driver = BrokerFileDriver::new(service);
    let surface = SurfaceId::new(3, 4);
    let timeout = Duration::from_secs(3);
    let admission = Request::SurfaceAdmitted {
        surface,
        profile: NamespaceProfile::Confined,
    };
    assert!(matches!(driver.request(admission,42,timeout).unwrap(),
        BrokerCompletion::Applied {request:1,rule:Some(rule),retired:None}
        if rule.disclosure==MetadataDisclosure::ClassOnly));
    let candidate = Request::CandidateReduced {
        surface,
        disclosure: MetadataDisclosure::ClassOnly,
        label: Some("Editor"),
        redacted: false,
        generation: 1,
    };
    driver.request(candidate, 42, timeout).unwrap();
    let grant = driver.owner().grants()[&surface];
    assert_eq!(
        driver
            .owner()
            .descriptors()
            .get(surface)
            .unwrap()
            .label
            .as_ref()
            .unwrap()
            .text,
        "Editor"
    );
    assert!(matches!(
        driver.request(candidate, 42, timeout).unwrap(),
        BrokerCompletion::Applied { .. }
    ));
    assert_eq!(driver.owner().grants()[&surface], grant);
    assert!(matches!(
        driver
            .request(
                Request::CandidateReduced {
                    surface,
                    disclosure: MetadataDisclosure::ClassOnly,
                    label: Some("Rejected"),
                    redacted: false,
                    generation: 0,
                },
                42,
                timeout
            )
            .unwrap(),
        BrokerCompletion::Rejected { reason: 2, .. }
    ));
    assert_eq!(
        driver
            .owner()
            .descriptors()
            .get(surface)
            .unwrap()
            .label
            .as_ref()
            .unwrap()
            .text,
        "Editor"
    );
    let attention = Request::AttentionChanged {
        surface,
        attention: AttentionState::Critical,
    };
    driver.request(attention, 42, timeout).unwrap();
    assert_eq!(
        driver.owner().descriptors().get(surface).unwrap().attention,
        AttentionState::Critical
    );
    assert!(matches!(
        driver.request(attention, 42, timeout).unwrap(),
        BrokerCompletion::NoChange { .. }
    ));
    driver
        .request(
            Request::SetDisclosure {
                surface,
                disclosure: MetadataDisclosure::Full,
            },
            42,
            timeout,
        )
        .unwrap();
    driver
        .request(
            Request::CandidateReduced {
                surface,
                disclosure: MetadataDisclosure::Full,
                label: Some("Private title"),
                redacted: false,
                generation: 2,
            },
            42,
            timeout,
        )
        .unwrap();
    assert_eq!(driver.owner().grants()[&surface].token, grant.token);
    // Readmission's default policy lowers Full to ClassOnly and emits both rows.
    driver.request(admission, 42, timeout).unwrap();
    assert!(
        driver
            .owner()
            .descriptors()
            .get(surface)
            .unwrap()
            .label
            .is_none()
    );
    assert_eq!(
        driver.owner().rule(surface).unwrap().disclosure,
        MetadataDisclosure::ClassOnly
    );
    assert_eq!(driver.owner().grants()[&surface].target_generation, 2);
    driver
        .request(
            Request::CandidateReduced {
                surface,
                disclosure: MetadataDisclosure::ClassOnly,
                label: Some("Editor"),
                redacted: false,
                generation: 3,
            },
            42,
            timeout,
        )
        .unwrap();
    // An older candidate must not replace the descriptor or its current grant.
    let current_grant = driver.owner().grants()[&surface];
    assert!(matches!(
        driver.request(candidate, 42, timeout).unwrap(),
        BrokerCompletion::Rejected { reason: 2, .. }
    ));
    assert_eq!(driver.owner().grants()[&surface], current_grant);
    assert_eq!(
        driver
            .owner()
            .descriptors()
            .get(surface)
            .unwrap()
            .generation,
        3
    );
    driver
        .request(
            Request::SetDisclosure {
                surface,
                disclosure: MetadataDisclosure::None,
            },
            42,
            timeout,
        )
        .unwrap();
    assert!(
        driver
            .owner()
            .descriptors()
            .get(surface)
            .unwrap()
            .label
            .is_none()
    );
    assert_eq!(
        driver.owner().rule(surface).unwrap().disclosure,
        MetadataDisclosure::None
    );
    assert_eq!(driver.owner().grants()[&surface].target_generation, 3);
    assert!(
        matches!(driver.request(Request::SurfaceRemoved {surface},42,timeout).unwrap(),
        BrokerCompletion::Applied {retired:Some(retired),..} if retired==surface)
    );
    assert!(driver.owner().descriptors().is_empty());
    assert!(driver.owner().grants().is_empty());
    assert!(driver.owner().route(surface).is_none());
    assert!(driver.owner().retired(surface));
    // Explicit supervisor shutdown owns process termination; no reconnect is allowed.
    supervisor.terminate().unwrap();
    driver.close().unwrap();
}
