//! Real shared-core export with independent SDK executables. The socketpair's
//! peer is this fixture, not a protected child: protection evidence is synthetic
//! here. Protected launch and foreign-process admission need separate gates.
use sophia_9p::{Errno, connection::ConnectionId, export::PeerCredentials, unix::Server};
use sophia_protocol::{MetadataDisclosure, SurfaceId, broker_files::*};
use sophia_runtime::{
    ProtectionBackendKind, ProtectionDomainEvidence, ProtectionDomainRole, broker_files::*,
};
use std::{
    os::{fd::OwnedFd, unix::net::UnixStream},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
struct Owner {
    expected: usize,
    commits: usize,
}
impl BrokerFileEffects for Owner {
    type Prepared = usize;
    fn prepare(&mut self, set: &BrokerFileResponseSet<'_>) -> Result<usize, Errno> {
        assert_eq!(set.request, 9);
        let rows: Vec<_> = set.rows().collect();
        assert_eq!(rows.len(), self.expected);
        if self.expected == 1 {
            assert_eq!(rows[0].kind, 3);
        } else {
            assert_eq!((rows[0].kind, rows[1].kind), (1, 2));
            assert_eq!(rows[1].grant_token, 12);
        }
        Ok(rows.len())
    }
    fn commit(&mut self, rows: usize) {
        assert_eq!(self.commits, 0);
        self.commits = rows;
    }
}
#[test]
#[ignore = "requires SOPHIA_BROKER_SDK_PEER pointing to an independently built SDK peer"]
fn external_sdk_broker_lifecycle() {
    let executable =
        std::path::PathBuf::from(std::env::var_os("SOPHIA_BROKER_SDK_PEER").expect("SDK peer"));
    assert!(executable.is_absolute() && executable.is_file());
    for expected in [1, 2] {
        let peer = PeerCredentials {
            pid: std::process::id() as i32,
            uid: rustix::process::geteuid().as_raw(),
            gid: rustix::process::getegid().as_raw(),
        };
        let evidence = ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: std::process::id(),
            peer_pid: std::process::id(),
            roles: [ProtectionDomainRole::MetadataBroker].into_iter().collect(),
        };
        let export = BrokerFileExport::new(
            Owner {
                expected,
                commits: 0,
            },
            ConnectionId(1),
            peer,
            &evidence,
            peer.uid,
            7,
            1,
            0,
            BrokerFileLimits::default(),
        )
        .unwrap();
        let mut server = Server::new(export, sophia_9p::Limits::default()).unwrap();
        let (owner, client) = UnixStream::pair().unwrap();
        assert_eq!(
            server.adopt(owner).unwrap_or_else(|_| panic!("adopt")),
            ConnectionId(1)
        );
        let mut child = Command::new(&executable)
            .env_remove(SOPHIA_BROKER_9P_SOCKET_ENV)
            .stdin(Stdio::from(OwnedFd::from(client)))
            .spawn()
            .unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        let mut published = false;
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "peer {expected}");
                break;
            }
            if Instant::now() >= end {
                let _ = child.kill();
                let _ = child.wait();
                panic!("peer timeout {expected}");
            }
            server.export_mut().expire(Instant::now());
            server.turn(Some(Duration::from_millis(1))).unwrap();
            if !published && server.export_mut().custody().negotiated() {
                let surface = SurfaceId::new(3, 4);
                let request = if expected == 1 {
                    BrokerFileRequest::SurfaceRemoved { surface }
                } else {
                    BrokerFileRequest::SetDisclosure {
                        surface,
                        disclosure: MetadataDisclosure::ClassOnly,
                    }
                };
                match server.export_mut().publish(9, request, Instant::now()) {
                    Ok(()) => published = true,
                    Err(Errno::EAGAIN) => (),
                    Err(e) => panic!("publish {e:?}"),
                }
            }
        }
        while server.connection_count() != 0 {
            assert!(Instant::now() < end);
            server.turn(Some(Duration::from_millis(1))).unwrap();
        }
        assert!(published);
        assert_eq!(server.export_mut().owner().commits, expected);
        assert!(server.export_mut().custody().closed());
    }
}

#[test]
#[ignore = "requires SOPHIA_BROKER_SDK_PEER and SOPHIA_TEST_BUBBLEWRAP absolute paths"]
fn external_sdk_protected_broker_lifecycle() {
    use sophia_runtime::{
        ProcessLaunchSpec, ProcessSupervisor, ProtectionDomainSpec, ProtectionPath,
        SupervisedProcessKind, SupervisorCommand,
    };
    use std::time::{SystemTime, UNIX_EPOCH};
    let executable =
        std::path::PathBuf::from(std::env::var_os("SOPHIA_BROKER_SDK_PEER").expect("SDK peer"));
    let bubblewrap =
        std::path::PathBuf::from(std::env::var_os("SOPHIA_TEST_BUBBLEWRAP").expect("bubblewrap"));
    assert!(executable.is_absolute() && executable.is_file());
    assert!(bubblewrap.is_absolute() && bubblewrap.is_file());
    for expected in [1, 2] {
        let directory = std::env::temp_dir().join(format!(
            "sophia-broker-sdk-protected-{}-{}",
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
            .bubblewrap_path(&bubblewrap)
            .path(ProtectionPath::read_only(
                listener.socket_path().parent().unwrap(),
            ))
            .unwrap();
        let spec = ProcessLaunchSpec::new(&executable)
            .env(SOPHIA_BROKER_9P_SOCKET_ENV, listener.socket_path())
            .process_group()
            .protection_domain(domain);
        let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::MetadataBroker, spec);
        supervisor
            .apply(SupervisorCommand::StartProcess {
                process: SupervisedProcessKind::MetadataBroker,
                delay: Duration::ZERO,
            })
            .unwrap();
        let evidence = supervisor
            .protection_evidence()
            .expect("real protection evidence")
            .clone();
        assert_ne!(Some(evidence.peer_pid), supervisor.child_id());
        let mut service = listener
            .accept(
                Owner {
                    expected,
                    commits: 0,
                },
                &evidence,
                7,
                1,
                0,
                BrokerFileLimits::default(),
                Duration::from_secs(3),
            )
            .unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        let mut published = false;
        while supervisor.poll().unwrap().is_none() {
            assert!(Instant::now() < end, "protected SDK peer timed out");
            service.turn(Duration::from_millis(10)).unwrap();
            if !published && service.export().custody().negotiated() {
                let surface = SurfaceId::new(3, 4);
                let request = if expected == 1 {
                    BrokerFileRequest::SurfaceRemoved { surface }
                } else {
                    BrokerFileRequest::SetDisclosure {
                        surface,
                        disclosure: MetadataDisclosure::ClassOnly,
                    }
                };
                match service.export_mut().publish(9, request, Instant::now()) {
                    Ok(()) => published = true,
                    Err(Errno::EAGAIN) => (),
                    Err(error) => panic!("protected publish {error:?}"),
                }
            }
        }
        assert!(supervisor.exit_status().unwrap().success());
        service.close().unwrap();
        assert!(published);
        assert_eq!(service.export().owner().commits, expected);
        assert!(service.export().custody().closed());
    }
}
