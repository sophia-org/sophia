//! Output-root ownership only. Authentication and namespace authority are not
//! supplied or claimed here; protected-role interoperability is a separate join.
use super::*;
use sophia_runtime::authenticated_files::{PreparedFileRoot, ProtectedFiles};

fn claims(connection: u64) -> AttachContext<'static> {
    AttachContext {
        connection: ConnectionId(connection),
        peer: None,
        uname: b"output",
        aname: b"output-authority",
        n_uname: u32::MAX,
    }
}

fn pending(qids: OutputFileQids) -> OutputFileExport {
    let mut view =
        OutputFileExport::new(7, OutputFileLimits::default(), snapshot(4), qids).unwrap();
    view.bind_connection(ConnectionId(1)).unwrap();
    view
}

fn root_access() -> Access<'static, Node> {
    Access {
        connection: ConnectionId(1),
        epoch: Epoch(7),
        node: &Node::Root,
        operation: Operation::Read,
    }
}

#[test]
fn abandoned_output_root_does_not_publish_admission() {
    let mut view = pending(OutputFileQids::default());
    {
        let _abandoned = view.prepare_authenticated_root(&claims(1)).unwrap();
    }
    assert_eq!(
        view.check(&root_access()),
        Err(Errno::ESTALE),
        "an abandoned output root must remain inaccessible"
    );
    let attached = view
        .prepare_authenticated_root(&claims(1))
        .unwrap()
        .publish();
    assert_eq!(attached.root, Node::Root);
    assert_eq!(attached.epoch, Epoch(7));
    assert_eq!(view.check(&root_access()), Ok(()));
    assert!(matches!(
        view.prepare_authenticated_root(&claims(1)),
        Err(Errno::EACCES)
    ));
}

#[test]
fn foreign_or_revoked_output_grant_cannot_prepare_a_root() {
    let mut view = pending(OutputFileQids::default());
    assert!(matches!(
        view.prepare_authenticated_root(&claims(2)),
        Err(Errno::EACCES)
    ));
    assert_eq!(view.check(&root_access()), Err(Errno::ESTALE));
    view.revoke();
    assert!(matches!(
        view.prepare_authenticated_root(&claims(1)),
        Err(Errno::ESTALE)
    ));
    view.reset_authenticated_root(ConnectionId(1));
    assert!(
        matches!(
            view.prepare_authenticated_root(&claims(1)),
            Err(Errno::ESTALE)
        ),
        "wire reset cannot revive a revoked output grant"
    );
}

#[test]
fn output_wire_reset_keeps_role_negotiation_and_qid_identity() {
    let qids = OutputFileQids::default();
    let mut view = pending(qids.clone());
    let first_auth = view.allocate_auth_qid().unwrap();
    view.prepare_authenticated_root(&claims(1))
        .unwrap()
        .publish();
    let (staging, _) = negotiate(&mut view, SOPHIA_OUTPUT_CAPABILITY_OBSERVE);
    view.release(Node::Transaction, Some(staging));
    let before = view.describe(&Node::Topology, None);
    let selected = view.admission().connection().selected_capabilities();
    assert_ne!(selected, 0);
    view.reset_authenticated_root(ConnectionId(2));
    assert_eq!(
        view.check(&root_access()),
        Ok(()),
        "another transport cannot reset this root"
    );
    view.reset_authenticated_root(ConnectionId(1));
    assert_eq!(view.check(&root_access()), Err(Errno::ESTALE));
    let next_auth = view.allocate_auth_qid().unwrap();
    assert!(next_auth > first_auth);
    view.prepare_authenticated_root(&claims(1))
        .unwrap()
        .publish();
    assert_eq!(
        view.admission().connection().selected_capabilities(),
        selected
    );
    assert_eq!(
        view.describe(&Node::Topology, None).qid_path,
        before.qid_path
    );
    assert_eq!(view.check(&root_access()), Ok(()));
    let mut replacement = pending(qids);
    assert!(replacement.allocate_auth_qid().unwrap() > next_auth);
}
