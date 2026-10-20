//! Supplied authority proves the real shell reactor's scope and parked-read
//! wiring. Session controls separately exercise the actual custody owner.
use super::*;
use crate::authenticated_files::FileAuthentication;
use sophia_9p::{AttachContext, ConnectionId, Errno, Fid, ReadOutcome, authentication::Attempt};
use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct State {
    scoped: bool,
    begins: usize,
    ends: usize,
    polls: Vec<bool>,
    checks: usize,
    deny_checks: bool,
    fail_begin: bool,
    panic_begin: bool,
    fail_poll: bool,
    fail_final_poll: bool,
    panic_poll: bool,
}
struct Owner {
    state: Arc<Mutex<State>>,
    admitted: bool,
}
impl FileAuthentication for Owner {
    fn begin_custody_dispatch(&mut self) -> Result<(), Errno> {
        let mut state = self.state.lock().unwrap();
        assert!(!state.scoped, "the previous service scope escaped");
        state.scoped = true;
        state.begins += 1;
        let (fail, panic) = (state.fail_begin, state.panic_begin);
        drop(state);
        assert!(!panic, "injected begin failure");
        if fail { Err(Errno::ESTALE) } else { Ok(()) }
    }
    fn end_custody_dispatch(&mut self) {
        let mut state = self.state.lock().unwrap();
        assert!(state.scoped, "scope ended twice");
        state.scoped = false;
        state.ends += 1;
    }
    fn claims(&self, _: &AttachContext<'_>) -> Result<(), Errno> {
        Ok(())
    }
    fn begin(&mut self, afid: Fid, _: Instant) -> Result<Attempt, Errno> {
        Ok(Attempt {
            server: 1,
            connection: ConnectionId(1),
            session: 1,
            audience: 1,
            wire_generation: 1,
            attempt_generation: 1,
            afid: afid.0,
        })
    }
    fn read(&mut self, _: Attempt, _: u64, _: u32, _: Instant) -> Result<ReadOutcome, Errno> {
        Err(Errno::EACCES)
    }
    fn commit(&mut self, _: Attempt, _: Instant) -> Result<(), Errno> {
        self.admitted = true;
        Ok(())
    }
    fn check(&mut self, _: ConnectionId, _: Instant) -> Result<(), Errno> {
        let mut state = self.state.lock().unwrap();
        assert!(
            state.scoped,
            "request and parked-read checks must enter the scope"
        );
        state.checks += 1;
        if state.deny_checks {
            Err(Errno::ESTALE)
        } else {
            Ok(())
        }
    }
    fn release(&mut self, _: Attempt, _: Instant) {}
    fn reset(&mut self, _: Instant) -> Result<(), Errno> {
        Ok(())
    }
    fn close(&mut self, _: Instant) {}
    fn poll(&mut self, _: Instant) -> Result<(), Errno> {
        let (fail, panic) = {
            let mut state = self.state.lock().unwrap();
            let scoped = state.scoped;
            state.polls.push(scoped);
            (
                state.fail_poll || (!scoped && state.fail_final_poll),
                state.panic_poll,
            )
        };
        assert!(!panic, "injected owner failure");
        if fail { Err(Errno::ESTALE) } else { Ok(()) }
    }
    fn drained(&self) -> bool {
        true
    }
    fn admitted(&self) -> bool {
        self.admitted
    }
    fn admission_context(&self) -> Option<sophia_protocol::ClientAdmissionContext> {
        None
    }
    fn wait(&self, _: Instant) -> Duration {
        Duration::from_millis(20)
    }
}
struct Factory(Arc<Mutex<State>>);
impl FileAuthenticationFactory for Factory {
    fn create(
        &mut self,
        _: ConnectionId,
        _: Instant,
        _: sophia_9p::unix::Wake,
    ) -> Result<Box<dyn FileAuthentication>, Errno> {
        Ok(Box::new(Owner {
            state: self.0.clone(),
            admitted: false,
        }))
    }
}
fn fixture() -> (ShellFileWire, UnixStream, Arc<Mutex<State>>) {
    let state = Arc::new(Mutex::new(State::default()));
    let (local, peer) = UnixStream::pair().unwrap();
    let files = ShellFiles::awaiting_negotiation(
        1,
        "descriptor",
        false,
        journal_bounds(DESCRIPTOR_RECORD_BYTES),
        100,
        Instant::now(),
    );
    let wire =
        ShellFileWire::adopt(local, files, Some(Box::new(Factory(state.clone()))), true).unwrap();
    (wire, peer, state)
}
#[test]
fn scope_closes_before_final_poll_and_on_error_and_unwind() {
    for failure in 0..6 {
        let (mut wire, _peer, state) = fixture();
        {
            let mut state = state.lock().unwrap();
            state.fail_begin = failure == 1;
            state.fail_poll = failure == 2;
            state.panic_poll = failure == 3;
            state.fail_final_poll = failure == 4;
            state.panic_begin = failure == 5;
        }
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| wire.turn()));
        let state = state.lock().unwrap();
        assert!(!state.scoped);
        assert_eq!((state.begins, state.ends), (1, 1));
        match failure {
            0 => {
                outcome.unwrap().unwrap();
                assert_eq!(state.polls, [true, false]);
            }
            1 | 2 | 4 => assert!(outcome.unwrap().is_err()),
            3 | 5 => assert!(outcome.is_err()),
            _ => unreachable!(),
        }
    }
}
fn reply(wire: &mut ShellFileWire, client: &mut Pipeline) -> Reply {
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        client.poll().unwrap();
        wire.turn().unwrap();
        client.poll().unwrap();
        if let Some((_, reply)) = client.take_reply() {
            return reply;
        }
        assert!(Instant::now() < end);
    }
}
#[test]
fn requests_and_parked_reads_recheck_permission_in_each_service_scope() {
    let (mut wire, peer, state) = fixture();
    let thread = std::thread::spawn(move || {
        Pipeline::over(peer, PipelineLimits::default(), Duration::from_secs(2)).unwrap()
    });
    let end = Instant::now() + Duration::from_secs(2);
    while !thread.is_finished() {
        wire.turn().unwrap();
        assert!(Instant::now() < end);
        std::thread::yield_now();
    }
    let mut client = thread.join().unwrap();
    let (_, afid) = client.auth(b"shell", b"descriptor").unwrap();
    assert!(matches!(reply(&mut wire, &mut client), Reply::Auth(_)));
    let (_, root) = client
        .attach_with_afid(afid, b"shell", b"descriptor")
        .unwrap();
    assert!(matches!(reply(&mut wire, &mut client), Reply::Attach(_)));
    let (_, events) = client.walk(root, &[b"events"]).unwrap();
    assert!(matches!(reply(&mut wire, &mut client), Reply::Walk(_)));
    client.lopen(events, 0).unwrap();
    assert!(matches!(reply(&mut wire, &mut client), Reply::Lopen { .. }));
    client.read(events, 0, 4096).unwrap();
    client.poll().unwrap();
    wire.turn().unwrap();
    client.poll().unwrap();
    assert!(
        client.take_reply().is_none(),
        "empty journal read must park"
    );
    let checks = state.lock().unwrap().checks;
    wire.flush().unwrap();
    assert!(
        state.lock().unwrap().checks > checks,
        "flush must recheck the parked read"
    );
    client.poll().unwrap();
    assert!(client.take_reply().is_none());
    // No new peer input: a local permission change must refuse the parked read.
    state.lock().unwrap().deny_checks = true;
    wire.flush().unwrap();
    client.poll().unwrap();
    assert!(matches!(
        client.take_reply(),
        Some((_, Reply::Error(Errno::ESTALE)))
    ));
    assert!(!state.lock().unwrap().scoped);
}
