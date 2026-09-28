//! Descriptor admission against the production 9P export. Protection identity
//! is supplied for this process; no protected child or presentation is claimed.
use sophia_protocol::*;
use sophia_runtime::*;
use sophia_shell_client::{ShellClientOptions, ShellConnection};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;
use shell_file_peer::Peer;
use sophia_protocol::shell_files::*;

const WAIT: Duration = Duration::from_secs(3);
const EPOCH: u64 = 41;
const BASE: u64 = 3;
const METADATA: u64 =
    BASE | (1 << 2) | (1 << 3) | (1 << 4) | (1 << 5) | (1 << 6) | (1 << 9) | (1 << 10);

#[path = "support/shell_file_descriptor_owner.rs"]
mod owner;

#[path = "support/shell_file_tabs_owner.rs"]
mod tabs;

struct Fixture {
    transport: ShellComponentTransport,
    epochs: ContentEpochRegistry,
    path: std::path::PathBuf,
}
impl Fixture {
    fn refuse(&mut self, revision: u16, required_capabilities: u64) -> ShellTransportError {
        let socket = self.transport.socket_path().to_owned();
        let peer = std::thread::spawn(move || {
            ShellConnection::connect_files(
                &socket,
                ShellClientOptions {
                    minimum_revision: revision,
                    maximum_revision: revision,
                    required_capabilities,
                    handshake_timeout: WAIT,
                },
            )
            .is_err()
        });
        let deadline = Instant::now() + WAIT;
        let error = loop {
            match self.transport.poll_negotiation(&mut self.epochs, 65536) {
                Err(error) => break error,
                Ok(None) => {}
                Ok(Some(_)) => panic!("invalid grant admitted"),
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        assert!(peer.join().unwrap());
        assert!(self.epochs.accounting().quiescent());
        error
    }

    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "descriptor-negotiation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut transport = ShellComponentTransport::bind_for_supervised_uid(
            &path,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        transport
            .authorize_protected_peer(&ProtectionDomainEvidence {
                backend: ProtectionBackendKind::Bubblewrap,
                supervisor_pid: std::process::id(),
                peer_pid: std::process::id(),
                roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
            })
            .unwrap();
        Self {
            transport,
            epochs: ContentEpochRegistry::new(64 * 1024 * 1024).unwrap(),
            path,
        }
    }

    fn start(&mut self, policy: ShellContentAdmissionPolicy) {
        self.transport
            .begin_descriptor_file_negotiation(&self.epochs, EPOCH, WAIT, policy)
            .unwrap();
    }

    fn connect(&mut self, revision: u16, caps: u64) -> ShellConnection {
        let socket = self.transport.socket_path().to_owned();
        let peer = std::thread::spawn(move || {
            ShellConnection::connect_files(
                &socket,
                ShellClientOptions {
                    minimum_revision: revision,
                    maximum_revision: revision,
                    required_capabilities: caps & !2,
                    handshake_timeout: WAIT,
                },
            )
            .unwrap()
        });
        let deadline = Instant::now() + WAIT;
        let welcome = loop {
            if let Some(welcome) = self
                .transport
                .poll_negotiation(&mut self.epochs, 65536)
                .unwrap()
            {
                break welcome;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        assert_eq!(welcome.selected_revision, revision);
        assert_eq!(welcome.connection_epoch, EPOCH);
        assert_eq!(welcome.capabilities, caps);
        while !peer.is_finished() {
            self.transport.poll_io(&mut self.epochs).unwrap();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        peer.join().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.transport.disconnect(&mut self.epochs).unwrap();
        assert!(
            self.transport
                .collect_content_accounting(&mut self.epochs)
                .quiescent()
        );
        std::fs::remove_dir_all(&self.path).unwrap();
    }
}

#[test]
fn metadata_only_sdk_bootstrap_accepts_each_revision_without_content() {
    for revision in 1..=8 {
        let mut f = Fixture::new();
        f.start(ShellContentAdmissionPolicy::Unavailable);
        let _client = f.connect(revision, BASE);
        assert!(f.transport.content_limits().is_none());
        assert!(f.epochs.accounting().quiescent());
    }
}

#[test]
fn descriptor_families_are_selected_only_when_requested() {
    for caps in [
        BASE | 4,
        BASE | 8,
        BASE | (1 << 5),
        BASE | (1 << 9),
        METADATA,
    ] {
        let mut f = Fixture::new();
        f.start(ShellContentAdmissionPolicy::Denied);
        let _client = f.connect(8, caps);
        assert!(f.transport.content_limits().is_none());
    }
}

#[test]
fn combined_descriptor_content_reuses_the_exact_reserved_grant() {
    let mut f = Fixture::new();
    let limits = ContentLimits::prototype(ContentGrant {
        connection_epoch: EPOCH,
        content_grant_epoch: 7,
    });
    f.transport
        .reserve_content(&mut f.epochs, limits.clone())
        .unwrap();
    f.start(ShellContentAdmissionPolicy::Granted {
        discrete_input: true,
    });
    let _client = f.connect(8, METADATA | (1 << 7) | (1 << 8));
    assert_eq!(f.transport.content_limits(), Some(&limits));
    let accounting = f.transport.content_accounting(&f.epochs);
    assert_eq!(accounting.snapshot_reserved_bytes, 15_015_936 + 2048);
    assert_eq!(accounting.snapshot_retained_bytes, 0);
    assert_eq!(
        accounting.epochs.reserved_bytes,
        limits.max_staging_bytes + limits.max_resident_bytes + limits.max_retiring_bytes
    );
}

#[test]
fn descriptor_snapshot_bound_follows_only_the_disclosed_feeds() {
    for (caps, bytes) in [
        (BASE, 4_202_496),
        (BASE | 4, 6_299_648),
        (BASE | 8, 4_464_640),
        (BASE | 4 | 8, 6_561_792),
        (BASE | 4 | 8 | (1 << 5), 14_950_400),
        (BASE | 4 | 8 | (1 << 9), 6_627_328),
        (METADATA, 15_015_936),
    ] {
        let mut f = Fixture::new();
        f.start(ShellContentAdmissionPolicy::Unavailable);
        assert_eq!(
            f.transport
                .content_accounting(&f.epochs)
                .snapshot_reserved_bytes,
            0
        );
        let _client = f.connect(8, caps);
        let accounting = f.transport.content_accounting(&f.epochs);
        assert_eq!(accounting.snapshot_reserved_bytes, bytes, "caps={caps}");
        assert_eq!(accounting.snapshot_retained_bytes, 0);
        assert!(accounting.epochs.quiescent());
        assert!(
            !accounting.quiescent(),
            "the file export still owns snapshot capacity"
        );
        f.transport.disconnect(&mut f.epochs).unwrap();
        assert!(f.transport.content_accounting(&f.epochs).quiescent());
    }
}

#[test]
fn descriptor_admission_cannot_borrow_other_role_stores() {
    for profile in [
        ContentStoreProfile::NativeLauncher,
        ContentStoreProfile::PersistentCatalog,
    ] {
        let mut f = Fixture::new();
        let limits = ContentLimits::prototype(ContentGrant {
            connection_epoch: EPOCH,
            content_grant_epoch: 7,
        });
        f.transport
            .reserve_content_with_profile(&mut f.epochs, limits, profile)
            .unwrap();
        assert_eq!(
            f.transport.begin_descriptor_file_negotiation(
                &f.epochs,
                EPOCH,
                WAIT,
                ShellContentAdmissionPolicy::Granted {
                    discrete_input: true
                }
            ),
            Err(ShellTransportError::WrongContentGrant)
        );
    }
}

#[test]
fn descriptor_negotiation_refuses_missing_prerequisites_and_foreign_profiles() {
    for (revision, required) in [
        (8, 0),
        (1, 1 | 4),
        (8, 1 | 16),
        (8, 1 | 64),
        (8, 1 | 256),
        (8, 1 | 1024),
        (8, 1 | 2048),
        (8, 1 | 4096),
    ] {
        let mut f = Fixture::new();
        f.start(ShellContentAdmissionPolicy::Granted {
            discrete_input: true,
        });
        assert_eq!(
            f.refuse(revision, required),
            ShellTransportError::MissingCapability
        );
    }
}

#[test]
fn denied_combined_content_is_refused_without_allocating_a_registry_grant() {
    let mut f = Fixture::new();
    f.start(ShellContentAdmissionPolicy::Denied);
    assert!(matches!(
        f.refuse(8, 1 | (1 << 7)),
        ShellTransportError::ContentAdmissionRefused(ContentAdmissionRefused { reason: 1, .. })
    ));
}

fn lookup(peer: &mut Peer, fid: u32, name: &[u8]) -> u8 {
    let body = [
        1u32.to_le_bytes().as_slice(),
        &fid.to_le_bytes(),
        &1u16.to_le_bytes(),
        &(name.len() as u16).to_le_bytes(),
        name,
    ]
    .concat();
    peer.rpc(110, &body).unwrap().0
}

#[test]
fn metadata_export_hides_content_names_before_and_after_negotiation() {
    let mut f = Fixture::new();
    f.start(ShellContentAdmissionPolicy::Unavailable);
    let path = f.transport.socket_path().to_owned();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&path);
        peer.setup();
        peer.open(8, b"api", 0);
        assert_eq!(
            peer.read(8, 0),
            format!(
                "sophia-shell-files version=1 role=descriptor epoch={EPOCH} fd_transfer=none\n"
            )
            .into_bytes()
        );
        for (i, name) in [
            b"limits".as_slice(),
            b"outputs",
            b"upload",
            b"descriptors",
            b"tabs",
            b"shortcuts",
            b"catalog",
            b"indicators",
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                lookup(&mut peer, 20 + i as u32, name),
                7,
                "pre-negotiation {name:?}"
            );
        }
        let offer = encode_shell_file_negotiate(
            ShellFileHeader {
                kind: ShellFileKind::Negotiate,
                connection_epoch: EPOCH,
                submission_id: 1,
                sequence: 0,
            },
            ShellV1ClientHello {
                minimum_revision: 8,
                maximum_revision: 8,
                required_capabilities: METADATA & !2,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        assert!(
            !decode_shell_file_negotiated(&negotiated)
                .unwrap()
                .limits_published
        );
        peer.ack(&negotiated);
        for (i, name) in [b"limits".as_slice(), b"outputs", b"upload"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(lookup(&mut peer, 40 + i as u32, name), 7);
        }
        for (i, name) in [
            b"descriptors".as_slice(),
            b"tabs",
            b"shortcuts",
            b"catalog",
            b"indicators",
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(lookup(&mut peer, 50 + i as u32, name), 111);
        }
    });
    let end = Instant::now() + WAIT;
    let mut negotiated = false;
    while !peer.is_finished() {
        if negotiated {
            match f.transport.poll_io(&mut f.epochs) {
                Ok(()) | Err(ShellTransportError::NotConnected) => {}
                Err(error) => panic!("{error:?}"),
            }
        } else {
            negotiated = f
                .transport
                .poll_negotiation(&mut f.epochs, 65536)
                .unwrap()
                .is_some();
        }
        assert!(Instant::now() < end);
        std::thread::yield_now();
    }
    peer.join().unwrap();
    assert!(negotiated);
}
