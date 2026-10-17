use sophia_9p::{Errno, connection::ConnectionId, export::*, records::OpenFlags};
use sophia_protocol::broker_files::*;
use sophia_runtime::{
    ProtectionBackendKind, ProtectionDomainEvidence, ProtectionDomainRole, broker_files::*,
};
struct Owner;
impl BrokerFileEffects for Owner {
    type Prepared = ();
    fn prepare(&mut self, _: &BrokerFileResponseSet<'_>) -> Result<(), Errno> {
        Ok(())
    }
    fn commit(&mut self, _: ()) {}
}
fn evidence() -> ProtectionDomainEvidence {
    ProtectionDomainEvidence {
        backend: ProtectionBackendKind::Bubblewrap,
        supervisor_pid: 1,
        peer_pid: 42,
        roles: [ProtectionDomainRole::MetadataBroker].into_iter().collect(),
    }
}
fn peer() -> PeerCredentials {
    PeerCredentials {
        pid: 42,
        uid: 1000,
        gid: 1000,
    }
}
fn export() -> BrokerFileExport<Owner> {
    BrokerFileExport::new(
        Owner,
        ConnectionId(1),
        peer(),
        &evidence(),
        1000,
        7,
        1,
        0,
        BrokerFileLimits::default(),
    )
    .unwrap()
}
fn context() -> AttachContext<'static> {
    AttachContext {
        connection: ConnectionId(1),
        peer: Some(peer()),
        uname: b"",
        aname: b"",
        n_uname: 1000,
    }
}
#[test]
fn protected_evidence_exact_peer_and_single_attach_are_required() {
    let mut missing = evidence();
    missing.roles.clear();
    assert!(matches!(
        BrokerFileExport::new(
            Owner,
            ConnectionId(1),
            peer(),
            &missing,
            1000,
            7,
            1,
            0,
            BrokerFileLimits::default()
        ),
        Err(Errno::EACCES)
    ));
    let mut e = export();
    let mut wrong = context();
    wrong.peer.as_mut().unwrap().pid = 43;
    assert!(matches!(e.attach(&wrong), Err(Errno::EACCES)));
    e.attach(&context()).unwrap();
    assert!(matches!(e.attach(&context()), Err(Errno::EACCES)));
    e.connection_closed(ConnectionId(1));
    assert!(e.custody().closed());
}
#[test]
fn transaction_is_single_bounded_immutable_and_not_reused() {
    use BrokerFileNode as N;
    let mut e = export();
    e.attach(&context()).unwrap();
    let mut tx = e.open(&N::Transaction, OpenFlags(2)).unwrap();
    let qid = e.describe(&N::Transaction, Some(&tx)).qid_path;
    assert!(matches!(
        e.open(&N::Transaction, OpenFlags(2)),
        Err(Errno(16))
    ));
    let mut bytes = [0; 40];
    bytes[0] = 40;
    bytes[4] = 1;
    bytes[7] = 1;
    bytes[8] = 7;
    bytes[16] = 1;
    bytes[32] = 2;
    bytes[34] = 2;
    assert_eq!(
        e.write(&N::Transaction, &mut tx, 1, &bytes),
        Err(Errno::EINVAL)
    );
    e.write(&N::Transaction, &mut tx, 0, &bytes[..3]).unwrap();
    e.write(&N::Transaction, &mut tx, 3, &bytes[3..]).unwrap();
    e.write(&N::Transaction, &mut tx, 0, &bytes).unwrap();
    let mut changed = bytes;
    changed[32] = 1;
    assert_eq!(
        e.write(&N::Transaction, &mut tx, 0, &changed),
        Err(Errno::EINVAL)
    );
    let mut submit = e.open(&N::Submit, OpenFlags(1)).unwrap();
    let mut control = [0; 24];
    control[0] = 7;
    control[8] = 1;
    control[16] = 40;
    e.write(&N::Submit, &mut submit, 0, &control).unwrap();
    e.write(&N::Submit, &mut submit, 0, &control).unwrap();
    assert_eq!(
        e.write(&N::Transaction, &mut tx, 0, &bytes),
        Err(Errno::EACCES)
    );
    assert_eq!(
        e.read(&N::Transaction, &mut tx, 0, 4096).unwrap(),
        ReadOutcome::Ready(bytes.to_vec())
    );
    let mut ack = e.open(&N::Ack, OpenFlags(1)).unwrap();
    let mut a = [0; 16];
    a[0] = 7;
    a[8] = 2;
    e.write(&N::Ack, &mut ack, 0, &a).unwrap();
    assert_eq!(e.read(&N::Transaction, &mut tx, 0, 1), Err(Errno::ESTALE));
    e.release(N::Transaction, Some(tx));
    let next = e.open(&N::Transaction, OpenFlags(2)).unwrap();
    assert!(e.describe(&N::Transaction, Some(&next)).qid_path > qid);
}
