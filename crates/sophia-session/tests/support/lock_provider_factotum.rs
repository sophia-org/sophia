//! Real Factotum export and shared client; trusted Session class is supplied.
use crate::launch_authentication::LaunchCoordinator;
use crate::session_unlock_supervisor::{
    SupervisedAuthenticator, UnlockAgent, UnlockAgentLauncher, UnlockSupervision,
};
use sophia_9p::{
    Limits,
    unix::{Server, Wake},
};
use sophia_factotum::{
    client::SessionClient,
    export::FactotumExport,
    launch::queue::{self, ConnectionOwner, Ingress},
    proto::{ChannelClass, Settings},
};
use std::{
    os::unix::net::UnixStream,
    sync::mpsc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub struct Fixture {
    pub coordinator: LaunchCoordinator,
    pub _authenticator: Option<SupervisedAuthenticator>,
}
pub fn namespaces() -> std::sync::Arc<std::sync::Mutex<sophia_runtime::NamespaceRegistry>> {
    use sophia_protocol::{NamespaceCapabilities, NamespaceContext, NamespaceId, NamespaceProfile};
    std::sync::Arc::new(std::sync::Mutex::new(
        sophia_runtime::NamespaceRegistry::with_namespace(
            31,
            NamespaceContext::new(
                NamespaceId::from_raw(2),
                NamespaceProfile::Confined,
                NamespaceCapabilities::NONE,
            )
            .unwrap(),
        )
        .unwrap(),
    ))
}
impl Fixture {
    pub fn new() -> Self {
        Self::with_namespaces(namespaces())
    }
    pub fn with_namespaces(
        namespaces: std::sync::Arc<std::sync::Mutex<sophia_runtime::NamespaceRegistry>>,
    ) -> Self {
        let authenticator =
            SupervisedAuthenticator::start(Launcher, UnlockSupervision::SESSION, || {}).unwrap();
        let mut f = Self {
            coordinator: LaunchCoordinator::new(authenticator.launch_service(), namespaces),
            _authenticator: Some(authenticator),
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while !f.coordinator.ready() {
            assert!(
                Instant::now() < deadline,
                "Factotum fixture initialization deadline"
            );
            f.tick();
        }
        f
    }
    pub fn tick(&mut self) {
        self.coordinator.poll(Instant::now()).unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
}
struct Launcher;
struct Agent {
    ingress: Ingress,
    owner: ConnectionOwner,
    wake: Wake,
    thread: Option<JoinHandle<()>>,
}
impl UnlockAgentLauncher for Launcher {
    type Agent = Agent;
    fn launch(&mut self) -> Result<Agent, String> {
        let (client, stream) = UnixStream::pair().unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            let mut server = Server::new(
                FactotumExport::new(Settings {
                    owner: "fixture".into(),
                    pam_service: "fixture".into(),
                }),
                Limits::default(),
            )
            .unwrap();
            let connection = server.adopt(stream).unwrap_or_else(|_| panic!("adopt"));
            server.export_mut().admit(connection, ChannelClass::Session);
            sender.send(server.wake()).unwrap();
            while server.turn(Some(Duration::from_secs(1))).unwrap() {}
        });
        let wake = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        let client = SessionClient::over(client, Duration::from_secs(2)).unwrap();
        let (ingress, owner) = queue::channel();
        Ok(Agent {
            ingress,
            owner: owner.connect(client),
            wake,
            thread: Some(thread),
        })
    }
}
impl UnlockAgent for Agent {
    fn launch_ingress(&self) -> Option<Ingress> {
        Some(self.ingress.clone())
    }
    fn begin_login(&mut self, _: &[u8]) -> Result<(), String> {
        Err("PAM is outside this fixture".into())
    }
    fn poll_login(&mut self) -> Result<Option<bool>, String> {
        self.owner
            .poll(Instant::now())
            .map_err(|e| format!("{e:?}"))?;
        Ok(None)
    }
    fn gone(&mut self) -> bool {
        !self.ingress.is_open()
    }
}
impl Drop for Agent {
    fn drop(&mut self) {
        self.wake.stop();
        let _ = self.thread.take().unwrap().join();
    }
}
