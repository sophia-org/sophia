//! Root and worker ownership with supplied authentication. Real launch-channel
//! verification remains Session's responsibility and is joined separately.
#![cfg(test)]
use super::*;
use crate::authenticated_files::{
    FileAuthentication, FileAuthenticationFactory, PreparedFileRoot, ProtectedFiles,
};
use sophia_9p::{
    AttachContext, ConnectionId, Errno, Fid,
    authentication::Attempt,
    export::{Access, Epoch, Operation},
};
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::time::Instant;

fn claims(id: u64) -> AttachContext<'static> {
    AttachContext {
        connection: ConnectionId(id),
        peer: None,
        uname: b"observer",
        aname: b"inspection",
        n_uname: u32::MAX,
    }
}
#[test]
fn observer_root_requires_authentication_and_reserves_before_publication() {
    let (shared, _, _wake) = setup();
    let mut files = InspectionExport::authenticated(shared.clone(), 2).unwrap();
    files.bind_connection(ConnectionId(1));
    assert!(
        matches!(files.attach(&claims(1)), Err(Errno::EACCES)),
        "observer export cannot fall back to plain attach"
    );
    assert!(matches!(
        files.prepare_authenticated_root(&claims(2)),
        Err(Errno::EACCES)
    ));
    let access = Access {
        connection: ConnectionId(1),
        epoch: Epoch(2),
        node: &Node::Root,
        operation: Operation::Read,
    };
    {
        let _abandoned = files.prepare_authenticated_root(&claims(1)).unwrap();
    }
    assert_eq!(files.check(&access), Err(Errno::ESTALE));
    files
        .prepare_authenticated_root(&claims(1))
        .unwrap()
        .publish();
    assert_eq!(files.check(&access), Ok(()));
    files.reset_authenticated_root(ConnectionId(2));
    assert_eq!(files.check(&access), Ok(()));
    files.reset_authenticated_root(ConnectionId(1));
    assert_eq!(files.check(&access), Err(Errno::ESTALE));
    files
        .prepare_authenticated_root(&claims(1))
        .unwrap()
        .publish();
    shared.fence(10, None).unwrap();
    assert_eq!(
        files.check(&access),
        Err(Errno::ESTALE),
        "source replacement must fence a retained observer root"
    );
    assert!(matches!(
        files.prepare_authenticated_root(&claims(1)),
        Err(Errno::ESTALE)
    ));
}
#[derive(Default)]
struct State {
    revoked: AtomicBool,
    retired: AtomicUsize,
}
struct Owner(Arc<State>);
impl FileAuthentication for Owner {
    fn claims(&self, _: &AttachContext<'_>) -> Result<(), Errno> {
        Err(Errno::EACCES)
    }
    fn begin(&mut self, _: Fid, _: Instant) -> Result<Attempt, Errno> {
        Err(Errno::EACCES)
    }
    fn read(&mut self, _: Attempt, _: u64, _: u32, _: Instant) -> Result<ReadOutcome, Errno> {
        Err(Errno::EACCES)
    }
    fn commit(&mut self, _: Attempt, _: Instant) -> Result<(), Errno> {
        Err(Errno::EACCES)
    }
    fn check(&mut self, _: ConnectionId, now: Instant) -> Result<(), Errno> {
        self.poll(now)
    }
    fn release(&mut self, _: Attempt, _: Instant) {}
    fn reset(&mut self, _: Instant) -> Result<(), Errno> {
        Ok(())
    }
    fn close(&mut self, _: Instant) {}
    fn poll(&mut self, _: Instant) -> Result<(), Errno> {
        if self.0.revoked.load(Ordering::SeqCst) {
            Err(Errno::ESTALE)
        } else {
            Ok(())
        }
    }
    fn drained(&self) -> bool {
        false
    }
    fn admitted(&self) -> bool {
        false
    }
    fn admission_context(&self) -> Option<sophia_protocol::ClientAdmissionContext> {
        None
    }
    fn wait(&self, _: Instant) -> Duration {
        Duration::from_millis(20)
    }
}
struct Factory(Arc<State>, bool);
impl FileAuthenticationFactory for Factory {
    fn create(
        &mut self,
        _: ConnectionId,
        _: Instant,
        _: sophia_9p::unix::Wake,
    ) -> Result<Box<dyn FileAuthentication>, Errno> {
        if self.1 {
            Err(Errno::EACCES)
        } else {
            Ok(Box::new(Owner(self.0.clone())))
        }
    }
    fn retire(
        &mut self,
        _: Box<dyn FileAuthentication>,
    ) -> Result<(), Box<dyn FileAuthentication>> {
        self.0.retired.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
#[test]
fn old_endpoint_generation_cannot_adopt_replacement_source() {
    let mut service = InspectionService::private().unwrap();
    assert!(service.generation().is_none());
    service.fence(9, None).unwrap();
    let old = service.generation().unwrap();
    service.fence(10, None).unwrap();
    let new = service.generation().unwrap();
    assert!(new > old);
    let (local, _peer) = UnixStream::pair().unwrap();
    assert!(
        service
            .adopt_authenticated(local, old, Box::new(Factory(Arc::default(), false)))
            .is_err(),
        "an old namespace endpoint cannot adopt replacement inspection data"
    );
    let (local, _peer) = UnixStream::pair().unwrap();
    let mut connection = service
        .adopt_authenticated(local, new, Box::new(Factory(Arc::default(), false)))
        .unwrap();
    assert!(connection.poll().unwrap());
}

#[test]
fn private_inspection_has_no_host_endpoint_and_bounds_authenticated_connections() {
    let mut service = InspectionService::private().unwrap();
    assert!(service.socket_path().is_none());
    assert!(service.is_running());
    service.fence(9, None).unwrap();
    service.publisher().publish(value(), None).unwrap();
    let state = Arc::new(State::default());
    for _ in 0..8 {
        let (local, mut peer) = UnixStream::pair().unwrap();
        assert!(
            service
                .adopt_authenticated(local, 2, Box::new(Factory(state.clone(), true)))
                .is_err()
        );
        assert_eq!(peer.read(&mut [0]).unwrap(), 0);
    }
    let mut connections = Vec::new();
    let mut peers = Vec::new();
    for _ in 0..4 {
        let (local, peer) = UnixStream::pair().unwrap();
        connections.push(
            service
                .adopt_authenticated(local, 2, Box::new(Factory(state.clone(), false)))
                .unwrap(),
        );
        peers.push(peer);
    }
    let (local, _peer) = UnixStream::pair().unwrap();
    assert!(
        service
            .adopt_authenticated(local, 2, Box::new(Factory(state.clone(), false)))
            .is_err()
    );
    peers[0].write_all(&wire::version()).unwrap();
    assert!(connections[0].poll().unwrap());
    assert_eq!(wire::receive(&mut peers[0]).unwrap()[4], 101);
    peers[0].write_all(&wire::attach()).unwrap();
    assert!(connections[0].poll().unwrap());
    assert_eq!(
        wire::errno(&wire::receive(&mut peers[0]).unwrap()),
        13,
        "private observer must require an afid"
    );
    state.revoked.store(true, Ordering::SeqCst);
    for connection in &mut connections {
        assert!(!connection.poll().unwrap());
    }
    assert_eq!(state.retired.load(Ordering::SeqCst), 4);
    connections.clear();
    assert_eq!(
        state.retired.load(Ordering::SeqCst),
        4,
        "retirement is handed off once"
    );
    let (local, _peer) = UnixStream::pair().unwrap();
    let mut other = service
        .adopt_authenticated(local, 2, Box::new(Factory(Arc::default(), false)))
        .unwrap();
    assert!(other.poll().unwrap());
    drop(service);
    assert!(
        !other.poll().unwrap(),
        "dropping the owner fences every private observer"
    );
}
