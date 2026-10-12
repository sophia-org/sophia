//! Root preparation, not authentication evidence. The wire and Session joins
//! separately prove that only a verified identity can publish this preparation.
use super::*;
use crate::authenticated_files::{PreparedFileRoot, ProtectedFiles};
use sophia_9p::Operation;

fn claims(connection: u64) -> AttachContext<'static> {
    AttachContext {
        connection: ConnectionId(connection),
        peer: None,
        uname: b"shell",
        aname: b"descriptor",
        n_uname: u32::MAX,
    }
}

fn access() -> Access<'static, Node> {
    Access {
        connection: ConnectionId(1),
        epoch: Epoch(EPOCH),
        node: &Node::Root,
        operation: Operation::Read,
    }
}

#[test]
fn shell_root_is_inaccessible_until_preparation_is_published() {
    let mut files = export("descriptor", BASE);
    files.bind_connection(ConnectionId(1));
    let qid = files.next_qid();
    {
        let _abandoned = files.prepare_authenticated_root(&claims(1)).unwrap();
    }
    assert_eq!(
        files.check(&access()),
        Err(Errno::ESTALE),
        "abandoned shell preparation must not admit the root"
    );
    assert_eq!(files.next_qid(), qid);
    let root = files
        .prepare_authenticated_root(&claims(1))
        .unwrap()
        .publish();
    assert_eq!(root.root, Node::Root);
    assert_eq!(root.epoch, Epoch(EPOCH));
    assert_eq!(files.check(&access()), Ok(()));
    assert!(matches!(
        files.prepare_authenticated_root(&claims(1)),
        Err(Errno::EACCES)
    ));
}

#[test]
fn shell_root_refuses_foreign_connections_and_revoked_epochs() {
    let mut files = export("descriptor", BASE);
    files.bind_connection(ConnectionId(1));
    assert!(matches!(
        files.prepare_authenticated_root(&claims(2)),
        Err(Errno::EACCES)
    ));
    files.revoke();
    assert!(matches!(
        files.prepare_authenticated_root(&claims(1)),
        Err(Errno::EACCES)
    ));
}

#[test]
fn shell_wire_reset_preserves_objects_and_never_reuses_auth_qids() {
    let mut files = export("descriptor", BASE);
    files.bind_connection(ConnectionId(1));
    let first = files.allocate_auth_qid().unwrap();
    files
        .prepare_authenticated_root(&claims(1))
        .unwrap()
        .publish();
    publish(&mut files, objects(1).remove(0));
    let bytes = files.descriptors.current.as_ref().unwrap().bytes.clone();
    let journal = files.journal.records();
    files.reset_authenticated_root(ConnectionId(2));
    assert_eq!(files.check(&access()), Ok(()));
    files.reset_authenticated_root(ConnectionId(1));
    assert_eq!(files.check(&access()), Err(Errno::ESTALE));
    assert_eq!(files.journal.records(), journal);
    assert_eq!(files.descriptors.current.as_ref().unwrap().bytes, bytes);
    files
        .prepare_authenticated_root(&claims(1))
        .unwrap()
        .publish();
    assert!(files.allocate_auth_qid().unwrap() > first);
    assert_eq!(files.check(&access()), Ok(()));
    files.next_qid = u64::MAX;
    assert_eq!(files.allocate_auth_qid(), Err(Errno::ENOSPC));
    assert_eq!(files.next_qid(), u64::MAX);
}
