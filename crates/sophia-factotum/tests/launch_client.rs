//! Actual core/Factotum exchange. Admission facts and PAM completion are supplied;
//! no Session worker, protected child relay or live PAM authentication is claimed.

use sophia_9p::{
    ConnectionId, Errno, Fid, Limits, Tag,
    authentication::{Attempt, Identity},
    unix::{Server, Wake},
};
use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
use sophia_factotum::{
    export::FactotumExport,
    jobs::{ConversationId, JobSink},
    launch::{
        Delegation,
        client::{Error, LaunchClient},
        wire::{Command, Response},
    },
    proto::{ChannelClass, Job, JobOutcome, PamVerdict, Settings},
};
use std::{
    os::unix::net::UnixStream,
    sync::mpsc::{self, Receiver, Sender, SyncSender},
    thread::JoinHandle,
    time::{Duration, Instant},
};

struct Jobs(SyncSender<ConversationId>);
impl JobSink for Jobs {
    fn submit(&mut self, id: ConversationId, _job: Job) -> bool {
        self.0.try_send(id).is_ok()
    }
    fn cancel(&mut self, _id: ConversationId) {}
}
struct Served {
    wake: Wake,
    thread: Option<JoinHandle<()>>,
    jobs: Receiver<ConversationId>,
    outcomes: Sender<(ConversationId, JobOutcome)>,
}
impl Served {
    fn start() -> (Self, Pipeline) {
        Self::with_limits(PipelineLimits::default())
    }

    fn with_limits(limits: PipelineLimits) -> (Self, Pipeline) {
        let (client, stream) = UnixStream::pair().unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        let (job_sender, jobs) = mpsc::sync_channel(1);
        let (outcomes, outcome_receiver) = mpsc::channel();
        let thread = Some(std::thread::spawn(move || {
            let mut export = FactotumExport::new(Settings {
                owner: "fixture".into(),
                pam_service: "fixture".into(),
            });
            export.attach_jobs(Box::new(Jobs(job_sender)), outcome_receiver);
            let mut server = Server::new(export, Limits::default()).unwrap();
            let id = server.adopt(stream).unwrap_or_else(|_| panic!("adopt"));
            server.export_mut().admit(id, ChannelClass::Session);
            sender.send(server.wake()).unwrap();
            server.run().unwrap();
        }));
        let wake = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        let pipeline = Pipeline::over(client, limits, Duration::from_secs(2)).unwrap();
        (
            Self {
                wake,
                thread,
                jobs,
                outcomes,
            },
            pipeline,
        )
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
fn step(client: &mut LaunchClient, pipeline: &mut Pipeline) {
    client.advance(pipeline, Instant::now()).unwrap();
    pipeline.poll().unwrap();
    while let Some((tag, reply)) = pipeline.take_reply() {
        assert!(
            client.owns(tag),
            "a pending PAM read must not complete before its outcome"
        );
        client.receive(tag, reply, Instant::now()).unwrap();
    }
    std::thread::yield_now();
}
fn opened(pipeline: &mut Pipeline) -> LaunchClient {
    let end = deadline();
    let mut client = LaunchClient::start(pipeline, Instant::now(), end).unwrap();
    while !client.ready() {
        assert!(Instant::now() < end, "private attach deadline");
        step(&mut client, pipeline);
    }
    client
}
fn exchange(
    client: &mut LaunchClient,
    pipeline: &mut Pipeline,
    command: Command,
) -> Result<Response, Errno> {
    let end = deadline();
    client
        .submit(pipeline, command, Instant::now(), end)
        .unwrap();
    loop {
        assert!(
            Instant::now() < end,
            "launch exchange must progress without the PAM outcome"
        );
        step(client, pipeline);
        if let Some(result) = client.take(Instant::now()).unwrap() {
            return result;
        }
    }
}
fn done(client: &mut LaunchClient, pipeline: &mut Pipeline, command: Command) {
    assert!(matches!(
        exchange(client, pipeline, command),
        Ok(Response::Done)
    ));
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
fn pam_step(pipeline: &mut Pipeline, rpc: Fid, bytes: &[u8]) {
    let tag = pipeline.write(rpc, 0, bytes).unwrap();
    assert_eq!(
        pipeline.wait(tag, deadline()).unwrap(),
        Reply::Write(bytes.len() as u32)
    );
    let tag = pipeline.read(rpc, 0, 4096).unwrap();
    assert_eq!(
        pipeline.wait(tag, deadline()).unwrap(),
        Reply::Read(b"ok".to_vec())
    );
}
fn waiting_pam(pipeline: &mut Pipeline) -> (Fid, Tag) {
    let (tag, root) = pipeline.attach(b"", b"factotum").unwrap();
    assert!(matches!(
        pipeline.wait(tag, deadline()).unwrap(),
        Reply::Attach(_)
    ));
    let (tag, rpc) = pipeline.walk(root, &[b"rpc"]).unwrap();
    assert!(matches!(
        pipeline.wait(tag, deadline()).unwrap(),
        Reply::Walk(_)
    ));
    let tag = pipeline.lopen(rpc, 2).unwrap();
    assert!(matches!(
        pipeline.wait(tag, deadline()).unwrap(),
        Reply::Lopen { .. }
    ));
    pam_step(pipeline, rpc, b"start proto=pam role=login service=fixture");
    pam_step(pipeline, rpc, b"write fixture");
    let tag = pipeline
        .write_secret(rpc, 0, b"write supplied-fixture-secret")
        .unwrap();
    assert!(matches!(
        pipeline.wait(tag, deadline()).unwrap(),
        Reply::Write(_)
    ));
    let tag = pipeline.read(rpc, 0, 4096).unwrap();
    pipeline.poll().unwrap();
    (rpc, tag)
}

#[test]
fn launch_exchange_progresses_while_pam_read_waits_on_the_same_connection() {
    let (served, mut pipeline) = Served::start();
    let (pam, pam_tag) = waiting_pam(&mut pipeline);
    let job = served.jobs.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut client = opened(&mut pipeline);
    assert!(!client.owns(pam_tag));
    assert!(matches!(
        client.receive(pam_tag, Reply::Read(b"ok".to_vec()), Instant::now()),
        Err(Error::ForeignTag)
    ));
    done(
        &mut client,
        &mut pipeline,
        Command::Initialize { session: 3 },
    );
    done(&mut client, &mut pipeline, Command::Reserve(delegation()));
    let a = attempt();
    let nonce = match exchange(
        &mut client,
        &mut pipeline,
        Command::Begin {
            attempt: a,
            remaining_msec: 5000,
        },
    )
    .unwrap()
    {
        Response::Challenge(nonce) => nonce,
        _ => panic!("challenge"),
    };
    assert_eq!(
        exchange(
            &mut client,
            &mut pipeline,
            Command::Relay { launch: 10, nonce }
        )
        .err(),
        Some(Errno::EAGAIN)
    );
    done(&mut client, &mut pipeline, Command::Activate { launch: 10 });
    done(
        &mut client,
        &mut pipeline,
        Command::Relay { launch: 10, nonce },
    );
    match exchange(&mut client, &mut pipeline, Command::Take { attempt: a }).unwrap() {
        Response::Verified(verdict) => {
            assert_eq!(verdict.attempt, a);
            assert_eq!(verdict.identity, delegation().identity);
        }
        _ => panic!("launch verdict must not wait for PAM"),
    }
    assert_eq!(
        pipeline.outstanding(),
        1,
        "only the PAM read remains outstanding"
    );
    served
        .outcomes
        .send((job, JobOutcome::Pam(PamVerdict::Rejected)))
        .unwrap();
    served.wake.wake();
    assert_eq!(
        pipeline.wait(pam_tag, deadline()).unwrap(),
        Reply::Read(b"error authentication failed".to_vec())
    );
    let tag = pipeline.clunk(pam).unwrap();
    assert_eq!(pipeline.wait(tag, deadline()).unwrap(), Reply::Clunk);
}

#[test]
fn a_buffered_result_keeps_its_slot_and_original_deadline() {
    let (_served, mut pipeline) = Served::start();
    let mut client = opened(&mut pipeline);
    let end = deadline();
    client
        .submit(
            &mut pipeline,
            Command::Initialize { session: 3 },
            Instant::now(),
            end,
        )
        .unwrap();
    loop {
        assert!(Instant::now() < end);
        client.advance(&mut pipeline, Instant::now()).unwrap();
        pipeline.poll().unwrap();
        if let Some((tag, reply)) = pipeline.take_reply() {
            let complete = matches!(reply, Reply::Read(_));
            client.receive(tag, reply, Instant::now()).unwrap();
            if complete {
                break;
            }
        }
        std::thread::yield_now();
    }
    assert!(!client.ready());
    assert!(matches!(
        client.submit(
            &mut pipeline,
            Command::Reserve(delegation()),
            Instant::now(),
            end
        ),
        Err(Error::Busy)
    ));
    assert!(
        matches!(client.take(end), Err(Error::Deadline)),
        "an undelivered result must not outlive the owner deadline"
    );
    assert!(matches!(client.take(end), Err(Error::Closed)));
}

#[test]
fn a_late_write_reply_cannot_renew_the_owner_deadline() {
    let (_served, mut pipeline) = Served::start();
    let mut client = opened(&mut pipeline);
    let end = deadline();
    client
        .submit(
            &mut pipeline,
            Command::Initialize { session: 3 },
            Instant::now(),
            end,
        )
        .unwrap();
    loop {
        assert!(Instant::now() < end);
        pipeline.poll().unwrap();
        if let Some((tag, reply)) = pipeline.take_reply() {
            assert!(matches!(
                client.receive(tag, reply, end),
                Err(Error::Deadline)
            ));
            break;
        }
        std::thread::yield_now();
    }
    assert!(matches!(
        client.advance(&mut pipeline, end),
        Err(Error::Closed)
    ));
}

#[test]
fn a_well_formed_verdict_for_another_attempt_is_fatal() {
    let (_served, mut pipeline) = Served::start();
    let mut client = opened(&mut pipeline);
    done(
        &mut client,
        &mut pipeline,
        Command::Initialize { session: 3 },
    );
    let a = attempt();
    assert!(matches!(
        exchange(
            &mut client,
            &mut pipeline,
            Command::Begin {
                attempt: a,
                remaining_msec: 5000
            }
        ),
        Ok(Response::Challenge(_))
    ));
    let end = deadline();
    client
        .submit(
            &mut pipeline,
            Command::Take { attempt: a },
            Instant::now(),
            end,
        )
        .unwrap();
    loop {
        assert!(Instant::now() < end);
        client.advance(&mut pipeline, Instant::now()).unwrap();
        pipeline.poll().unwrap();
        if let Some((tag, reply)) = pipeline.take_reply() {
            if matches!(reply, Reply::Read(_)) {
                let mut other = a;
                other.attempt_generation += 1;
                let bytes = Response::Verified(sophia_factotum::launch::Verdict {
                    attempt: other,
                    identity: delegation().identity,
                })
                .encode();
                assert!(
                    matches!(
                        client.receive(tag, Reply::Read(bytes), Instant::now()),
                        Err(Error::Unexpected)
                    ),
                    "a verdict must name the exact requested attempt"
                );
                assert!(matches!(client.take(Instant::now()), Err(Error::Closed)));
                break;
            }
            client.receive(tag, reply, Instant::now()).unwrap();
        }
        std::thread::yield_now();
    }
}

#[test]
fn pipeline_pressure_preserves_the_unsent_read_and_does_not_repeat_the_command() {
    let (_served, mut pipeline) = Served::with_limits(PipelineLimits {
        max_outstanding: 1,
        ..PipelineLimits::default()
    });
    let mut client = opened(&mut pipeline);
    let end = deadline();
    client
        .submit(
            &mut pipeline,
            Command::Initialize { session: 3 },
            Instant::now(),
            end,
        )
        .unwrap();
    // Stop after the write answer, before the response read is queued.
    loop {
        assert!(Instant::now() < end);
        pipeline.poll().unwrap();
        if let Some((tag, reply)) = pipeline.take_reply() {
            assert!(matches!(reply, Reply::Write(_)));
            client.receive(tag, reply, Instant::now()).unwrap();
            break;
        }
        std::thread::yield_now();
    }
    let (other, _) = pipeline.attach(b"", b"factotum").unwrap();
    assert!(matches!(
        client.advance(&mut pipeline, Instant::now()),
        Err(Error::Transport(
            sophia_9p_client::pipeline::PipelineError::Limit(_)
        ))
    ));
    assert!(matches!(
        pipeline.wait(other, end).unwrap(),
        Reply::Attach(_)
    ));
    loop {
        assert!(Instant::now() < end);
        step(&mut client, &mut pipeline);
        if let Some(result) = client.take(Instant::now()).unwrap() {
            assert!(
                matches!(result, Ok(Response::Done)),
                "credit recovery must read the original response without reinitializing Factotum"
            );
            break;
        }
    }
    done(&mut client, &mut pipeline, Command::Reserve(delegation()));
}
