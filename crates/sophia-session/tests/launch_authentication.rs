#![cfg(feature = "native-session")]
//! Actual protected spawn, child 9P relay and Factotum verdict. Session's
//! control-channel class and launch policy are supplied. The lock transport
//! case also exercises the shipped role worker with an independent wire peer.
use sophia_9p::{
    ConnectionId, Limits,
    authentication::Attempt,
    unix::{Server, Wake},
};
use sophia_factotum::{
    client::SessionClient,
    export::FactotumExport,
    launch::{
        Nonce,
        queue::{self, ConnectionOwner, Ingress, Ticket},
        wire::{Command, Response},
    },
    proto::{ChannelClass, Settings},
};
use sophia_runtime::{
    ProcessLaunchSpec, ProcessSupervisor, ProtectionDomainRole, ProtectionDomainSpec,
    SupervisedProcessKind,
};
use sophia_session::launch_authentication::{Error, LaunchBook, LaunchId, LaunchPolicy, Phase};
use std::{
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::mpsc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[path = "support/role_authentication.rs"]
mod role_authentication;

#[path = "support/lock_authentication.rs"]
mod lock_authentication;

#[path = "support/lock_authenticated_transport.rs"]
mod lock_authenticated_transport;

#[path = "support/authentication_transport_budget.rs"]
mod authentication_transport_budget;

#[path = "support/output_authenticated_transport.rs"]
mod output_authenticated_transport;

#[path = "support/inspection_authenticated_transport.rs"]
mod inspection_authenticated_transport;

#[path = "support/authentication_endpoint_grants.rs"]
mod authentication_endpoint_grants;

struct Fixture {
    namespaces: std::sync::Arc<std::sync::Mutex<sophia_runtime::NamespaceRegistry>>,
    book: LaunchBook,
    ingress: Ingress,
    owner: ConnectionOwner,
    wake: Wake,
    thread: Option<JoinHandle<()>>,
}
impl Fixture {
    fn new() -> Self {
        let (client, stream) = UnixStream::pair().unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            let export = FactotumExport::new(Settings {
                owner: "fixture".into(),
                pam_service: "fixture".into(),
            });
            let mut server = Server::new(export, Limits::default()).unwrap();
            let id = server.adopt(stream).unwrap_or_else(|_| panic!("adopt"));
            server.export_mut().admit(id, ChannelClass::Session);
            sender.send(server.wake()).unwrap();
            while server.turn(Some(Duration::from_secs(1))).unwrap() {}
        });
        let wake = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        let client = SessionClient::over(client, Duration::from_secs(2)).unwrap();
        let (ingress, owner) = queue::channel();
        let now = Instant::now();
        let book = LaunchBook::new(3, ingress.clone(), now, now + Duration::from_secs(5)).unwrap();
        let mut f = Self {
            namespaces: std::sync::Arc::new(std::sync::Mutex::new(
                sophia_runtime::NamespaceRegistry::with_namespace(
                    31,
                    sophia_protocol::NamespaceContext::new(
                        sophia_protocol::NamespaceId::from_raw(9),
                        sophia_protocol::NamespaceProfile::Confined,
                        sophia_protocol::NamespaceCapabilities::NONE,
                    )
                    .unwrap(),
                )
                .unwrap(),
            )),
            book,
            ingress,
            owner: owner.connect(client),
            wake,
            thread: Some(thread),
        };
        let end = deadline();
        while !f.book.ready() {
            f.tick(end);
        }
        f
    }
    fn tick(&mut self, end: Instant) {
        assert!(Instant::now() < end, "launch owner progress deadline");
        self.owner.poll(Instant::now()).unwrap();
        self.book.poll(Instant::now()).unwrap();
        std::thread::yield_now();
    }
    fn answer(&mut self, mut ticket: Ticket) -> Response {
        let end = deadline();
        loop {
            self.tick(end);
            if let Some(response) = ticket.take(Instant::now()).unwrap() {
                return response;
            }
        }
    }
    fn command(&mut self, command: Command) -> Response {
        let ticket = self
            .ingress
            .submit(command, Instant::now(), deadline())
            .unwrap();
        self.answer(ticket)
    }
    fn reserve(&mut self) -> LaunchId {
        self.reserve_audiences(&[4])
    }
    fn reserve_audiences(&mut self, audiences: &[u64]) -> LaunchId {
        self.reserve_role_audiences(ProtectionDomainRole::MetadataShell, audiences)
    }
    fn reserve_role_audiences(
        &mut self,
        role: ProtectionDomainRole,
        audiences: &[u64],
    ) -> LaunchId {
        let id = self
            .book
            .reserve_with_audiences(
                LaunchPolicy {
                    principal: 8,
                    namespace: 9,
                    audience: 4,
                    role,
                },
                sophia_factotum::launch::Audiences::new(audiences).unwrap(),
                Instant::now(),
                deadline(),
            )
            .unwrap();
        let end = deadline();
        while self.book.phase(id) != Some(Phase::Ready) {
            self.tick(end);
        }
        id
    }
    fn activate(&mut self, id: LaunchId, supervisor: &mut ProcessSupervisor) {
        let end = deadline();
        loop {
            self.tick(end);
            if self.book.activate(id, supervisor, Instant::now()).unwrap() {
                break;
            }
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.wake.stop();
        let result = self.thread.take().unwrap().join();
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
fn attempt() -> Attempt {
    Attempt {
        server: 1,
        connection: ConnectionId(2),
        session: 3,
        audience: 4,
        wire_generation: 1,
        attempt_generation: 1,
        afid: 7,
    }
}
struct Probe(PathBuf);
impl Probe {
    fn build() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "launch-authentication-probe-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        assert!(
            std::process::Command::new("/usr/bin/cc")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-O2"])
                .arg(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/support/launch_authentication_probe.c"
                ))
                .arg("-o")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        Self(path)
    }
    fn supervisor(&self, nonce: Nonce) -> ProcessSupervisor {
        let hex = nonce
            .0
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        ProcessSupervisor::new(
            SupervisedProcessKind::Shell,
            ProcessLaunchSpec::new(&self.0).arg(hex).protection_domain(
                ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell]).unwrap(),
            ),
        )
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn protected_child_relay_verdict_requires_the_live_exact_launch() {
    let mut f = Fixture::new();
    let probe = Probe::build();
    let id = f.reserve();
    let Response::Challenge(nonce) = f.command(Command::Begin {
        attempt: attempt(),
        remaining_msec: 5000,
    }) else {
        panic!("challenge")
    };
    let mut supervisor = probe.supervisor(nonce);
    let channel = f.book.take_channel(id, Instant::now()).unwrap();
    supervisor.start_with_launch_channel(channel).unwrap();
    f.activate(id, &mut supervisor);
    let end = deadline();
    let verdict = loop {
        assert!(
            Instant::now() < end,
            "protected child must relay Factotum's challenge"
        );
        match f.command(Command::Take { attempt: attempt() }) {
            Response::Verified(verdict) => break verdict,
            Response::Pending => {}
            _ => panic!("unexpected verdict response"),
        }
    };
    assert_eq!(verdict.identity.launch, id.generation());
    assert_eq!(
        f.book
            .authorize(verdict, &mut supervisor, Instant::now())
            .unwrap(),
        verdict.identity
    );
    let mut forged = verdict;
    forged.identity.namespace += 1;
    assert!(matches!(
        f.book.authorize(forged, &mut supervisor, Instant::now()),
        Err(Error::Stale)
    ));
    supervisor.request_termination().unwrap();
    assert!(
        matches!(
            f.book.authorize(verdict, &mut supervisor, Instant::now()),
            Err(Error::Stale)
        ),
        "a termination request must withdraw launch custody before attach"
    );
    assert_eq!(f.book.phase(id), Some(Phase::Retiring));
    supervisor.terminate().unwrap();
    let end = deadline();
    while f.book.phase(id).is_some() {
        f.tick(end);
    }
}

#[test]
fn protected_recipe_launch_authenticates_an_approved_secondary_audience_only() {
    let mut f = Fixture::new();
    let probe = Probe::build();
    let id = f.reserve_audiences(&[4, 7]);
    let mut secondary = attempt();
    secondary.audience = 7;
    let Response::Challenge(nonce) = f.command(Command::Begin {
        attempt: secondary,
        remaining_msec: 5000,
    }) else {
        panic!("challenge")
    };
    let mut supervisor = probe.supervisor(nonce);
    let channel = f.book.take_channel(id, Instant::now()).unwrap();
    supervisor.start_with_launch_channel(channel).unwrap();
    f.activate(id, &mut supervisor);
    let end = deadline();
    let verdict = loop {
        assert!(Instant::now() < end, "secondary audience relay deadline");
        match f.command(Command::Take { attempt: secondary }) {
            Response::Verified(v) => break v,
            Response::Pending => (),
            _ => panic!("unexpected response"),
        }
    };
    assert_eq!(
        f.book
            .authorize(verdict, &mut supervisor, Instant::now())
            .unwrap(),
        verdict.identity
    );
    for audience in [4, 7] {
        assert!(
            f.book
                .worker_authority_for_audience(id, audience, &mut supervisor, Instant::now())
                .is_ok()
        );
    }
    assert!(
        matches!(
            f.book
                .worker_authority_for_audience(id, 8, &mut supervisor, Instant::now()),
            Err(Error::Invalid)
        ),
        "a worker must not acquire an unapproved recipe audience"
    );
    let mut forged = verdict;
    forged.attempt.audience = 8;
    assert!(
        matches!(
            f.book.authorize(forged, &mut supervisor, Instant::now()),
            Err(Error::Stale)
        ),
        "Session must independently refuse an audience outside launch policy"
    );
    f.book.revoke(id, Instant::now()).unwrap();
    assert!(matches!(
        f.book.authorize(verdict, &mut supervisor, Instant::now()),
        Err(Error::Stale)
    ));
    supervisor.terminate().unwrap();
    let end = deadline();
    while f.book.phase(id).is_some() {
        f.tick(end);
    }
}

#[test]
fn another_spawn_cannot_activate_a_reserved_launch_with_the_same_number() {
    let mut f = Fixture::new();
    let id = f.reserve();
    let reserved = f.book.take_channel(id, Instant::now()).unwrap();
    let (_owner, foreign) = sophia_runtime::LaunchChannel::reserve(id.generation()).unwrap();
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::Shell,
        ProcessLaunchSpec::new("/usr/bin/sleep")
            .arg("10")
            .protection_domain(
                ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell]).unwrap(),
            ),
    );
    supervisor.start_with_launch_channel(foreign).unwrap();
    assert!(
        matches!(
            f.book.activate(id, &mut supervisor, Instant::now()),
            Err(Error::Stale)
        ),
        "matching generation numbers must not substitute another spawn's custody"
    );
    drop(reserved);
    supervisor.terminate().unwrap();
}

#[test]
fn failed_spawn_retires_registration_and_burns_its_generation() {
    let mut f = Fixture::new();
    let id = f.reserve();
    let channel = f.book.take_channel(id, Instant::now()).unwrap();
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::Shell,
        ProcessLaunchSpec::new("/nonexistent-sophia-launch-authentication-probe")
            .protection_domain(
                ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell]).unwrap(),
            ),
    );
    assert!(supervisor.start_with_launch_channel(channel).is_err());
    assert!(matches!(
        f.book.activate(id, &mut supervisor, Instant::now()),
        Err(Error::Stale)
    ));
    let end = deadline();
    while f.book.phase(id).is_some() {
        f.tick(end);
    }
    let next = f.reserve();
    assert!(
        next.generation() > id.generation(),
        "failed launch generations must never be reused"
    );
    assert!(matches!(
        f.book.take_channel(id, Instant::now()),
        Err(Error::Stale)
    ));
}

#[test]
fn reserved_launch_capacity_is_reclaimed_only_after_retirement() {
    let mut f = Fixture::new();
    let ids = (0..sophia_factotum::launch::MAX_DELEGATIONS)
        .map(|_| f.reserve())
        .collect::<Vec<_>>();
    let policy = LaunchPolicy {
        principal: 8,
        namespace: 9,
        audience: 4,
        role: ProtectionDomainRole::MetadataShell,
    };
    assert!(matches!(
        f.book.reserve(policy, Instant::now(), deadline()),
        Err(Error::Capacity)
    ));
    f.book.revoke(ids[0], Instant::now()).unwrap();
    assert!(
        matches!(
            f.book.reserve(policy, Instant::now(), deadline()),
            Err(Error::Capacity)
        ),
        "local revocation cannot release a registration before Factotum retires it"
    );
    let end = deadline();
    while f.book.phase(ids[0]).is_some() {
        f.tick(end);
    }
    let next = f.reserve();
    assert!(next.generation() > ids.last().unwrap().generation());
    let ingress = f.ingress.clone();
    drop(f);
    assert!(
        !ingress.is_open(),
        "dropping the launch book invalidates its shared connection"
    );
}
