//! Nested launch controls use fabricated endpoints only. The outer xtask
//! process has already hidden the operator's sockets, devices and environment.
use super::{COOKIE, NEXT, WAIT};
use sophia_conformance::private_instance::{Child, ENVIRONMENT, Launch, Mount, NAMESPACES};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const LOADER: &str = "/usr/lib/ld-linux-x86-64.so.2";
const HOST: &str = "/work/evidence/native-input-conformance-host";

struct Fixture {
    child: Option<Child>,
    control: File,
    directory: PathBuf,
    case: PathBuf,
}

impl Fixture {
    fn prepare() -> (Self, File) {
        let directory = PathBuf::from("/work/evidence").join(format!(
            "m4-host-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let case = directory.join("case");
        std::fs::create_dir_all(&case).unwrap();
        std::fs::write(case.join("cookie"), COOKIE).unwrap();
        let (read, write) = rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC).unwrap();
        (
            Self {
                child: None,
                control: File::from(write),
                directory,
                case,
            },
            File::from(read),
        )
    }

    fn arguments(control: i32, activation: &str) -> Vec<String> {
        [
            "--activation-fd",
            activation,
            "--control-fd",
            &control.to_string(),
            "--socket",
            "/work/case/private.sock",
            "--cookie-file",
            "/work/case/cookie",
            "--ready-file",
            "/work/case/ready",
            "--instance",
            "731",
            "--namespace",
            "731",
            "--session-generation",
            "1",
            "--width",
            "320",
            "--height",
            "240",
            "--grants",
            "disabled",
            "--lifetime-ms",
            "15000",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }

    fn launch(&mut self, read: &File, extra: &[&str]) {
        let mut command = vec![
            LOADER.into(),
            "--library-path".into(),
            "/usr/lib".into(),
            "/work/host".into(),
        ];
        command.extend(Self::arguments(read.as_raw_fd(), "{activation_fd}"));
        command.extend(extra.iter().map(|arg| (*arg).to_owned()));
        self.child = Some(
            Launch {
                source: "/work/source".into(),
                directory: self.directory.join("launch"),
                command,
                mounts: vec![
                    Mount {
                        source: HOST.into(),
                        destination: "/work/host".into(),
                        writable: false,
                    },
                    Mount {
                        source: self.case.clone(),
                        destination: "/work/case".into(),
                        writable: true,
                    },
                ],
                timeout: Duration::from_secs(20),
            }
            .spawn(&[read.as_fd()])
            .unwrap(),
        );
    }

    fn await_ready(&mut self) {
        let deadline = Instant::now() + WAIT;
        let ready = self.case.join("ready");
        while std::fs::read(&ready).ok().as_deref() != Some(b"ready\n") {
            let child = self.child.as_mut().unwrap();
            assert!(
                child.try_wait().unwrap().is_none(),
                "{}",
                std::fs::read_to_string(child.log()).unwrap()
            );
            assert!(
                Instant::now() < deadline,
                "private host never reported readiness"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn stopped(&mut self) -> (String, usize) {
        self.control.write_all(b"stop\n").unwrap();
        let child = self.child.as_mut().unwrap();
        let status = child.wait().unwrap();
        let text = std::fs::read_to_string(child.log()).unwrap();
        assert!(status.success(), "{text}");
        assert_eq!(
            text.lines()
                .filter(|line| *line == "sophia_m4_host ready")
                .count(),
            1,
            "{text}"
        );
        let stopped = text
            .lines()
            .filter(|line| line.starts_with("sophia_m4_host stopped "))
            .collect::<Vec<_>>();
        assert_eq!(stopped.len(), 1, "{text}");
        assert!(stopped[0].contains("service_joined=true"), "{text}");
        assert!(stopped[0].contains("interrupted=false"), "{text}");
        // This row tests host entry and its delegated control, without an X
        // peer. Public Session rows separately exercise real X connections.
        let field = |name: &str| {
            stopped[0]
                .split_whitespace()
                .find_map(|word| word.strip_prefix(name))
                .unwrap()
                .parse::<usize>()
                .unwrap()
        };
        let workers = field("workers=");
        assert_eq!(workers, 0, "{text}");
        assert_eq!(workers, field("workers_joined="), "{text}");
        self.child.take(); // releases the private descendant owner after wait
        (text, workers + 2) // the host process, its service thread and its workers
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.child.take());
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

pub fn containment() {
    let (mut fixture, read) = Fixture::prepare();
    let outside = fixture.directory.join("fabricated.sock");
    let listener = UnixListener::bind(&outside).unwrap();
    let mut authorized = UnixStream::connect(&outside).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    authorized.set_read_timeout(Some(WAIT)).unwrap();
    server.set_read_timeout(Some(WAIT)).unwrap();
    authorized.write_all(&COOKIE).unwrap();
    let mut credential = [0; 32];
    server.read_exact(&mut credential).unwrap();
    assert_eq!(credential, COOKIE);
    server.write_all(b"allowed").unwrap();
    let mut answer = [0; 7];
    authorized.read_exact(&mut answer).unwrap();
    assert_eq!(&answer, b"allowed");
    rustix::io::fcntl_setfd(&authorized, rustix::io::FdFlags::empty()).unwrap();
    fixture.launch(&read, &[]);
    fixture.await_ready();
    let (report, host_actors) = fixture.stopped();
    // The real host validated its inherited descriptors before starting:
    // only the pipe was delegated, although the connected socket was inheritable.
    assert!(report.contains("settlement_readable=true"));

    let probe_launch = Launch {
        source: "/work/source".into(),
        directory: fixture.directory.join("outside-probe"),
        command: vec![
            "/work/probe".into(),
            "--activation-fd".into(),
            "{activation_fd}".into(),
            "--outside".into(),
            outside.display().to_string(),
        ],
        mounts: vec![Mount {
            source: "/work/evidence/private-instance-probe".into(),
            destination: "/work/probe".into(),
            writable: false,
        }],
        timeout: Duration::from_secs(10),
    };
    let mut probe = probe_launch.spawn(&[]).unwrap();
    assert!(probe.wait().unwrap().success());
    let text = std::fs::read_to_string(probe.log()).unwrap();
    assert!(text.contains("\"outside_connected\":false"), "{text}");
    drop(probe);
    assert!(outside.exists()); // denial was not caused by removing the positive endpoint
    forged_entry(&fixture, &read);
    emit(
        "containment",
        &[
            "outside_endpoint",
            "outside_authorized_control",
            "unrelated_descriptor_closed",
            "delegated_descriptor_works",
            "forged_activation_refused",
        ],
        host_actors + 2,
    );
}

/// A real pipe and genuine descriptors for the *current* namespaces must not
/// impersonate a transition into fresh namespaces. Run the actual host here.
fn forged_entry(fixture: &Fixture, control: &File) {
    let namespaces = NAMESPACES.map(|name| {
        let file = File::open(format!("/proc/self/ns/{name}")).unwrap();
        rustix::io::fcntl_setfd(&file, rustix::io::FdFlags::empty()).unwrap();
        (name, file)
    });
    let (read, write) = rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC).unwrap();
    let entries = namespaces
        .iter()
        .map(|(name, file)| format!("\"{name}\":{}", file.as_raw_fd()))
        .collect::<Vec<_>>()
        .join(",");
    File::from(write)
        .write_all(format!("{{\"namespaces\":{{{entries}}},\"descriptors\":{{}}}}").as_bytes())
        .unwrap();
    rustix::io::fcntl_setfd(&read, rustix::io::FdFlags::empty()).unwrap();
    let output = File::create(fixture.directory.join("forged.log")).unwrap();
    let forged_ready = fixture.case.join("forged-ready");
    let forged_socket = fixture.case.join("forged.sock");
    let mut arguments = Fixture::arguments(control.as_raw_fd(), &read.as_raw_fd().to_string());
    for pair in arguments.chunks_mut(2) {
        let path = match pair[0].as_str() {
            "--socket" => Some(forged_socket.clone()),
            "--ready-file" => Some(forged_ready.clone()),
            "--cookie-file" => Some(fixture.case.join("cookie")),
            _ => None,
        };
        if let Some(path) = path {
            pair[1] = path.display().to_string();
        }
    }
    let mut child = Command::new(LOADER)
        .args(["--library-path", "/usr/lib", HOST])
        .args(arguments)
        .env_clear()
        .envs(ENVIRONMENT)
        .stdin(Stdio::null())
        .stdout(output.try_clone().unwrap())
        .stderr(output)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + WAIT;
    let mut forbidden_ready = false;
    let status = loop {
        if forged_ready.exists() && !forbidden_ready {
            forbidden_ready = true;
            // Even a negative that starts the forbidden service must collect
            // it before failing the assertion. The original control pipe is
            // valid, so the omission mutant has no unrelated setup excuse.
            (&fixture.control).write_all(b"stop\n").unwrap();
        }
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("forged entry did not terminate");
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    assert!(
        !forbidden_ready && !forged_socket.exists(),
        "forged activation started a real service"
    );
    assert!(!status.success());
    let text = std::fs::read_to_string(fixture.directory.join("forged.log")).unwrap();
    assert!(text.contains("did not cross the kernel"), "{text}");
    assert!(!text.contains("sophia_m4_host ready"));
}

pub fn no_ambient_fallback() {
    // xtask deliberately poisons only this test process. Its launcher must
    // clear the values before the real host validates entry and starts.
    assert_eq!(std::env::var("DISPLAY").unwrap(), ":64999");
    assert_eq!(
        std::env::var("WAYLAND_DISPLAY").unwrap(),
        "m4-fabricated-wayland"
    );
    assert_eq!(
        std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap(),
        "unix:path=/tmp/m4-fabricated-bus"
    );
    assert_eq!(
        std::env::var("XAUTHORITY").unwrap(),
        "/tmp/m4-fabricated-authority"
    );
    let (mut fixture, read) = Fixture::prepare();
    fixture.launch(&read, &[]);
    fixture.await_ready();
    let (_, host_actors) = fixture.stopped();
    // Entry validates absence of DRM/input paths and controlling terminal
    // before it can report Ready; no host-side device enumeration is used.
    let (mut refused, read) = Fixture::prepare();
    refused.launch(&read, &["--display", ":64999"]);
    let child = refused.child.as_mut().unwrap();
    assert!(!child.wait().unwrap().success());
    let text = std::fs::read_to_string(child.log()).unwrap();
    assert!(text.contains("ambient options are unsupported"), "{text}");
    assert!(!refused.case.join("private.sock").exists());
    emit(
        "no_ambient_fallback",
        &[
            "poisoned_environment",
            "ambient_options_refused",
            "no_devices",
            "no_controlling_terminal",
        ],
        host_actors + 1,
    );
}

fn emit(case: &str, subcases: &[&str], collected: usize) {
    let names = subcases
        .iter()
        .map(|name| format!("\"{name}\":\"PASS\""))
        .collect::<Vec<_>>()
        .join(",");
    // Child.wait includes the namespace monitors; the host additionally
    // asserts its own service/worker collection. The case process's complete
    // descendant inventory is independently checked by xtask.
    println!(
        "sophia_m4_acceptance {{\"schema\":1,\"case\":\"M4.{case}\",\"subcases\":{{{names}}},\"cleanup\":{{\"actors_started\":{collected},\"actors_collected\":{collected},\"pending_actors\":0,\"complete\":true}},\"observations\":{{\"actor_scope\":\"owned_children_service_threads_registered_workers\",\"real_host\":true}}}}"
    );
}
