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
}

#[derive(Default)]
struct Record {
    launches: usize,
    dropped: usize,
    secrets: Vec<Vec<u8>>,
}

struct Launcher {
    script: VecDeque<Launch>,
    record: Arc<Mutex<Record>>,
}

struct Agent {
    logins: VecDeque<Login>,
    record: Arc<Mutex<Record>>,
}

impl UnlockAgentLauncher for Launcher {
    type Agent = Agent;

    fn launch(&mut self) -> Result<Agent, String> {
        self.record.lock().unwrap().launches += 1;
        match self.script.pop_front() {
            Some(Launch::Agent(logins)) => Ok(Agent {
                logins: logins.into(),
                record: Arc::clone(&self.record),
            }),
            Some(Launch::Refused) | None => Err("refused".into()),
        }
    }
}

impl UnlockAgent for Agent {
    fn login(&mut self, secret: &[u8]) -> Result<bool, String> {
        self.record.lock().unwrap().secrets.push(secret.to_vec());
        match self.logins.pop_front() {
            Some(Login::Accept) => Ok(true),
            Some(Login::Refuse) => Ok(false),
            Some(Login::Break | Login::Vanish) | None => Err("broken".into()),
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
