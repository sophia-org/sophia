//! Ordinary Xauthority lookup across the real confined launch and X frontend.
use super::LiveXAuthorityFile;
use crate::live_session::x_frontend::LiveXAdmissionPolicy;
use sophia_protocol::{NamespaceCapabilities, NamespaceProfile};
use sophia_runtime::{ConfinedXApplicationLaunch, NamespaceRegistry};
use sophia_x_authority::*;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use x11rb::protocol::xproto::ConnectionExt;

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Server {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let result = thread.join();
            if !std::thread::panicking() {
                result.unwrap();
            }
        }
    }
}

struct ChildGuard(Option<Child>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn client(expect_cookie: bool) {
    // Ignored child fixtures must never connect to a developer's display.
    assert_eq!(
        std::env::var("XAUTHORITY").unwrap(),
        sophia_runtime::CONFINED_X_AUTHORITY_FILE
    );
    assert_eq!(
        rustix::system::uname().nodename().to_bytes(),
        sophia_runtime::PROTECTION_DOMAIN_HOSTNAME.as_bytes()
    );
    match x11rb::connect(None) {
        Ok((connection, _)) => {
            assert!(
                expect_cookie,
                "host-only record unexpectedly selected inside sandbox"
            );
            connection.get_input_focus().unwrap().reply().unwrap();
            println!("confined X cookie selected and request answered");
        }
        Err(error) => {
            assert!(!expect_cookie, "sandbox cookie lookup failed: {error:?}");
            assert!(
                matches!(error, x11rb::errors::ConnectError::SetupFailed(_)),
                "must reach setup refusal, not fail to connect: {error:?}"
            );
            println!("host-only cookie lookup refused at X setup");
        }
    }
}

#[test]
#[ignore = "child of confined_cookie_lookup_reaches_the_group_frontend"]
fn confined_client() {
    client(true);
}

#[test]
#[ignore = "child of confined_cookie_lookup_reaches_the_group_frontend"]
fn host_only_client() {
    client(false);
}

#[test]
fn confined_cookie_lookup_reaches_the_group_frontend() {
    assert!(
        std::path::Path::new("/usr/bin/bwrap").exists(),
        "this joined confinement test requires bubblewrap"
    );
    assert_ne!(
        rustix::system::uname().nodename().to_bytes(),
        sophia_runtime::PROTECTION_DOMAIN_HOSTNAME.as_bytes(),
        "negative control needs distinct host and sandbox names"
    );
    let root = Scratch(std::env::temp_dir().join(format!("scxa-{}", std::process::id())));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root.0)
        .unwrap();
    let group = root.0.join("group");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&group)
        .unwrap();
    let display = 20_000 + std::process::id() % 20_000;
    let (authority, cookie) = LiveXAuthorityFile::create_in_with_local_hosts(
        &root.0,
        display,
        &[sophia_runtime::PROTECTION_DOMAIN_HOSTNAME.as_bytes()],
    )
    .unwrap();
    // Preserve the same secret and display; remove only the sandbox hostname.
    let bytes = std::fs::read(authority.path()).unwrap();
    let mut end = 2;
    for _ in 0..4 {
        let len = u16::from_be_bytes(bytes[end..end + 2].try_into().unwrap()) as usize;
        end += 2 + len;
    }
    let host_only = root.0.join("host-only");
    use std::io::Write;
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&host_only)
        .unwrap()
        .write_all(&bytes[..end])
        .unwrap();
    let mut namespaces = NamespaceRegistry::new(1).unwrap();
    let shared =
        namespaces.create_namespace(NamespaceProfile::ClassicShared, NamespaceCapabilities::NONE);
    let confined =
        namespaces.create_namespace(NamespaceProfile::Confined, NamespaceCapabilities::NONE);
    let registry = Arc::new(Mutex::new(namespaces));
    let policy = Arc::new(LiveXAdmissionPolicy {
        launch_origins: Default::default(),
        registry: registry.clone(),
        namespaces: [
            (XServerFrontendListenerId::PRIMARY, shared.id),
            (XServerFrontendListenerId(7), confined.id),
        ]
        .into(),
        session_user_id: rustix::process::geteuid().as_raw(),
    });
    let config = XServerFrontendConfig::new_with_namespace_context(root.0.join("shared"), shared)
        .unwrap()
        .with_setup_authorization(XServerFrontendSetupAuthorization::MitMagicCookie(cookie))
        .with_admission_policy(policy)
        .with_listener(
            XServerFrontendListenerId(7),
            group.join(format!("X{display}")),
            confined,
        )
        .unwrap();
    let mut frontend = XServerFrontend::bind_exclusive(config).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = stop.clone();
    let server = Server {
        stop,
        thread: Some(std::thread::spawn(move || {
            let broker = XServerFrontendRouteBroker::new(std::num::NonZeroUsize::new(4).unwrap());
            let observer: Arc<X11CoreTraceObserver> = Arc::new(|_| Ok(None));
            while !server_stop.load(Ordering::Acquire) {
                frontend
                    .try_serve_next_concurrently_routed_traced(&broker, observer.clone())
                    .unwrap();
                std::thread::sleep(Duration::from_millis(1));
            }
            frontend.shutdown_all_client_workers().unwrap();
            frontend.wait_for_clients().unwrap();
        })),
    };
    for (name, path, marker) in [
        (
            "host_only_client",
            host_only.as_path(),
            "host-only cookie lookup refused at X setup",
        ),
        (
            "confined_client",
            authority.path(),
            "confined X cookie selected and request answered",
        ),
    ] {
        let launch = ConfinedXApplicationLaunch {
            executable: std::env::current_exe().unwrap(),
            args: [
                "--exact".to_owned(),
                format!("live_session::tests::confined_x_authentication::{name}"),
                "--ignored".to_owned(),
                "--nocapture".to_owned(),
            ]
            .map(Into::into)
            .into(),
            bubblewrap: "/usr/bin/bwrap".into(),
            group_directory: group.clone(),
            cookie_file: path.into(),
            display_number: display,
        };
        let mut child = ChildGuard(Some(
            launch
                .command()
                .unwrap()
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ));
        let deadline = Instant::now() + Duration::from_secs(8);
        while child.0.as_mut().unwrap().try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "confined cookie child deadline");
            std::thread::sleep(Duration::from_millis(5));
        }
        let output = child.0.take().unwrap().wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{name}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains(marker));
    }
    drop(server);
    assert_eq!(registry.lock().unwrap().admission_count(), 0);
}
