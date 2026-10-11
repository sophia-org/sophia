//! Shared production client/core/export, with supplied Session admission and
//! bounded fixture PAM outcomes. No actual passwords, PAM helpers or devices.

use sophia_9p::{
    ConnectionId, Limits,
    authentication::{Attempt, Identity},
    unix::{Server, Wake},
};
use sophia_9p_client::pipeline::PipelineError;
use sophia_factotum::{
    client::{ClientError, LoginVerdict, SessionClient, UnlockClient},
    export::FactotumExport,
    jobs::{ConversationId, JobSink},
    launch::{
        Delegation,
        client::Error as LaunchError,
        wire::{Command, Response},
    },
    proto::{ChannelClass, Job, JobOutcome, PamVerdict, Settings},
};
use std::{
    os::unix::net::UnixStream,
    sync::{
        Arc, OnceLock,
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

const SECRET: &[u8] = b"supplied-fixture-secret";
#[path = "support/launch_owner.rs"]
mod launch_owner;
struct Jobs {
    wake: Arc<OnceLock<Wake>>,
    submitted: SyncSender<ConversationId>,
    cancelled: SyncSender<ConversationId>,
    automatic: Option<(PamVerdict, Sender<(ConversationId, JobOutcome)>)>,
}
impl JobSink for Jobs {
    fn submit(&mut self, id: ConversationId, job: Job) -> bool {
        let Job::Pam(request) = job;
        assert_eq!(request.user, "fixture");
        assert_eq!(request.service, "fixture");
        assert!(
            request.secret.as_slice() == SECRET,
            "PAM must receive the exact supplied secret"
        );
        if self.submitted.try_send(id).is_err() {
            return false;
        }
        if let Some((verdict, sender)) = &self.automatic {
            sender.send((id, JobOutcome::Pam(*verdict))).unwrap();
            self.wake.get().expect("server running").wake();
        }
        true
    }
    fn cancel(&mut self, id: ConversationId) {
        self.cancelled.try_send(id).unwrap();
    }
}

struct Served {
    wake: Wake,
    thread: Option<JoinHandle<()>>,
    jobs: Receiver<ConversationId>,
    cancelled: Receiver<ConversationId>,
    outcomes: Sender<(ConversationId, JobOutcome)>,
    closed: Receiver<()>,
}
impl Served {
    fn start(automatic: Option<PamVerdict>) -> (Self, SessionClient) {
        let (client, stream) = UnixStream::pair().unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        let (submitted, jobs) = mpsc::sync_channel(16);
        let (cancel_sender, cancelled) = mpsc::sync_channel(16);
        let (outcomes, outcome_receiver) = mpsc::channel();
        let auto = automatic.map(|verdict| (verdict, outcomes.clone()));
        let (closed_sender, closed) = mpsc::sync_channel(1);
        let thread = Some(std::thread::spawn(move || {
            let job_wake = Arc::new(OnceLock::new());
            let mut export = FactotumExport::new(Settings {
                owner: "fixture".into(),
                pam_service: "fixture".into(),
            });
            export.attach_jobs(
                Box::new(Jobs {
                    wake: Arc::clone(&job_wake),
                    submitted,
                    cancelled: cancel_sender,
                    automatic: auto,
                }),
                outcome_receiver,
            );
            let mut server = Server::new(export, Limits::default()).unwrap();
            assert!(job_wake.set(server.wake()).is_ok());
            let id = server.adopt(stream).unwrap_or_else(|_| panic!("adopt"));
            server.export_mut().admit(id, ChannelClass::Session);
            sender.send(server.wake()).unwrap();
            while server.turn(Some(Duration::from_secs(1))).unwrap() {
                if server.connection_count() == 0 {
                    closed_sender.send(()).unwrap();
                    break;
                }
            }
        }));
        let wake = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        let served = Self {
            wake,
            thread,
            jobs,
            cancelled,
            outcomes,
            closed,
        };
        let client = SessionClient::over(client, Duration::from_secs(2)).unwrap();
        (served, client)
    }

    fn finish(&self, job: ConversationId, verdict: PamVerdict) {
        self.outcomes.send((job, JobOutcome::Pam(verdict))).unwrap();
        self.wake.wake();
    }

    fn assert_closed_and_cancelled(&self, job: ConversationId) {
        self.closed
            .recv_timeout(Duration::from_secs(2))
            .expect("fatal authentication failure must close the actual transport");
        assert_eq!(
            self.cancelled.recv_timeout(Duration::from_secs(2)).unwrap(),
            job,
            "closing the shared owner must cancel the outstanding PAM conversation"
        );
    }
}
impl Drop for Served {
    fn drop(&mut self) {
        self.wake.stop();
        let result = self.thread.take().unwrap().join();
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}
fn tick(client: &mut SessionClient, end: Instant) {
    assert!(Instant::now() < end, "shared client progress deadline");
    client.poll(Instant::now()).unwrap();
    std::thread::yield_now();
}
fn begin_pam(served: &Served, client: &mut SessionClient, end: Instant) -> ConversationId {
    client
        .begin_login("fixture", "fixture", SECRET, Instant::now(), end)
        .unwrap();
    loop {
        tick(client, end);
        if let Ok(job) = served.jobs.try_recv() {
            return job;
        }
    }
}
fn login_result(client: &mut SessionClient, end: Instant) -> LoginVerdict {
    loop {
        tick(client, end);
        if let Some(result) = client.take_login(Instant::now()).unwrap() {
            return result;
        }
    }
}
fn exchange(client: &mut SessionClient, command: Command) -> Response {
    let end = deadline();
    client.submit_launch(command, Instant::now(), end).unwrap();
    loop {
        tick(client, end);
        if let Some(result) = client.take_launch(Instant::now()).unwrap() {
            return result.unwrap();
        }
    }
}
fn done(client: &mut SessionClient, command: Command) {
    assert!(matches!(exchange(client, command), Response::Done));
}
fn attempt() -> Attempt {
    Attempt {
        server: 1,
        connection: ConnectionId(2),
        session: 3,
        audience: 4,
        wire_generation: 5,
        attempt_generation: 6,
        afid: 7,
    }
}
fn delegation() -> Delegation {
    Delegation {
        identity: Identity {
            principal: 8,
            namespace: 9,
            launch: 10,
        },
        audience: 4,
    }
}

#[test]
fn one_owner_delivers_a_launch_verdict_while_pam_waits_then_accepts_the_login() {
    let (served, mut client) = Served::start(None);
    let end = deadline();
    let job = begin_pam(&served, &mut client, end);
    assert!(matches!(
        client.begin_login("fixture", "fixture", SECRET, Instant::now(), end),
        Err(ClientError::Busy)
    ));
    done(&mut client, Command::Initialize { session: 3 });
    done(&mut client, Command::Reserve(delegation()));
    done(&mut client, Command::Activate { launch: 10 });
    let a = attempt();
    let nonce = match exchange(
        &mut client,
        Command::Begin {
            attempt: a,
            remaining_msec: 5000,
        },
    ) {
        Response::Challenge(nonce) => nonce,
        _ => panic!("challenge"),
    };
    done(&mut client, Command::Relay { launch: 10, nonce });
    match exchange(&mut client, Command::Take { attempt: a }) {
        Response::Verified(verdict) => {
            assert_eq!(verdict.attempt, a);
            assert_eq!(verdict.identity, delegation().identity);
        }
        _ => panic!("launch authentication must progress while the PAM job is pending"),
    }
    assert!(client.take_login(Instant::now()).unwrap().is_none());
    served.finish(job, PamVerdict::Accepted);
    assert_eq!(login_result(&mut client, end), LoginVerdict::Accepted);
    assert_eq!(
        served
            .cancelled
            .recv_timeout(Duration::from_secs(2))
            .unwrap(),
        job
    );
    assert!(client.take_login(Instant::now()).unwrap().is_none());
    let end = deadline();
    let second = begin_pam(&served, &mut client, end);
    assert_ne!(job, second, "each login must own a fresh conversation");
    served.finish(second, PamVerdict::Rejected);
    assert_eq!(
        login_result(&mut client, end),
        LoginVerdict::Refused("authentication failed".into())
    );
    assert!(!client.closed());
}

#[test]
fn launch_timeout_closes_the_shared_transport_and_cancels_pam() {
    let (served, mut client) = Served::start(None);
    let job = begin_pam(&served, &mut client, deadline());
    let end = Instant::now() + Duration::from_millis(100);
    client
        .submit_launch(Command::Initialize { session: 3 }, Instant::now(), end)
        .unwrap();
    // Expire logically without a timing race or a real 100 ms wait.
    assert!(matches!(
        client.poll(end),
        Err(ClientError::Launch(LaunchError::Deadline))
    ));
    served.assert_closed_and_cancelled(job);
    assert!(client.closed());
    assert!(matches!(client.take_login(end), Err(ClientError::Closed)));
    assert!(matches!(client.take_launch(end), Err(ClientError::Closed)));
}

#[test]
fn pam_timeout_closes_launch_custody_and_cannot_deliver_a_late_login() {
    let (served, mut client) = Served::start(None);
    let end = deadline();
    let job = begin_pam(&served, &mut client, end);
    client
        .submit_launch(
            Command::Initialize { session: 3 },
            Instant::now(),
            end + Duration::from_secs(1),
        )
        .unwrap();
    assert!(matches!(
        client.poll(end),
        Err(ClientError::Transport(PipelineError::Timeout))
    ));
    assert!(client.closed());
    assert!(matches!(client.take_launch(end), Err(ClientError::Closed)));
    assert!(matches!(
        client.begin_login(
            "fixture",
            "fixture",
            SECRET,
            end,
            end + Duration::from_secs(1)
        ),
        Err(ClientError::Closed)
    ));
    served.assert_closed_and_cancelled(job);
}

#[test]
fn wrong_user_and_service_refuse_without_a_pam_job_and_leave_the_owner_usable() {
    let (served, mut client) = Served::start(None);
    for (service, user) in [("fixture", "other-user"), ("other-service", "fixture")] {
        let end = deadline();
        client
            .begin_login(service, user, SECRET, Instant::now(), end)
            .unwrap();
        assert!(matches!(
            login_result(&mut client, end),
            LoginVerdict::Refused(_)
        ));
        assert!(served.jobs.try_recv().is_err());
        assert!(!client.closed());
    }
    done(&mut client, Command::Initialize { session: 3 });
}

#[test]
fn login_slot_is_not_released_until_the_result_is_consumed() {
    let (served, mut client) = Served::start(Some(PamVerdict::Accepted));
    let end = deadline();
    let job = begin_pam(&served, &mut client, end);
    loop {
        tick(&mut client, end);
        if let Ok(cancelled) = served.cancelled.try_recv() {
            assert_eq!(cancelled, job);
            break;
        }
    }
    assert!(
        matches!(
            client.begin_login("fixture", "fixture", SECRET, Instant::now(), end),
            Err(ClientError::Busy)
        ),
        "an undelivered PAM result must retain its admission slot"
    );
    assert_eq!(login_result(&mut client, end), LoginVerdict::Accepted);
    client
        .begin_login("fixture", "fixture", SECRET, Instant::now(), deadline())
        .unwrap();
}

#[test]
fn blocking_unlock_compatibility_uses_the_same_conversation_and_can_repeat() {
    let (served, client) = Served::start(Some(PamVerdict::Accepted));
    let mut client: UnlockClient = client;
    // Exceed the pipeline's 128-fid bound: every finished login must clunk.
    for _ in 0..130 {
        assert_eq!(
            client
                .login("fixture", "fixture", SECRET, deadline())
                .unwrap(),
            LoginVerdict::Accepted
        );
        let job = served.jobs.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            served
                .cancelled
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            job
        );
    }
    done(&mut client, Command::Initialize { session: 3 });
}

#[test]
fn peer_disconnect_drops_pending_results_instead_of_reusing_the_connection() {
    let (served, mut client) = Served::start(None);
    let end = deadline();
    begin_pam(&served, &mut client, end);
    client
        .submit_launch(Command::Initialize { session: 3 }, Instant::now(), end)
        .unwrap();
    served.wake.stop();
    loop {
        assert!(Instant::now() < end, "peer loss must end the shared owner");
        if client.poll(Instant::now()).is_err() {
            break;
        }
        std::thread::yield_now();
    }
    assert!(client.closed());
    assert!(matches!(
        client.take_login(Instant::now()),
        Err(ClientError::Closed)
    ));
    assert!(matches!(
        client.take_launch(Instant::now()),
        Err(ClientError::Closed)
    ));
}

#[test]
fn invalid_or_already_expired_inputs_spend_no_login_slot() {
    let (_served, mut client) = Served::start(Some(PamVerdict::Rejected));
    let now = Instant::now();
    assert!(matches!(
        client.begin_login("fixture", "fixture", SECRET, now, now),
        Err(ClientError::Transport(PipelineError::Timeout))
    ));
    assert!(matches!(
        client.begin_login("fixture", "fixture", &[42; 513], now, deadline()),
        Err(ClientError::InvalidInput)
    ));
    assert!(matches!(
        client.submit_launch(Command::Initialize { session: 3 }, now, now),
        Err(ClientError::Launch(LaunchError::Deadline))
    ));
    assert!(!client.closed());
    assert_eq!(
        client
            .login("fixture", "fixture", SECRET, deadline())
            .unwrap(),
        LoginVerdict::Refused("authentication failed".into())
    );
    done(&mut client, Command::Initialize { session: 3 });
}
