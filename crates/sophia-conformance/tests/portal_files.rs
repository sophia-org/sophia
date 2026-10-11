//! Independent C records through the production 9P server and portal export.
//! Admission and namespace/source facts are supplied by this fixture. The raw
//! child has real kernel peer credentials, but this is not a protected-launch
//! proof. The recording recipient supplies the actual effect result.
use sha2::{Digest, Sha256};
use sophia_9p::{export::PeerCredentials, unix::Server};
use sophia_portal::files::{
    PortalFileAuthority, PortalFileExport, PortalRequesterAdmission, PortalRequesterCustody,
};
use sophia_protocol::{
    NamespaceCapabilities, NamespaceContext, NamespaceId, NamespacePortalCapability,
    NamespaceProfile, PortalRequest, PortalTransferKind, portal_files::PortalFileLimits,
};
use std::{
    os::unix::{fs::DirBuilderExt, net::UnixListener},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

#[path = "support/bounded_peer.rs"]
#[expect(
    dead_code,
    reason = "The owner drives Peer while servicing 9P; the shared blocking run helper is unused."
)]
mod bounded_peer;
#[path = "support/c_file_peer.rs"]
mod c_file_peer;

struct Authority;
impl PortalFileAuthority for Authority {
    fn namespace(&self, id: NamespaceId) -> Option<NamespaceContext> {
        let capabilities = match id.raw() {
            1 => NamespaceCapabilities::NONE.with_publish(NamespacePortalCapability::Clipboard),
            2 => NamespaceCapabilities::NONE.with_request(NamespacePortalCapability::Clipboard),
            _ => return None,
        };
        NamespaceContext::new(id, NamespaceProfile::Confined, capabilities)
    }
    fn source_generation(&self, request: &PortalRequest) -> Option<u64> {
        (request.transfer.transfer.raw() == 41).then_some(9)
    }
    fn now_msec(&self) -> u64 {
        100
    }
    fn live(&self) -> bool {
        true
    }
    fn allow(&self, request: &PortalRequest) -> bool {
        request.transfer.kind == PortalTransferKind::Clipboard
    }
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "pf-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        Self(path)
    }
    fn peer(&self, mutation: u32) -> PathBuf {
        let path = c_file_peer::build(&self.0, "portal_file_peer", mutation);
        println!(
            "portal_c_artifact mutation={mutation} path={} sha256={:x}",
            path.display(),
            Sha256::digest(std::fs::read(&path).unwrap())
        );
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scenario(directory: &Path, executable: &Path, mode: &str) -> (Output, Vec<bool>) {
    let path = directory.join("socket");
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut child = bounded_peer::Peer::spawn(
        Command::new(executable).arg(&path).arg(mode),
        Duration::from_secs(15),
    );
    let started = Instant::now();
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("accept C peer: {error}"),
        }
        assert!(
            child.poll().unwrap().is_none(),
            "peer exited before connect"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "connect deadline"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
    let credentials = rustix::net::sockopt::socket_peercred(&stream).unwrap();
    assert_eq!(credentials.pid.as_raw_nonzero().get() as u32, child.id());
    let mut limits = PortalFileLimits::defaults(3);
    if mode == "withhold" {
        limits.ack_progress_timeout_ms = 200;
    }
    let export = PortalFileExport::new(Authority, limits).unwrap();
    let mut server = Server::new(export, sophia_9p::Limits::default()).unwrap();
    let connection = server.adopt(stream).unwrap_or_else(|_| panic!("adopt"));
    server
        .export_mut()
        .admit(PortalRequesterAdmission {
            connection,
            custody: PortalRequesterCustody::ProtectedPeer(PeerCredentials {
                pid: credentials.pid.as_raw_nonzero().get(),
                uid: credentials.uid.as_raw(),
                gid: credentials.gid.as_raw(),
            }),
            epoch: 7,
            source: NamespaceId::from_raw(1),
            target: NamespaceId::from_raw(2),
        })
        .unwrap();
    let mut effects = Vec::new();
    let output = loop {
        if let Some(output) = child.poll().unwrap() {
            break output;
        }
        server.export_mut().expire(Instant::now());
        server.turn(Some(Duration::from_millis(1))).unwrap();
        if let Some(work) = server.export_mut().take_execution() {
            let accepted = work.grant.transfer.raw() == 41
                && work.grant.source_generation == 9
                && work.grant.broker_generation == 3
                && work.payload == b"independent-c-portal";
            effects.push(accepted);
            server
                .export_mut()
                .settle_execution(work.grant.transfer, accepted)
                .unwrap();
        }
    };
    server.export_mut().disconnect(connection);
    std::fs::remove_file(path).unwrap();
    (output, effects)
}

#[test]
fn independent_c_portal_records_preserve_custody_and_recipient_outcome() {
    let scratch = Scratch::new();
    let peer = scratch.peer(0);
    let (output, effects) = scenario(&scratch.0, &peer, "transfer");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        effects,
        [true],
        "one complete valid payload reaches the recipient"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("mode=transfer status=complete"));
}

#[test]
fn independent_c_refusal_drains_on_ack_and_expires_without_ack() {
    let scratch = Scratch::new();
    let peer = scratch.peer(0);
    for mode in ["refused", "withhold"] {
        let (output, effects) = scenario(&scratch.0, &peer, mode);
        assert!(
            output.status.success(),
            "{mode}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            effects.is_empty(),
            "negotiation refusal has no recipient effect"
        );
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .contains(&format!("mode={mode} status=complete"))
        );
    }
}

#[test]
fn compiled_c_peer_controls_cannot_report_success() {
    let scratch = Scratch::new();
    for (mutation, message, expected_effects) in [
        (1, "valid candidate refused", vec![]),
        (2, "terminal outcome must be Completed", vec![false]),
    ] {
        let peer = scratch.peer(mutation);
        let (output, effects) = scenario(&scratch.0, &peer, "transfer");
        assert!(!output.status.success(), "mutation {mutation} was accepted");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(message),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("status=complete"));
        assert_eq!(effects, expected_effects);
    }
}
