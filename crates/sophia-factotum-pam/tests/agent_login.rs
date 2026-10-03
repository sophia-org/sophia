//! Session's unlock end to end: the agent's real serve loop on one end of a
//! socket pair, Session's client on the other, and the real PAM helper
//! behind a private configuration directory.

use sophia_factotum::agent::{AgentConfig, serve};
use sophia_factotum::client::{LoginVerdict, UnlockClient};
use sophia_factotum::pam_helper::PamHelper;
use sophia_factotum::proto::Settings;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

struct Confdir(PathBuf);

impl Confdir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sophia-factotum-login-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("permit"), "auth required pam_permit.so\n").unwrap();
        std::fs::write(path.join("deny"), "auth required pam_deny.so\n").unwrap();
        Self(path)
    }
}

impl Drop for Confdir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An agent for `service` serving one Session connection on a thread.
fn agent(confdir: &Confdir, service: &str) -> (UnlockClient, JoinHandle<std::io::Result<()>>) {
    let (session, agent_end) = UnixStream::pair().unwrap();
    let config = AgentConfig {
        settings: Settings {
            owner: "sophia-test-user".into(),
            pam_service: service.into(),
        },
        helper: PamHelper {
            path: PathBuf::from(env!("CARGO_BIN_EXE_sophia-factotum-pam")),
            confdir: Some(confdir.0.clone()),
            deadline: Duration::from_secs(10),
        },
        workers: 2,
    };
    let served = std::thread::spawn(move || serve(config, agent_end));
    let client = UnlockClient::over(session, Duration::from_secs(5)).unwrap();
    (client, served)
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(20)
}

#[test]
fn the_owners_login_is_accepted_and_each_attempt_stands_alone() {
    let confdir = Confdir::new("accept");
    let (mut client, served) = agent(&confdir, "permit");
    for _ in 0..2 {
        assert_eq!(
            client
                .login("permit", "sophia-test-user", b"anything", deadline())
                .unwrap(),
            LoginVerdict::Accepted
        );
    }
    drop(client);
    served.join().unwrap().unwrap();
}

#[test]
fn a_rejected_password_and_another_user_are_refused() {
    let confdir = Confdir::new("refuse");
    let (mut client, served) = agent(&confdir, "deny");
    assert_eq!(
        client
            .login("deny", "sophia-test-user", b"wrong", deadline())
            .unwrap(),
        LoginVerdict::Refused("authentication failed".into())
    );
    assert_eq!(
        client.login("deny", "root", b"any", deadline()).unwrap(),
        LoginVerdict::Refused("user not permitted".into())
    );
    assert_eq!(
        client
            .login("permit", "sophia-test-user", b"any", deadline())
            .unwrap(),
        LoginVerdict::Refused("unknown pam service".into()),
        "the agent runs only its configured service"
    );
    drop(client);
    served.join().unwrap().unwrap();
}
