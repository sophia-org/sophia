//! t293 review: the session lock's authenticator is supervised. It is
//! reported available only once an agent has answered, every attempt made
//! while none is up is answered undecided, and a failed or vanished agent is
//! replaced rather than leaving the lock without a way to open it.
#![cfg(feature = "native-session")]

use sophia_engine::SessionLockEpoch;
use sophia_session::session_lock::{SessionUnlockAttempt, SessionUnlockVerdict};
use sophia_session::session_lock_input::SessionUnlockAuthenticator;
use sophia_session::session_unlock_supervisor::{
    SupervisedAuthenticator, UnlockAgent, UnlockAgentLauncher, UnlockSupervision,
};
use std::collections::VecDeque;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What one launch does, and then what its agent does.
#[derive(Clone, Debug)]
enum Launch {
    Refused,
    Agent(Vec<Login>),
}

#[derive(Clone, Copy, Debug)]
enum Login {
    Accept,
    Refuse,
    Break,
    /// The agent is found gone while idle instead of logging in.
    Vanish,
    Pending,
}

#[derive(Default)]
struct Record {
    launches: usize,
    dropped: usize,
    secrets: Vec<Vec<u8>>,
    polls: usize,
}

struct Launcher {
    script: VecDeque<Launch>,
    record: Arc<Mutex<Record>>,
}

struct Agent {
    ingress: sophia_factotum::launch::queue::Ingress,
    _queue: sophia_factotum::launch::queue::Owner,
    logins: VecDeque<Login>,
    record: Arc<Mutex<Record>>,
    pending: Option<Login>,
}

impl UnlockAgentLauncher for Launcher {
    type Agent = Agent;

    fn launch(&mut self) -> Result<Agent, String> {
        self.record.lock().unwrap().launches += 1;
        match self.script.pop_front() {
            Some(Launch::Agent(logins)) => {
                let (ingress, queue) = sophia_factotum::launch::queue::channel();
                Ok(Agent {
                    ingress,
                    _queue: queue,
                    logins: logins.into(),
                    record: Arc::clone(&self.record),
                    pending: None,
                })
            }
            Some(Launch::Refused) | None => Err("refused".into()),
        }
    }
}

impl UnlockAgent for Agent {
    fn launch_ingress(&self) -> Option<sophia_factotum::launch::queue::Ingress> {
        Some(self.ingress.clone())
    }
    fn begin_login(&mut self, secret: &[u8]) -> Result<(), String> {
        self.record.lock().unwrap().secrets.push(secret.to_vec());
        self.pending = Some(self.logins.pop_front().ok_or("no scripted login")?);
        Ok(())
    }

    fn poll_login(&mut self) -> Result<Option<bool>, String> {
        self.record.lock().unwrap().polls += 1;
        if matches!(self.pending, Some(Login::Pending)) {
            return Ok(None);
        }
        match self.pending.take() {
            Some(Login::Accept) => Ok(Some(true)),
            Some(Login::Refuse) => Ok(Some(false)),
            Some(Login::Break | Login::Vanish) => Err("broken".into()),
            None => Ok(None),
            Some(Login::Pending) => unreachable!(),
        }
    }

    fn gone(&mut self) -> bool {
        matches!(self.logins.front(), Some(Login::Vanish))
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        self.record.lock().unwrap().dropped += 1;
    }
}

const QUICK: UnlockSupervision = UnlockSupervision {
    first_restart: Duration::from_millis(30),
    longest_restart: Duration::from_millis(120),
    liveness: Duration::from_millis(5),
};

fn start(
    script: Vec<Launch>,
) -> (
    SupervisedAuthenticator,
    Arc<Mutex<Record>>,
    Arc<AtomicUsize>,
) {
    let record = Arc::new(Mutex::new(Record::default()));
    let wakes = Arc::new(AtomicUsize::new(0));
    let rung = Arc::clone(&wakes);
    let authenticator = SupervisedAuthenticator::start(
        Launcher {
            script: script.into(),
            record: Arc::clone(&record),
        },
        QUICK,
        move || {
            rung.fetch_add(1, Ordering::SeqCst);
        },
    )
    .unwrap();
    (authenticator, record, wakes)
}

fn attempt(serial: u64) -> SessionUnlockAttempt {
    SessionUnlockAttempt {
        epoch: SessionLockEpoch::FIRST,
        serial: NonZeroU64::new(serial).unwrap(),
    }
}

fn until(mut done: impl FnMut() -> bool, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting: {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn verdict(
    authenticator: &mut SupervisedAuthenticator,
) -> (SessionUnlockAttempt, SessionUnlockVerdict) {
    let mut verdict = None;
    until(
        || {
            verdict = authenticator.poll();
            verdict.is_some()
        },
        "a verdict",
    );
    verdict.unwrap()
}

#[test]
fn verdicts_follow_the_agent_and_ring_the_owner() {
    let (mut authenticator, record, wakes) =
        start(vec![Launch::Agent(vec![Login::Refuse, Login::Accept])]);
    until(|| authenticator.available(), "the first agent");
    authenticator.begin(attempt(1), "wrong").unwrap();
    assert_eq!(
        verdict(&mut authenticator),
        (attempt(1), SessionUnlockVerdict::Rejected)
    );
    authenticator.begin(attempt(2), "right").unwrap();
    assert_eq!(
        verdict(&mut authenticator),
        (attempt(2), SessionUnlockVerdict::Accepted)
    );
    assert_eq!(wakes.load(Ordering::SeqCst), 2);
    assert_eq!(
        record.lock().unwrap().secrets,
        [b"wrong".to_vec(), b"right".to_vec()]
    );
    assert_eq!(record.lock().unwrap().launches, 1);
}

#[test]
fn nothing_is_available_until_an_agent_answers_and_waiting_attempts_are_undecided() {
    let (mut authenticator, record, _) = start(vec![
        Launch::Refused,
        Launch::Refused,
        Launch::Agent(vec![Login::Accept]),
    ]);
    until(|| record.lock().unwrap().launches >= 1, "the first launch");
    assert!(!authenticator.available());
    authenticator.begin(attempt(1), "early").unwrap();
    assert_eq!(
        verdict(&mut authenticator),
        (attempt(1), SessionUnlockVerdict::Unavailable)
    );
    until(|| authenticator.available(), "the third launch");
    assert_eq!(record.lock().unwrap().launches, 3);
    assert!(
        record.lock().unwrap().secrets.is_empty(),
        "no agent saw the early secret"
    );
    authenticator.begin(attempt(2), "late").unwrap();
    assert_eq!(
        verdict(&mut authenticator),
        (attempt(2), SessionUnlockVerdict::Accepted)
    );
}

#[test]
fn a_broken_agent_decides_nothing_and_is_replaced() {
    let (mut authenticator, record, _) = start(vec![
        Launch::Agent(vec![Login::Break]),
        Launch::Agent(vec![Login::Accept]),
    ]);
    until(|| authenticator.available(), "the first agent");
    authenticator.begin(attempt(1), "secret").unwrap();
    assert_eq!(
        verdict(&mut authenticator),
        (attempt(1), SessionUnlockVerdict::Unavailable)
    );
    until(
        || record.lock().unwrap().dropped == 1,
        "the broken agent retired",
    );
    until(
        || record.lock().unwrap().launches == 2 && authenticator.available(),
        "the replacement",
    );
    authenticator.begin(attempt(2), "secret").unwrap();
    assert_eq!(
        verdict(&mut authenticator),
        (attempt(2), SessionUnlockVerdict::Accepted)
    );
}

#[test]
fn an_agent_gone_while_idle_is_replaced_before_anyone_asks() {
    let (authenticator, record, _) = start(vec![
        Launch::Agent(vec![Login::Vanish]),
        Launch::Agent(vec![Login::Accept]),
    ]);
    until(
        || record.lock().unwrap().launches == 2 && authenticator.available(),
        "the replacement",
    );
    assert_eq!(record.lock().unwrap().dropped, 1);
    assert!(record.lock().unwrap().secrets.is_empty());
}

#[test]
fn replacements_back_off_and_are_bounded() {
    // Five refusals: waits of 30, 60, 120, 120 and 120 ms at the least.
    let started = Instant::now();
    let (authenticator, record, _) = start(vec![
        Launch::Refused,
        Launch::Refused,
        Launch::Refused,
        Launch::Refused,
        Launch::Refused,
        Launch::Agent(vec![]),
    ]);
    until(|| authenticator.available(), "the sixth launch");
    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_millis(450),
        "backed off for {elapsed:?}"
    );
    assert_eq!(record.lock().unwrap().launches, 6);
}

#[test]
fn dropping_the_authenticator_ends_the_agent() {
    let (authenticator, record, _) = start(vec![Launch::Agent(vec![])]);
    until(|| authenticator.available(), "the agent");
    drop(authenticator);
    until(
        || record.lock().unwrap().dropped == 1,
        "the agent ended with Session",
    );
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(record.lock().unwrap().launches, 1, "and is not replaced");
}

#[test]
fn a_waiting_login_keeps_turning_and_owner_drop_cancels_without_a_verdict() {
    let (mut authenticator, record, _) = start(vec![Launch::Agent(vec![Login::Pending])]);
    until(|| authenticator.available(), "the agent");
    authenticator.begin(attempt(1), "pending").unwrap();
    until(
        || record.lock().unwrap().secrets.len() == 1,
        "pending login admitted",
    );
    let before = record.lock().unwrap().polls;
    until(
        || record.lock().unwrap().polls >= before + 3,
        "a pending PAM login must not block the shared connection owner",
    );
    assert!(authenticator.poll().is_none());
    authenticator.begin(attempt(2), "busy").unwrap();
    assert!(
        authenticator.begin(attempt(3), "no-slot").is_err(),
        "active and undelivered attempts must retain their result reservations"
    );
    assert_eq!(
        verdict(&mut authenticator),
        (attempt(2), SessionUnlockVerdict::Unavailable)
    );
    assert_eq!(
        record.lock().unwrap().secrets,
        [b"pending".to_vec()],
        "a busy owner must not start a second PAM conversation"
    );
    drop(authenticator);
    until(
        || record.lock().unwrap().dropped == 1,
        "dropping Session must cancel a pending login without waiting for PAM",
    );
    assert_eq!(record.lock().unwrap().launches, 1);
}

#[test]
fn undelivered_verdicts_bound_admission_until_session_consumes_them() {
    let (mut authenticator, _, wakes) = start(vec![Launch::Agent(vec![
        Login::Refuse,
        Login::Refuse,
        Login::Accept,
    ])]);
    until(|| authenticator.available(), "the agent");
    assert!(authenticator.begin(attempt(99), &"x".repeat(1025)).is_err());
    authenticator.begin(attempt(1), "first").unwrap();
    until(
        || wakes.load(Ordering::SeqCst) == 1,
        "first retained result",
    );
    authenticator.begin(attempt(2), "second").unwrap();
    until(
        || wakes.load(Ordering::SeqCst) == 2,
        "second retained result",
    );
    assert!(
        authenticator.begin(attempt(3), "third").is_err(),
        "undelivered verdicts must retain their admission slots"
    );
    assert_eq!(
        verdict(&mut authenticator),
        (attempt(1), SessionUnlockVerdict::Rejected)
    );
    authenticator.begin(attempt(3), "third").unwrap();
    assert_eq!(
        verdict(&mut authenticator),
        (attempt(2), SessionUnlockVerdict::Rejected)
    );
    assert_eq!(
        verdict(&mut authenticator),
        (attempt(3), SessionUnlockVerdict::Accepted)
    );
}

#[test]
fn replacement_publishes_fresh_launch_ingress_and_closes_old_tickets() {
    use sophia_factotum::launch::{queue::Error, wire::Command};
    let (mut authenticator, _, _) = start(vec![
        Launch::Agent(vec![Login::Break]),
        Launch::Agent(vec![Login::Pending]),
    ]);
    let service = authenticator.launch_service();
    let mut first = None;
    until(
        || {
            first = service.current();
            first.is_some()
        },
        "first launch service",
    );
    let first = first.unwrap();
    let now = Instant::now();
    let mut old = first
        .ingress
        .submit(
            Command::Initialize { session: 1 },
            now,
            now + Duration::from_secs(5),
        )
        .unwrap();
    authenticator.begin(attempt(1), "break").unwrap();
    assert_eq!(
        verdict(&mut authenticator).1,
        SessionUnlockVerdict::Unavailable
    );
    let mut next = None;
    until(
        || {
            next = service
                .current()
                .filter(|c| c.generation > first.generation);
            next.is_some()
        },
        "replacement launch service",
    );
    assert!(
        matches!(old.take(Instant::now()), Err(Error::Closed)),
        "replacing Factotum must invalidate the previous launch result"
    );
    assert!(!first.ingress.is_open());
    let next = next.unwrap();
    assert!(next.ingress.is_open());
    drop(authenticator);
    assert!(
        service.current().is_none() && !next.ingress.is_open(),
        "owner drop must withdraw launch authority before the worker next turns"
    );
}

#[test]
fn dropping_during_startup_cannot_publish_a_late_launch_service() {
    use std::sync::mpsc;
    struct Delayed {
        started: mpsc::SyncSender<()>,
        resume: mpsc::Receiver<()>,
        inner: Launcher,
    }
    impl UnlockAgentLauncher for Delayed {
        type Agent = Agent;
        fn launch(&mut self) -> Result<Agent, String> {
            self.started.send(()).unwrap();
            self.resume.recv().unwrap();
            self.inner.launch()
        }
    }
    let (started, start) = mpsc::sync_channel(1);
    let (resume, wait) = mpsc::sync_channel(1);
    let record = Arc::new(Mutex::new(Record::default()));
    let authenticator = SupervisedAuthenticator::start(
        Delayed {
            started,
            resume: wait,
            inner: Launcher {
                script: vec![Launch::Agent(vec![])].into(),
                record: record.clone(),
            },
        },
        QUICK,
        || {},
    )
    .unwrap();
    let service = authenticator.launch_service();
    start.recv_timeout(Duration::from_secs(2)).unwrap();
    drop(authenticator);
    resume.send(()).unwrap();
    until(|| record.lock().unwrap().dropped == 1, "late agent retired");
    assert!(
        service.current().is_none(),
        "closed Session cannot republish late launch authority"
    );
}
