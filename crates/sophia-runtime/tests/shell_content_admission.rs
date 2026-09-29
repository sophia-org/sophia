//! Admission through the production 9P export with supplied protection evidence.
//! The peer checks actual refusal and limits records; no protected child runs.
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;
use shell_file_peer::Peer;
const WAIT: Duration = Duration::from_secs(3);
const CONTENT: u64 = SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
const INPUT: u64 = SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT;
struct Fixture {
    transport: ShellComponentTransport,
    epochs: ContentEpochRegistry,
    path: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "content-admission-files-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let transport = ShellComponentTransport::bind_for_supervised_uid(
            &path,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        Self {
            transport,
            epochs: ContentEpochRegistry::new(64 * 1024 * 1024).unwrap(),
            path,
        }
    }
    fn start(&mut self, epoch: u64, policy: ShellContentAdmissionPolicy) {
        self.transport
            .authorize_protected_peer(&ProtectionDomainEvidence {
                backend: ProtectionBackendKind::Bubblewrap,
                supervisor_pid: std::process::id(),
                peer_pid: std::process::id(),
                roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
            })
            .unwrap();
        self.transport
            .begin_descriptor_file_negotiation(&self.epochs, epoch, WAIT, policy)
            .unwrap();
    }
    fn welcome(&mut self) -> Result<ShellV1ServerWelcome, ShellTransportError> {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(value) = self.transport.poll_negotiation(&mut self.epochs, 65536)? {
                return Ok(value);
            }
            assert!(Instant::now() < deadline, "admission did not settle");
            std::thread::yield_now();
        }
    }
    fn refusal(&mut self, required: u64, expected: ContentAdmissionRefused) {
        let socket = self.transport.socket_path().to_owned();
        let peer_expected = expected.clone();
        let peer = std::thread::spawn(move || {
            let mut peer = Peer::connect(&socket);
            peer.setup();
            peer.submit_acknowledged(&offer(1, 6, required), 1);
            let event = peer.next_event();
            assert_eq!(decode_shell_file_refused(&event).unwrap(), peer_expected);
            peer.ack(&event);
        });
        assert_eq!(
            self.welcome(),
            Err(ShellTransportError::ContentAdmissionRefused(expected))
        );
        peer.join().unwrap();
        assert!(!self.transport.supports_content());
        assert!(self.transport.content_accounting(&self.epochs).quiescent());
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
fn offer(epoch: u64, revision: u16, required: u64) -> Vec<u8> {
    encode_shell_file_negotiate(
        ShellFileHeader {
            kind: ShellFileKind::Negotiate,
            connection_epoch: epoch,
            submission_id: 1,
            sequence: 0,
        },
        ShellV1ClientHello {
            minimum_revision: revision,
            maximum_revision: revision,
            required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | required,
        },
    )
    .unwrap()
}
#[test]
fn content_is_unavailable_without_an_explicit_service_policy() {
    let mut f = Fixture::new();
    f.start(1, ShellContentAdmissionPolicy::Unavailable);
    f.refusal(
        CONTENT,
        ContentAdmissionRefused {
            reason: 4,
            denied_capabilities: CONTENT,
        },
    );
}
#[test]
fn granted_content_gets_limits_and_a_fresh_epoch_on_replacement() {
    let mut f = Fixture::new();
    let mut prior = 0;
    for epoch in [1, 2] {
        f.start(
            epoch,
            ShellContentAdmissionPolicy::Granted {
                discrete_input: true,
            },
        );
        let socket = f.transport.socket_path().to_owned();
        let peer = std::thread::spawn(move || {
            let mut peer = Peer::connect(&socket);
            peer.setup();
            peer.submit_acknowledged(&offer(epoch, 6, CONTENT | INPUT), 1);
            let event = peer.next_event();
            let negotiated = decode_shell_file_negotiated(&event).unwrap();
            assert!(negotiated.limits_published);
            peer.ack(&event);
            peer.open(7, b"limits", 0);
            let mut bytes = Vec::new();
            loop {
                let part = peer.read(7, bytes.len() as u64);
                if part.is_empty() {
                    break;
                }
                bytes.extend(part);
                assert!(bytes.len() <= 2048);
            }
            peer.clunk(7);
            let limits = decode_shell_file_limits(&bytes).unwrap();
            (peer, negotiated.welcome, limits)
        });
        let welcome = f.welcome().unwrap();
        let deadline = Instant::now() + WAIT;
        while !peer.is_finished() {
            f.transport.poll_io(&mut f.epochs).unwrap();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        let (_peer, received, limits) = peer.join().unwrap();
        assert_eq!(received, welcome);
        assert_eq!(welcome.selected_revision, 6);
        assert_eq!(welcome.connection_epoch, epoch);
        assert!(f.transport.supports_content());
        assert_eq!(
            f.transport.content_reserved_bytes(&f.epochs),
            40 * 1024 * 1024
        );
        assert_eq!(
            f.transport.content_backing_reserved_bytes(&f.epochs),
            32 * 1024 * 1024
        );
        assert_eq!(limits.grant.connection_epoch, epoch);
        assert!(limits.grant.content_grant_epoch > prior);
        prior = limits.grant.content_grant_epoch;
        assert_eq!(f.transport.content_grant(), Some(limits.grant));
        assert_eq!(f.transport.content_limits(), Some(&limits));
        f.transport.disconnect(&mut f.epochs).unwrap();
        assert!(!f.transport.supports_content());
        assert!(
            f.transport
                .collect_content_accounting(&mut f.epochs)
                .quiescent()
        );
    }
}
#[test]
fn operator_denial_is_distinct_from_unavailable_implementation() {
    let mut f = Fixture::new();
    f.start(1, ShellContentAdmissionPolicy::Denied);
    f.refusal(
        CONTENT,
        ContentAdmissionRefused {
            reason: 1,
            denied_capabilities: CONTENT,
        },
    );
}
#[test]
fn discrete_input_denial_names_only_that_required_capability() {
    let mut f = Fixture::new();
    f.start(
        1,
        ShellContentAdmissionPolicy::Granted {
            discrete_input: false,
        },
    );
    f.refusal(
        CONTENT | INPUT,
        ContentAdmissionRefused {
            reason: 1,
            denied_capabilities: INPUT,
        },
    );
}
fn closed(error: std::io::Error) {
    assert!(
        matches!(
            error.kind(),
            std::io::ErrorKind::UnexpectedEof
                | std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::ConnectionReset
        ),
        "{error}"
    );
}
fn missing_capability(revision: u16, required: u64) {
    let mut f = Fixture::new();
    f.start(
        1,
        ShellContentAdmissionPolicy::Granted {
            discrete_input: true,
        },
    );
    let socket = f.transport.socket_path().to_owned();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();
        // Submission custody may precede refusal, but neither Negotiated nor
        // a content Refused record is valid for this malformed capability offer.
        let bytes = offer(1, revision, required);
        peer.open(5, b"transaction", 2);
        assert_eq!(peer.write(5, &bytes).0, 119);
        let submit = encode_shell_file_submit(ShellFileSubmit {
            connection_epoch: 1,
            submission_id: 1,
            candidate_bytes: bytes.len() as u32,
        })
        .unwrap();
        let reply = peer.rpc(
            118,
            &[
                3u32.to_le_bytes().as_slice(),
                &0u64.to_le_bytes(),
                &(submit.len() as u32).to_le_bytes(),
                &submit,
            ]
            .concat(),
        );
        match reply {
            Err(error) => closed(error),
            Ok(reply) => {
                assert_eq!(reply.0, 119);
                match peer.try_next_event() {
                    Err(error) => closed(error),
                    Ok(event) => {
                        assert_eq!(
                            decode_shell_file_submitted(&event).unwrap().submission_id,
                            1
                        );
                        closed(peer.try_next_event().unwrap_err());
                    }
                }
            }
        }
    });
    assert_eq!(f.welcome(), Err(ShellTransportError::MissingCapability));
    peer.join().unwrap();
    assert!(f.transport.content_accounting(&f.epochs).quiescent());
}
#[test]
fn invalid_content_dependencies_receive_no_content_record() {
    missing_capability(6, INPUT);
}
#[test]
fn pre_revision_five_peer_receives_no_content_record() {
    missing_capability(4, CONTENT);
}
