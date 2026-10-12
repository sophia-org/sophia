//! Supplied authority isolates reactor construction, polling and retirement.
//! Real Factotum authentication belongs to the Session/client join.
use super::*;
use crate::authenticated_files::FileAuthentication;
use sophia_9p::{
    AttachContext, ConnectionId, Errno, Export, Fid, ReadOutcome, authentication::Attempt,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Default)]
struct State {
    revoked: AtomicBool,
    polls: AtomicUsize,
    closed: AtomicUsize,
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
    fn close(&mut self, _: Instant) {
        self.0.closed.fetch_add(1, Ordering::SeqCst);
    }
    fn poll(&mut self, _: Instant) -> Result<(), Errno> {
        self.0.polls.fetch_add(1, Ordering::SeqCst);
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
        Duration::from_millis(17)
    }
}
struct Factory {
    state: Arc<State>,
    refuse: bool,
}
impl FileAuthenticationFactory for Factory {
    fn create(
        &mut self,
        _: ConnectionId,
        _: Instant,
        _: sophia_9p::unix::Wake,
    ) -> Result<Box<dyn FileAuthentication>, Errno> {
        if self.refuse {
            Err(Errno::EACCES)
        } else {
            Ok(Box::new(Owner(self.state.clone())))
        }
    }
    fn retire(
        &mut self,
        owner: Box<dyn FileAuthentication>,
    ) -> Result<(), Box<dyn FileAuthentication>> {
        assert!(!owner.drained());
        self.state.retired.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
fn export() -> ShellFiles {
    ShellFiles::awaiting_negotiation(
        1,
        "descriptor",
        false,
        journal_bounds(DESCRIPTOR_RECORD_BYTES),
        100,
        Instant::now(),
    )
}

#[test]
fn shell_reactor_refuses_missing_or_failed_authentication_factory() {
    for missing in [true, false] {
        let (local, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let factory = (!missing).then(|| {
            Box::new(Factory {
                state: Arc::default(),
                refuse: true,
            }) as Box<dyn FileAuthenticationFactory>
        });
        assert!(
            ShellFileWire::adopt(local, export(), factory, true).is_err(),
            "required shell authentication must never fall back"
        );
        use std::io::Read;
        assert_eq!(peer.read(&mut [0]).unwrap(), 0);
    }
}

#[test]
fn shell_reactor_polls_idle_authority_and_hands_closed_custody_back_once() {
    let state = Arc::new(State::default());
    let (local, _peer) = UnixStream::pair().unwrap();
    let factory = Box::new(Factory {
        state: state.clone(),
        refuse: false,
    });
    let mut wire = ShellFileWire::adopt(local, export(), Some(factory), true).unwrap();
    assert!(wire.server.export().requires_authentication());
    assert!(
        wire.server
            .export_mut()
            .attach(&AttachContext {
                connection: ConnectionId(1),
                peer: None,
                uname: b"shell",
                aname: b"descriptor",
                n_uname: u32::MAX
            })
            .is_err(),
        "shell root requires a verified afid"
    );
    assert_eq!(
        wire.authentication_wait(Instant::now()),
        Duration::from_millis(17)
    );
    wire.turn().unwrap();
    assert!(state.polls.load(Ordering::SeqCst) >= 2);
    state.revoked.store(true, Ordering::SeqCst);
    assert_eq!(
        wire.turn(),
        Err(ShellTransportError::NotConnected),
        "idle shell revocation must stop the reactor"
    );
    drop(wire);
    assert_eq!(state.closed.load(Ordering::SeqCst), 1);
    assert_eq!(
        state.retired.load(Ordering::SeqCst),
        1,
        "shell cleanup must return to its Session owner"
    );
}
