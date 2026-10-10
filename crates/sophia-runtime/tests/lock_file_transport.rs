#![cfg(target_os = "linux")]
//! t294: the lock provider's endpoint. Only the authorized process attaches;
//! every admitted connection gets a fresh epoch; a provider that never
//! negotiates loses the role; and Session's lock publications reach the
//! provider as events over real 9P.
#[path = "support/shell_file_peer.rs"]
mod raw_peer;

use std::os::unix::net::UnixStream;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sophia_protocol::lock_files::*;
use sophia_runtime::lock_files::*;

fn limits(assembly_timeout_ms: u32) -> LockFileLimits {
    LockFileLimits {
        max_outputs: 1,
        upload_slots: 1,
        max_chords: 0,
        max_width_px: 16,
        max_height_px: 16,
        max_resource_bytes: 1024,
        max_live_resources: 2,
        journal_records: 32,
        journal_bytes: 8192,
        assembly_timeout_ms,
        ack_progress_timeout_ms: 2000,
    }
}

fn unlocked() -> LockObject {
    LockObject {
        lock_epoch: 0,
        topology_generation: 1,
        phase: LockPhase::Unlocked,
        allocations: Vec::new(),
    }
}

fn locked() -> LockObject {
    LockObject {
        lock_epoch: 3,
        topology_generation: 1,
        phase: LockPhase::Locked,
        allocations: vec![LockAllocation {
            output_id: 1,
            output_generation: 1,
            allocation_id: 2,
            allocation_generation: 1,
            pixel_width: 4,
            pixel_height: 4,
            scale_numerator: 1,
            scale_denominator: 1,
        }],
    }
}

fn transport(label: &str, assembly_timeout_ms: u32) -> LockFileTransport {
    let directory = std::env::temp_dir().join(format!(
        "lock-file-transport-{label}-{}",
        std::process::id()
    ));
    LockFileTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
        7,
        limits(assembly_timeout_ms),
    )
    .unwrap()
}

/// Pumps the transport until `peer` finishes, accepting as needed.
fn pump<T>(transport: &mut LockFileTransport, peer: JoinHandle<T>, lock: &LockObject) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !peer.is_finished() {
        assert!(Instant::now() < deadline, "peer did not finish");
        if transport.export().is_none() {
            let _ = transport.poll_accept(lock, &[]);
        } else {
            let _ = transport.turn();
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    peer.join().unwrap()
}

fn record(kind: LockFileKind, epoch: u64, submission_id: u64, body: &[u8]) -> Vec<u8> {
    encode_lock_file_record(
        LockFileHeader {
            kind,
            connection_epoch: epoch,
            submission_id,
            sequence: 0,
        },
        body,
    )
    .unwrap()
}

fn negotiate(peer: &mut raw_peer::Peer, epoch: u64) {
    let body = LockNegotiate {
        minimum_revision: 1,
        maximum_revision: 1,
        requested_capabilities: LOCK_FILE_CAPABILITY_PRESENT,
        chords: Vec::new(),
    }
    .encode()
    .unwrap();
    let candidate = record(LockFileKind::Negotiate, epoch, 1, &body);
    peer.open(5, b"transaction", 2);
    assert_eq!(peer.write(5, &candidate).0, 119);
    let submit = encode_lock_file_submit(LockFileSubmit {
        connection_epoch: epoch,
        submission_id: 1,
        candidate_bytes: candidate.len() as u32,
    })
    .unwrap();
    assert_eq!(peer.write(3, &submit).0, 119);
    peer.clunk(5);
}

/// Every event record in `bytes`, as (kind, sequence).
fn events(bytes: &[u8]) -> Vec<(LockFileKind, u64)> {
    let mut out = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        let size = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
        let record = decode_lock_file_record(&rest[..size], LockFileClass::Event).unwrap();
        out.push((record.header.kind, record.header.sequence));
        rest = &rest[size..];
    }
    out
}

fn epoch_of(api: &[u8]) -> u64 {
    let api = std::str::from_utf8(api).unwrap();
    api.trim_end()
        .rsplit_once("epoch=")
        .unwrap()
        .1
        .parse()
        .unwrap()
}

#[test]
fn an_authorized_provider_negotiates_and_sees_the_lock_published() {
    let mut transport = transport("published", 1000);
    transport
        .authorize_supervised_pid(std::process::id())
        .unwrap();
    let path = transport.socket_path().to_owned();
    let peer = std::thread::spawn(move || {
        let mut peer = raw_peer::Peer::connect(&path);
        peer.setup();
        peer.open(6, b"api", 0);
        let epoch = epoch_of(&peer.read(6, 0));
        negotiate(&mut peer, epoch);
        let first = peer.read(2, 0);
        (peer, epoch, first)
    });
    let (mut peer, epoch, first) = pump(&mut transport, peer, &unlocked());
    assert_eq!(epoch, 7);
    assert_eq!(
        events(&first)
            .into_iter()
            .map(|(kind, _)| kind)
            .collect::<Vec<_>>(),
        [
            LockFileKind::Submitted,
            LockFileKind::Negotiated,
            LockFileKind::ObjectPublished
        ]
    );
    // Session locks: the provider sees a new publication, then the object.
    transport.publish_lock(locked()).unwrap();
    let offset = first.len() as u64;
    let reader = std::thread::spawn(move || {
        let published = peer.read(2, offset);
        peer.open(7, b"lock", 0);
        let lock = peer.read(7, 0);
        (published, lock)
    });
    let (published, lock) = pump(&mut transport, reader, &unlocked());
    assert_eq!(events(&published)[0].0, LockFileKind::ObjectPublished);
    let lock = decode_lock_file_record(&lock, LockFileClass::Object).unwrap();
    assert_eq!(LockObject::decode(lock.body).unwrap(), locked());
}

#[test]
fn a_replacement_connection_gets_a_fresh_epoch() {
    let mut transport = transport("replacement", 1000);
    transport
        .authorize_supervised_pid(std::process::id())
        .unwrap();
    let path = transport.socket_path().to_owned();
    for expected in [7, 8] {
        let path = path.clone();
        let peer = std::thread::spawn(move || {
            let mut peer = raw_peer::Peer::connect(&path);
            peer.setup();
            peer.open(6, b"api", 0);
            epoch_of(&peer.read(6, 0))
        });
        assert_eq!(pump(&mut transport, peer, &unlocked()), expected);
        transport.disconnect().unwrap();
        assert!(transport.export().is_none());
    }
    assert_eq!(transport.next_epoch(), 9);
}

#[test]
fn an_unauthorized_process_is_never_admitted() {
    let mut transport = transport("unauthorized", 1000);
    // The parent is alive but is not this connector.
    transport
        .authorize_supervised_pid(rustix::process::getppid().unwrap().as_raw_pid() as u32)
        .unwrap();
    let _stream = UnixStream::connect(transport.socket_path()).unwrap();
    for _ in 0..50 {
        assert!(!transport.poll_accept(&unlocked(), &[]).unwrap_or(false));
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(transport.export().is_none());
    assert_eq!(transport.next_epoch(), 7, "no epoch was spent");
}

#[test]
fn a_provider_that_never_negotiates_loses_the_role() {
    let mut transport = transport("silent", 30);
    transport
        .authorize_supervised_pid(std::process::id())
        .unwrap();
    // Silence is the condition under test. Performing a multi-RPC setup under
    // the same 30 ms deadline races correct revocation on a busy test host.
    let _peer = UnixStream::connect(transport.socket_path()).unwrap();
    assert!(transport.poll_accept(&unlocked(), &[]).unwrap());
    assert!(!transport.export().unwrap().custody().is_negotiated());
    assert!(!transport.export().unwrap().is_revoked());
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline, "never revoked");
        if !transport.turn().unwrap_or(false) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(transport.export().unwrap().is_revoked());
}
