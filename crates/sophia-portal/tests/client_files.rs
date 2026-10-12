//! File semantics with supplied authenticated-root commitment. The Session SDK
//! join separately qualifies real afid authentication and core retry behavior.
#[path = "support/client_object_fixture.rs"]
mod support;
use sophia_9p::{ConnectionId, Errno, OpenFlags, export::*};
use sophia_portal::client_files::{PortalClientFiles, PortalClientNode as Node};
use sophia_protocol::NamespaceId;
use sophia_runtime::authenticated_files::{PreparedFileRoot, ProtectedFiles};
use std::sync::atomic::Ordering;

fn context(connection: u64) -> AttachContext<'static> {
    AttachContext {
        connection: ConnectionId(connection),
        peer: None,
        uname: b"namespace-client",
        aname: b"portal-client",
        n_uname: u32::MAX,
    }
}
fn admitted(f: &support::Fixture) -> PortalClientFiles {
    let mut files = PortalClientFiles::new(NamespaceId::from_raw(2), f.objects.clone(), 7).unwrap();
    files.bind_connection(ConnectionId(1)).unwrap();
    files
        .prepare_authenticated_root(&context(1))
        .unwrap()
        .publish();
    files
}
fn checked(files: &mut PortalClientFiles, node: &Node) -> Result<(), Errno> {
    files.check(&Access {
        connection: ConnectionId(1),
        epoch: Epoch(7),
        node,
        operation: Operation::Read,
    })
}

#[test]
fn client_export_has_no_unauthenticated_root_or_coordinator_authority() {
    let f = support::Fixture::new();
    assert!(PortalClientFiles::new(NamespaceId::from_raw(3), f.objects.clone(), 7).is_err());
    let mut files = PortalClientFiles::new(NamespaceId::from_raw(2), f.objects.clone(), 7).unwrap();
    assert_eq!(files.attach(&context(1)).err(), Some(Errno::EACCES));
    files.bind_connection(ConnectionId(1)).unwrap();
    assert!(files.bind_connection(ConnectionId(2)).is_err());
    assert!(files.prepare_authenticated_root(&context(2)).is_err());
    files
        .prepare_authenticated_root(&context(1))
        .unwrap()
        .publish();
    assert!(files.prepare_authenticated_root(&context(1)).is_err());
    for name in [
        b"transaction".as_slice(),
        b"submit",
        b"ack",
        b"upload",
        b"ctl",
    ] {
        assert_eq!(
            files.lookup(&Node::Root, WalkName::Child(name)),
            Err(Errno::ENOENT)
        );
    }
    assert_eq!(
        files.lookup(&Node::Root, WalkName::Child(b"transfer")),
        Err(Errno::ENOENT)
    );
    files.reset_authenticated_root(ConnectionId(1));
    assert_eq!(checked(&mut files, &Node::Root), Err(Errno::ESTALE));
    files
        .prepare_authenticated_root(&context(1))
        .unwrap()
        .publish();
    assert_eq!(checked(&mut files, &Node::Root), Ok(()));
}

#[test]
fn common_grants_govern_data_metadata_and_waiting_watch_reads() {
    for boundary in ["generation", "lock", "binding"] {
        let f = support::Fixture::new();
        let mut files = admitted(&f);
        f.transfer(1);
        let data = files
            .lookup(&Node::Root, WalkName::Child(b"transfer"))
            .unwrap();
        let watch = files
            .lookup(&Node::Root, WalkName::Child(b"watch"))
            .unwrap();
        let mut data_handle = files.open(&data, OpenFlags(0)).unwrap();
        let mut watch_handle = files.open(&watch, OpenFlags(0)).unwrap();
        assert_eq!(
            files.read(&data, &mut data_handle, 0, 100).unwrap(),
            ReadOutcome::Ready(b"granted".to_vec())
        );
        assert_eq!(files.describe(&data, Some(&data_handle)).size, 7);
        assert_eq!(
            files.read(&watch, &mut watch_handle, 0, 1).unwrap(),
            ReadOutcome::Pending
        );
        assert!(files.open(&data, OpenFlags(1)).is_err());
        assert!(
            f.objects.current().is_ok(),
            "natural expiry must not mask the control boundary"
        );
        match boundary {
            "generation" => f.source.0.store(10, Ordering::Release),
            "lock" => {
                f.fence.block();
                f.fence.unblock();
            }
            "binding" => f.objects.revoke(),
            _ => unreachable!(),
        }
        assert_eq!(
            checked(&mut files, &data),
            Err(Errno::ESTALE),
            "retained transfer authority must end at {boundary}"
        );
        assert_eq!(
            checked(&mut files, &watch),
            Err(Errno::ESTALE),
            "waiting watch must be stale at {boundary}"
        );
        assert_eq!(
            files.read(&watch, &mut watch_handle, 0, 1),
            Err(Errno::ESTALE)
        );
        assert_eq!(
            files.read(&data, &mut data_handle, 0, 100),
            Err(Errno::ESTALE)
        );
        assert_eq!(
            checked(&mut files, &Node::Status),
            Ok(()),
            "transfer revocation does not revoke the role"
        );
    }
}

#[test]
fn replacement_does_not_retarget_old_fids_or_opened_status_metadata() {
    let f = support::Fixture::new();
    let mut files = admitted(&f);
    let mut status = files.open(&Node::Status, OpenFlags(0)).unwrap();
    let metadata = files.describe(&Node::Status, Some(&status));
    let before = files.read(&Node::Status, &mut status, 0, 1024).unwrap();
    f.transfer(1);
    let old = files
        .lookup(&Node::Root, WalkName::Child(b"transfer"))
        .unwrap();
    f.transfer(2);
    let new = files
        .lookup(&Node::Root, WalkName::Child(b"transfer"))
        .unwrap();
    assert_ne!(
        files.describe(&old, None).qid_path,
        files.describe(&new, None).qid_path
    );
    assert_eq!(checked(&mut files, &old), Err(Errno::ESTALE));
    assert_eq!(checked(&mut files, &new), Ok(()));
    assert_eq!(files.describe(&Node::Status, Some(&status)), metadata);
    assert_eq!(
        files.read(&Node::Status, &mut status, 0, 1024).unwrap(),
        before
    );
}
