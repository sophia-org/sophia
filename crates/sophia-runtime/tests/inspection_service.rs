#![cfg(target_os = "linux")]
#[path = "support/inspection_wire.rs"]
mod wire;

use sophia_runtime::inspection::*;
use std::io::{Read, Write};
use std::os::unix::{fs::PermissionsExt, net::UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use wire::*;

static NEXT: AtomicU64 = AtomicU64::new(1);
struct Fixture {
    service: InspectionService,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "si-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut service = InspectionService::bind(&root).unwrap();
        service.fence(9, None).unwrap();
        Self { service, root }
    }
    fn publish(&self, value: InspectionSnapshot, event: Option<InspectionEvent>) -> u64 {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match self
                .service
                .publisher()
                .publish(value.clone(), event)
                .unwrap()
            {
                PublishOutcome::Published { sequence } => return sequence,
                PublishOutcome::Busy { .. } => {
                    assert!(Instant::now() < deadline);
                    std::thread::yield_now();
                }
                PublishOutcome::Unchanged => panic!("expected changed publication"),
            }
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn snapshot() -> InspectionSnapshot {
    InspectionSnapshot {
        session_generation: 4,
        wm_epoch: 9,
        scene_generation: 1,
        selected_capabilities: u64::MAX,
        wire: InspectionWire::CurrentIpc,
        state: InspectionState::Ready,
        outputs: vec![],
        surfaces: vec![],
    }
}
fn pin(stream: &mut UnixStream, fid: u32) -> InspectionSnapshotRecord {
    opened(stream, fid, b"snapshot");
    decode_inspection_snapshot(body(&call(stream, &read(4, fid, 0, 65525)))).unwrap()
}

#[test]
fn admitted_observer_lists_only_fixed_read_only_root_and_pins_snapshot() {
    let fixture = Fixture::new();
    fixture.publish(snapshot(), None);
    let mut stream = connect(fixture.service.socket_path());
    let mut clone = 0_u32.to_le_bytes().to_vec();
    clone.extend_from_slice(&14_u32.to_le_bytes());
    clone.extend_from_slice(&0_u16.to_le_bytes());
    assert_eq!(call(&mut stream, &frame(110, 2, &clone))[4], 111);
    assert_eq!(call(&mut stream, &open(2, 14, 0))[4], 13);
    let mut dir = 14_u32.to_le_bytes().to_vec();
    dir.extend_from_slice(&0_u64.to_le_bytes());
    dir.extend_from_slice(&1024_u32.to_le_bytes());
    let listing = call(&mut stream, &frame(40, 3, &dir));
    assert_eq!(listing[4], 41);
    let mut pos = 11;
    let mut names = Vec::new();
    while pos < listing.len() {
        let len = u16::from_le_bytes(listing[pos + 22..pos + 24].try_into().unwrap()) as usize;
        names.push(listing[pos + 24..pos + 24 + len].to_vec());
        pos += 24 + len;
    }
    assert_eq!(
        names,
        [
            b"api".to_vec(),
            b"status".to_vec(),
            b"snapshot".to_vec(),
            b"events".to_vec()
        ]
    );
    call(&mut stream, &clunk(8, 14));
    for name in [b"submit".as_slice(), b"ack", b"transaction", b"profile"] {
        assert_eq!(errno(&call(&mut stream, &walk(8, 15, name))), 2);
    }
    let first = pin(&mut stream, 1);
    assert_eq!(first.snapshot, snapshot());
    assert_eq!(call(&mut stream, &walk(5, 2, b"snapshot"))[4], 111);
    assert_eq!(
        errno(&call(&mut stream, &open(6, 2, 0))),
        16,
        "one snapshot pin"
    );
    assert_eq!(
        errno(&call(&mut stream, &open(7, 2, 1))),
        13,
        "no write authority"
    );
    let mut changed = snapshot();
    changed.scene_generation = 2;
    fixture.publish(changed.clone(), Some(InspectionEvent::ProjectionCommitted));
    assert_eq!(
        decode_inspection_snapshot(body(&call(&mut stream, &read(4, 1, 0, 65525)))).unwrap(),
        first
    );
    call(&mut stream, &clunk(8, 1));
    assert_eq!(pin(&mut stream, 3).snapshot, changed);
}

#[test]
fn status_before_first_snapshot_is_unavailable_then_copies_safe_scalars() {
    let fixture = Fixture::new();
    let mut stream = connect(fixture.service.socket_path());
    opened(&mut stream, 1, b"status");
    let before = decode_inspection_status(body(&call(&mut stream, &read(4, 1, 0, 4096)))).unwrap();
    assert_eq!(
        (
            before.state,
            before.wire,
            before.session_generation,
            before.selected_capabilities
        ),
        (InspectionState::Unavailable, None, 0, 0)
    );
    assert!(!before.snapshot_available);
    fixture.publish(snapshot(), None);
    opened(&mut stream, 2, b"status");
    let after = decode_inspection_status(body(&call(&mut stream, &read(4, 2, 0, 4096)))).unwrap();
    assert_eq!(
        (
            after.state,
            after.wire,
            after.session_generation,
            after.selected_capabilities
        ),
        (
            InspectionState::Ready,
            Some(InspectionWire::CurrentIpc),
            4,
            u64::MAX
        )
    );
    assert!(after.snapshot_available);
}

#[test]
fn loss_rejects_existing_watch_then_unchanged_republish_requires_fresh_pin() {
    let fixture = Fixture::new();
    fixture.publish(snapshot(), None);
    let mut stream = connect(fixture.service.socket_path());
    let old = pin(&mut stream, 1);
    opened(&mut stream, 2, b"events");
    stream
        .write_all(&read(20, 2, old.event_offset, 4096))
        .unwrap();
    let mut invalid = snapshot();
    invalid.outputs = vec![InspectionOutput {
        id: 0,
        generation: 0,
        geometry: InspectionRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        work_area: InspectionRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        focus: None,
    }];
    assert!(matches!(
        fixture
            .service
            .publisher()
            .publish(invalid, Some(InspectionEvent::ConfigurationRejected)),
        Err(InspectionError::Record(_))
    ));
    assert_eq!(
        errno(&receive(&mut stream).unwrap()),
        116,
        "pending read sees loss before new bytes"
    );
    fixture.publish(snapshot(), None);
    assert_eq!(
        errno(&call(&mut stream, &read(4, 2, old.event_offset, 4096))),
        116
    );
    assert_eq!(errno(&call(&mut stream, &read(4, 1, 0, 4096))), 116);
    call(&mut stream, &clunk(8, 1));
    call(&mut stream, &clunk(9, 2));
    let fresh = pin(&mut stream, 3);
    assert!(fresh.sequence > old.sequence);
    assert!(fresh.loss_generation > old.loss_generation);
    opened(&mut stream, 4, b"events");
    fixture.publish(snapshot(), Some(InspectionEvent::SessionOperationAccepted));
    let bytes = call(&mut stream, &read(5, 4, fresh.event_offset, 4096));
    assert_eq!(
        decode_inspection_event(body(&bytes)).unwrap().event,
        InspectionEvent::SessionOperationAccepted
    );
}

#[test]
fn quiet_slow_reader_does_not_hold_publication_credit_and_gets_explicit_gap() {
    let fixture = Fixture::new();
    fixture.publish(snapshot(), None);
    let mut stream = connect(fixture.service.socket_path());
    let first = pin(&mut stream, 1);
    opened(&mut stream, 2, b"events");
    let deadline = Instant::now() + Duration::from_secs(3);
    for _ in 0..70 {
        fixture.publish(snapshot(), Some(InspectionEvent::SnapshotChanged));
    }
    assert!(
        Instant::now() < deadline,
        "no observer ACK or writer credit is needed"
    );
    assert_eq!(
        errno(&call(&mut stream, &read(5, 2, first.event_offset, 4096))),
        116
    );
    opened(&mut stream, 3, b"status");
    let status = decode_inspection_status(body(&call(&mut stream, &read(4, 3, 0, 4096)))).unwrap();
    assert!(status.event_floor > first.event_offset);
    assert!(status.sequence >= 71);
    assert_eq!(
        errno(&call(&mut stream, &read(5, 2, status.event_tail, 4096))),
        116,
        "a gapped watch cannot silently jump to the current tail"
    );
    assert_eq!(call(&mut stream, &walk(2, 4, b"events"))[4], 111);
    assert_eq!(
        errno(&call(&mut stream, &open(3, 4, 0))),
        11,
        "another watch still requires a fresh snapshot"
    );
    call(&mut stream, &clunk(8, 1));
    let fresh = pin(&mut stream, 5);
    opened(&mut stream, 6, b"events");
    fixture.publish(snapshot(), Some(InspectionEvent::ConfigurationChanged));
    let event = call(&mut stream, &read(5, 6, fresh.event_offset, 4096));
    assert_eq!(
        decode_inspection_event(body(&event)).unwrap().event,
        InspectionEvent::ConfigurationChanged
    );
}

#[test]
fn fence_closes_pending_old_authority_and_qids_continue_in_fresh_epoch() {
    let mut fixture = Fixture::new();
    fixture.publish(snapshot(), None);
    let mut stream = connect(fixture.service.socket_path());
    let qid = opened(&mut stream, 1, b"snapshot");
    let first = decode_inspection_snapshot(body(&call(&mut stream, &read(4, 1, 0, 4096)))).unwrap();
    opened(&mut stream, 2, b"events");
    stream
        .write_all(&read(20, 2, first.event_offset, 4096))
        .unwrap();
    fixture.service.fence(10, None).unwrap();
    let old = receive(&mut stream);
    assert!(old.is_err() || old.is_ok_and(|r| r[4] == 7));
    let mut value = snapshot();
    value.wm_epoch = 10;
    fixture.publish(value, None);
    let mut fresh = connect(fixture.service.socket_path());
    let new_qid = opened(&mut fresh, 1, b"snapshot");
    assert_ne!(qid, new_qid);
    let new = decode_inspection_snapshot(body(&call(&mut fresh, &read(4, 1, 0, 4096)))).unwrap();
    assert!(new.generation > first.generation);
    assert!(new.sequence > first.sequence);
    fixture.service.fence(11, Some(std::process::id())).unwrap();
    let mut denied = UnixStream::connect(fixture.service.socket_path()).unwrap();
    denied
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    denied.write_all(&version()).unwrap();
    assert!(
        receive(&mut denied).is_err(),
        "same UID does not override explicit role PID exclusion"
    );
}

#[test]
fn binding_refuses_nonprivate_or_symlink_runtime_directory() {
    let fixture = Fixture::new();
    let bad = fixture.root.join("public");
    std::fs::create_dir(&bad).unwrap();
    std::fs::set_permissions(&bad, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(InspectionService::bind(&bad).is_err());
    let link = fixture.root.join("link");
    std::os::unix::fs::symlink(&fixture.root, &link).unwrap();
    assert!(InspectionService::bind(&link).is_err());
}

#[test]
fn service_drop_closes_observer_and_removes_only_its_endpoint() {
    let fixture = Fixture::new();
    let mut service = InspectionService::bind(&fixture.root).unwrap();
    service.fence(9, None).unwrap();
    let path = service.socket_path().to_path_buf();
    let mut stream = connect(&path);
    let publisher = service.publisher();
    drop(service);
    let mut byte = [0];
    assert!(matches!(stream.read(&mut byte), Ok(0) | Err(_)));
    assert!(!path.exists());
    assert!(fixture.root.exists());
    assert!(fixture.service.socket_path().exists());
    assert!(matches!(
        publisher.publish(snapshot(), None),
        Err(InspectionError::Stopped)
    ));
}

#[test]
fn four_peer_sixteen_fid_and_eight_pending_limits_are_real_socket_bounds() {
    let fixture = Fixture::new();
    fixture.publish(snapshot(), None);
    let mut peers = (0..4)
        .map(|_| connect(fixture.service.socket_path()))
        .collect::<Vec<_>>();
    let mut extra = UnixStream::connect(fixture.service.socket_path()).unwrap();
    extra
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let _ = extra.write_all(&version());
    assert!(receive(&mut extra).is_err());
    let stream = &mut peers[0];
    let pinned = pin(stream, 1);
    opened(stream, 2, b"events");
    for fid in 3..16 {
        assert_eq!(call(stream, &walk(2, fid, b"api"))[4], 111);
    }
    assert_eq!(errno(&call(stream, &walk(2, 16, b"api"))), 24);
    for tag in 20..29 {
        stream
            .write_all(&read(tag, 2, pinned.event_offset, 512))
            .unwrap();
    }
    let refused = receive(stream).unwrap();
    assert_eq!(u16::from_le_bytes(refused[5..7].try_into().unwrap()), 28);
    assert_eq!(errno(&refused), 11);
    for tag in 20_u16..28 {
        assert_eq!(
            call(stream, &frame(108, tag + 100, &tag.to_le_bytes()))[4],
            109
        );
    }
}

#[test]
fn same_uid_in_another_namespace_cannot_use_discovered_observer_socket() {
    let fixture = Fixture::new();
    fixture.publish(snapshot(), None);
    let status = std::process::Command::new("/usr/bin/bwrap")
        .args([
            "--unshare-user",
            "--unshare-pid",
            "--unshare-net",
            "--ro-bind",
            "/",
            "/",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
        ])
        .arg(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "namespace_denied_child",
            "--ignored",
            "--test-threads=1",
        ])
        .env(
            "SOPHIA_INSPECTION_TEST_SOCKET",
            fixture.service.socket_path(),
        )
        .status()
        .unwrap();
    assert!(
        status.success(),
        "namespace proof requires working bubblewrap; it cannot silently skip"
    );
}

#[test]
#[ignore = "invoked with exact endpoint by the namespace-denial parent"]
fn namespace_denied_child() {
    let path = std::env::var_os("SOPHIA_INSPECTION_TEST_SOCKET").expect("explicit child endpoint");
    let mut stream = UnixStream::connect(path).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let _ = stream.write_all(&version());
    assert!(receive(&mut stream).is_err());
}
