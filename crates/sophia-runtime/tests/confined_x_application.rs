//! Device-free controls for the confined X application command.
//!
//! Validation is checked without spawning anything. With Bubblewrap present,
//! the command is run for real against this test binary as the application:
//! the `confined_x_probe` test below, ignored in ordinary runs, reports what
//! it can reach from inside, and the parent asserts it. The probe proves
//! "cannot reach" as absence (ENOENT, a refused abstract connect), not denial.

use std::ffi::OsString;
use std::io::{Read as _, Write as _};
use std::os::fd::AsRawFd as _;
use std::os::linux::net::SocketAddrExt as _;
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use sophia_runtime::{
    CONFINED_X_AUTHORITY_FILE, CONFINED_X_SOCKET_DIRECTORY, ConfinedXApplicationError,
    ConfinedXApplicationLaunch, ConfinedXPathFault, DEFAULT_BUBBLEWRAP_PATH,
};

const COOKIE: &[u8] = b"sophia-confined-cookie";
const ENOENT: &str = "errno2";
const EROFS: &str = "errno30";
const ECONNREFUSED: &str = "errno111";

/// One group layout under this test's own scratch directory: the launch
/// group, a second confined group and the shared group, each holding a
/// bound socket, plus the cookie.
struct Fixture {
    root: PathBuf,
    display: u32,
    group: PathBuf,
    other: PathBuf,
    shared: PathBuf,
    cookie: PathBuf,
    _listeners: Vec<UnixListener>,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let display = 20_000 + std::process::id() % 20_000;
        let root = std::env::temp_dir().join(format!("scx-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let make = |path: &Path| {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(path)
                .unwrap();
        };
        make(&root);
        let mut listeners = Vec::new();
        let mut group = |name: &str| {
            let directory = root.join(name);
            make(&directory);
            let socket = directory.join(format!("X{display}"));
            assert!(
                socket.as_os_str().len() < 100,
                "fixture socket path too long: {}",
                socket.display()
            );
            listeners.push(UnixListener::bind(socket).unwrap());
            directory
        };
        let (group, other, shared) = (group("confined-7"), group("confined-3"), group("shared"));
        let cookie = root.join("Xauthority");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&cookie)
            .unwrap()
            .write_all(COOKIE)
            .unwrap();
        Self {
            root,
            display,
            group,
            other,
            shared,
            cookie,
            _listeners: listeners,
        }
    }

    fn launch(
        &self,
        executable: impl Into<PathBuf>,
        args: Vec<OsString>,
    ) -> ConfinedXApplicationLaunch {
        ConfinedXApplicationLaunch {
            executable: executable.into(),
            args,
            bubblewrap: DEFAULT_BUBBLEWRAP_PATH.into(),
            group_directory: self.group.clone(),
            cookie_file: self.cookie.clone(),
            display_number: self.display,
        }
    }

    fn socket(&self, directory: &Path) -> PathBuf {
        directory.join(format!("X{}", self.display))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn require_bubblewrap() {
    assert!(
        Path::new(DEFAULT_BUBBLEWRAP_PATH).is_file(),
        "confinement proof requires Bubblewrap"
    );
}

fn group_fault(launch: &ConfinedXApplicationLaunch) -> ConfinedXPathFault {
    match launch.command() {
        Err(ConfinedXApplicationError::GroupDirectory { fault, .. }) => fault,
        other => panic!("expected a group-directory refusal, got {other:?}"),
    }
}

fn cookie_fault(launch: &ConfinedXApplicationLaunch) -> ConfinedXPathFault {
    match launch.command() {
        Err(ConfinedXApplicationError::CookieFile { fault, .. }) => fault,
        other => panic!("expected a cookie refusal, got {other:?}"),
    }
}

#[test]
fn a_group_directory_is_refused_unless_it_holds_exactly_its_socket() {
    let fixture = Fixture::new("group-refusals");
    let launch = fixture.launch("/usr/bin/true", Vec::new());

    let planted = fixture.group.join("planted");
    std::fs::write(&planted, b"").unwrap();
    assert!(
        matches!(group_fault(&launch), ConfinedXPathFault::Occupied(name) if name == "planted")
    );
    std::fs::remove_file(&planted).unwrap();

    std::fs::set_permissions(&fixture.group, std::fs::Permissions::from_mode(0o750)).unwrap();
    assert!(matches!(
        group_fault(&launch),
        ConfinedXPathFault::Reachable(0o750)
    ));
    std::fs::set_permissions(&fixture.group, std::fs::Permissions::from_mode(0o700)).unwrap();

    let socket = fixture.socket(&fixture.group);
    std::fs::rename(&socket, fixture.root.join("parked")).unwrap();
    assert!(matches!(
        group_fault(&launch),
        ConfinedXPathFault::MissingSocket
    ));
    std::os::unix::fs::symlink(fixture.socket(&fixture.shared), &socket).unwrap();
    assert!(matches!(
        group_fault(&launch),
        ConfinedXPathFault::SymbolicLink
    ));
    std::fs::remove_file(&socket).unwrap();
    std::fs::write(&socket, b"").unwrap();
    assert!(matches!(
        group_fault(&launch),
        ConfinedXPathFault::NotASocket
    ));
    std::fs::remove_file(&socket).unwrap();
    std::fs::rename(fixture.root.join("parked"), &socket).unwrap();

    let linked = fixture.root.join("linked-group");
    std::os::unix::fs::symlink(&fixture.group, &linked).unwrap();
    let mut through_link = fixture.launch("/usr/bin/true", Vec::new());
    through_link.group_directory = linked;
    assert!(matches!(
        group_fault(&through_link),
        ConfinedXPathFault::SymbolicLink
    ));
    through_link.group_directory = PathBuf::from("relative/confined-7");
    assert!(matches!(
        group_fault(&through_link),
        ConfinedXPathFault::Relative
    ));
}

#[test]
fn a_cookie_is_refused_unless_it_is_an_owner_only_file() {
    let fixture = Fixture::new("cookie-refusals");
    let mut launch = fixture.launch("/usr/bin/true", Vec::new());

    std::fs::set_permissions(&fixture.cookie, std::fs::Permissions::from_mode(0o640)).unwrap();
    assert!(matches!(
        cookie_fault(&launch),
        ConfinedXPathFault::Reachable(0o640)
    ));
    std::fs::set_permissions(&fixture.cookie, std::fs::Permissions::from_mode(0o600)).unwrap();

    let linked = fixture.root.join("linked-cookie");
    std::os::unix::fs::symlink(&fixture.cookie, &linked).unwrap();
    launch.cookie_file = linked;
    assert!(matches!(
        cookie_fault(&launch),
        ConfinedXPathFault::SymbolicLink
    ));
    // A directory is not a cookie file: only the file is ever bound.
    launch.cookie_file = fixture.root.clone();
    assert!(matches!(
        cookie_fault(&launch),
        ConfinedXPathFault::NotARegularFile
    ));
}

fn position(args: &[String], window: &[&str]) -> Option<usize> {
    args.windows(window.len())
        .position(|candidate| candidate == window)
}

#[test]
fn the_command_grants_the_group_and_cookie_read_only_and_nothing_else() {
    require_bubblewrap();
    let fixture = Fixture::new("arguments");
    let command = fixture
        .launch("/usr/bin/true", Vec::new())
        .command()
        .unwrap();
    assert_eq!(command.get_program(), Path::new(DEFAULT_BUBBLEWRAP_PATH));
    // No explicit variables on the cleared outer environment; sandbox
    // variables come from the three --setenv entries below.
    assert_eq!(command.get_envs().count(), 0);
    let args = command
        .get_args()
        .map(|arg| arg.to_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let group = fixture.group.to_str().unwrap();
    let cookie = fixture.cookie.to_str().unwrap();
    let display = format!(":{}", fixture.display);
    let windows: &[&[&str]] = &[
        &["--ro-bind", group, CONFINED_X_SOCKET_DIRECTORY],
        &["--ro-bind", cookie, CONFINED_X_AUTHORITY_FILE],
        &["--setenv", "DISPLAY", display.as_str()],
        &["--setenv", "XAUTHORITY", CONFINED_X_AUTHORITY_FILE],
        &["--setenv", "PATH", "/usr/bin"],
        &["--tmpfs", "/tmp"],
        &["--proc", "/proc"],
        &["--dev", "/dev"],
        &["--dir", "/run"],
        &["--unshare-net"],
        &["--unshare-pid"],
        &["--clearenv"],
    ];
    for window in windows {
        assert!(
            position(&args, window).is_some(),
            "missing {window:?} in {args:?}"
        );
    }
    // The private /tmp comes first, so the group mount lands on top of it.
    assert!(position(&args, &["--tmpfs", "/tmp"]) < position(&args, &["--ro-bind", group]));
    for forbidden in ["--bind", "--dev-bind", "--share-net"] {
        assert!(
            !args.iter().any(|arg| arg == forbidden),
            "{forbidden} in {args:?}"
        );
    }
    let shared = fixture.shared.to_str().unwrap();
    let root = fixture.root.to_str().unwrap();
    let setenvs = args.iter().filter(|arg| *arg == "--setenv").count();
    assert_eq!(setenvs, 3, "{args:?}");
    for arg in &args {
        assert!(!arg.starts_with(shared), "{arg}");
        assert!(!arg.contains("/run/user"), "{arg}");
        // The only paths under the fixture are the two grants' sources.
        if arg.starts_with(root) {
            assert!(arg == group || arg == cookie, "{arg}");
        }
    }
}

/// Run the probe and collect its `probe <key> <value>` lines.
fn run_probe(mut command: std::process::Command) -> std::collections::BTreeMap<String, String> {
    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "probe failed: {}\n{stdout}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
        .lines()
        .filter_map(|line| line.rsplit_once("probe ").map(|(_, report)| report))
        .filter_map(|line| line.split_once(' '))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

/// The probe's arguments. Only a confined run gets the display, which is
/// what turns on the checks that try to write: run on the host they would
/// plant files in the real `/tmp/.X11-unix`.
fn probe_args(fixture: Option<&Fixture>, sentinel: i32) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "confined_x_probe",
        "--exact",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    args.push(format!("probe-fd={sentinel}").into());
    let Some(fixture) = fixture else {
        return args;
    };
    args.push(format!("probe-display={}", fixture.display).into());
    for absent in [
        fixture.socket(&fixture.shared),
        fixture.shared.clone(),
        fixture.socket(&fixture.other),
        fixture.other.clone(),
        fixture.cookie.clone(),
        fixture.root.clone(),
        PathBuf::from("/run/user"),
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")),
    ] {
        let mut arg = OsString::from("probe-absent=");
        arg.push(absent);
        args.push(arg);
    }
    args
}

/// A descriptor the parent holds without close-on-exec: inheritable by any
/// child that does not mark it.
fn inheritable_sentinel() -> std::fs::File {
    let sentinel = std::fs::File::open("/dev/null").unwrap();
    rustix::io::fcntl_setfd(&sentinel, rustix::io::FdFlags::empty()).unwrap();
    assert!(
        !rustix::io::fcntl_getfd(&sentinel)
            .unwrap()
            .contains(rustix::io::FdFlags::CLOEXEC)
    );
    sentinel
}

fn assert_sentinel_unchanged(sentinel: &mut std::fs::File) {
    assert!(
        !rustix::io::fcntl_getfd(&*sentinel)
            .unwrap()
            .contains(rustix::io::FdFlags::CLOEXEC)
    );
    let mut buffer = [0_u8; 1];
    assert_eq!(sentinel.read(&mut buffer).unwrap(), 0);
}

#[test]
fn the_version_probe_also_excludes_host_environment_and_descriptors() {
    let fixture = Fixture::new("version");
    let mut sentinel = inheritable_sentinel();
    let fake = fixture.root.join("bwrap");
    let script = format!(
        "#!/bin/sh\n/usr/bin/env > \"$0.env\"\nif test -e /proc/$$/fd/{}; then echo present; else echo absent; fi > \"$0.fd\"\nprintf 'bubblewrap 0.11.2\\n'\n",
        sentinel.as_raw_fd()
    );
    std::fs::write(&fake, script).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
    // The existing public probe still inherits its caller's environment and
    // descriptors. It supplies the control for the confined probe below.
    assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
    sophia_runtime::bubblewrap_version(&fake).unwrap();
    let environment = fake.with_extension("env");
    let descriptor = fake.with_extension("fd");
    assert!(
        std::fs::read_to_string(&environment)
            .unwrap()
            .contains("CARGO_MANIFEST_DIR=")
    );
    assert_eq!(
        std::fs::read_to_string(&descriptor).unwrap().trim(),
        "present"
    );

    let mut launch = fixture.launch("/usr/bin/true", Vec::new());
    launch.bubblewrap = fake;
    let _command = launch.command().unwrap();
    let observed = std::fs::read_to_string(environment).unwrap();
    assert!(
        !observed.lines().any(|line| line.starts_with("CARGO_")
            || line.starts_with("HOME=")
            || line.starts_with("LD_")),
        "{observed}"
    );
    assert_eq!(
        std::fs::read_to_string(descriptor).unwrap().trim(),
        "absent"
    );
    assert_sentinel_unchanged(&mut sentinel);
}

#[test]
fn the_cloexec_hook_withholds_an_inheritable_descriptor_and_leaves_the_parent_alone() {
    let mut sentinel = inheritable_sentinel();
    let fd = sentinel.as_raw_fd();
    let executable = std::env::current_exe().unwrap();

    // Control: without the hook the sentinel reaches the child, so its
    // absence below is the hook's doing and not the descriptor's own flag.
    let mut plain = std::process::Command::new(&executable);
    plain.args(probe_args(None, fd));
    assert_eq!(run_probe(plain)["sentinel"], "ok");

    let mut hooked = std::process::Command::new(&executable);
    hooked.args(probe_args(None, fd));
    sophia_linux_peer::cloexec_inherited_fds_on_exec(&mut hooked);
    assert_eq!(run_probe(hooked)["sentinel"], ENOENT);
    assert_sentinel_unchanged(&mut sentinel);
}

#[test]
fn a_confined_application_reaches_its_own_socket_and_nothing_else() {
    require_bubblewrap();
    let fixture = Fixture::new("sandbox");
    // A live abstract endpoint under the name X clients try first. Reachable
    // from the host, so its absence inside is the network namespace's doing.
    let abstract_name = format!("/tmp/.X11-unix/X{}", fixture.display);
    let abstract_listener =
        UnixListener::bind_addr(&SocketAddr::from_abstract_name(&abstract_name).unwrap()).unwrap();
    UnixStream::connect_addr(&SocketAddr::from_abstract_name(&abstract_name).unwrap()).unwrap();
    let mut sentinel = inheritable_sentinel();
    let fd = sentinel.as_raw_fd();

    let command = fixture
        .launch(
            std::env::current_exe().unwrap(),
            probe_args(Some(&fixture), fd),
        )
        .command()
        .unwrap();
    let report = run_probe(command);

    assert_eq!(report["own_connect"], "ok", "{report:?}");
    assert_eq!(report["socket_directory"], format!("X{}", fixture.display));
    assert_eq!(report["abstract_connect"], ECONNREFUSED, "{report:?}");
    for (key, value) in &report {
        if key.starts_with("absent:") {
            assert_eq!(value, ENOENT, "{key} {report:?}");
        }
    }
    for key in [
        "plant_file",
        "plant_directory",
        "unlink_socket",
        "rename_socket",
        "write_cookie",
    ] {
        assert_eq!(report[key], EROFS, "{key} {report:?}");
    }
    assert_eq!(report["cookie"], String::from_utf8_lossy(COOKIE));
    assert_eq!(report["display"], format!(":{}", fixture.display));
    assert_eq!(report["xauthority"], CONFINED_X_AUTHORITY_FILE);
    assert_eq!(report["environment"], "DISPLAY,PATH,PWD,XAUTHORITY");
    assert_eq!(report["pwd"], "/");
    assert_eq!(report["sentinel"], ENOENT);
    assert_eq!(report["descriptors"], "0,1,2", "{report:?}");

    // Nothing inside changed the group, and the parent's sentinel is intact.
    let entries = std::fs::read_dir(&fixture.group)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(entries, [OsString::from(format!("X{}", fixture.display))]);
    assert_sentinel_unchanged(&mut sentinel);
    drop(abstract_listener);
}

fn outcome(result: std::io::Result<()>) -> String {
    match result {
        Ok(()) => "ok".to_owned(),
        Err(error) => error
            .raw_os_error()
            .map_or_else(|| format!("other:{error}"), |code| format!("errno{code}")),
    }
}

/// The application side. Does nothing unless launched with probe arguments.
#[test]
#[ignore = "runs only as the confined application's probe"]
fn confined_x_probe() {
    let args = std::env::args().collect::<Vec<_>>();
    let values = |key: &str| {
        args.iter()
            .filter_map(|arg| arg.strip_prefix(key).map(str::to_owned))
            .collect::<Vec<_>>()
    };
    let report = |key: &str, value: String| println!("probe {key} {value}");
    if let Some(fd) = values("probe-fd=").pop() {
        report(
            "sentinel",
            outcome(std::fs::read_link(format!("/proc/self/fd/{fd}")).map(drop)),
        );
    }
    let Some(display) = values("probe-display=").pop() else {
        return;
    };
    let directory = Path::new(CONFINED_X_SOCKET_DIRECTORY);
    let socket = directory.join(format!("X{display}"));

    report(
        "own_connect",
        outcome(UnixStream::connect(&socket).map(drop)),
    );
    report(
        "abstract_connect",
        outcome(
            SocketAddr::from_abstract_name(format!("/tmp/.X11-unix/X{display}"))
                .and_then(|address| UnixStream::connect_addr(&address))
                .map(drop),
        ),
    );
    for absent in values("probe-absent=") {
        report(
            &format!("absent:{absent}"),
            outcome(std::fs::symlink_metadata(&absent).map(drop)),
        );
    }
    let entries = std::fs::read_dir(directory).map(|entries| {
        let mut names = entries
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        names.sort();
        names.join(",")
    });
    report(
        "socket_directory",
        entries.unwrap_or_else(|error| format!("error:{error}")),
    );
    report(
        "plant_file",
        outcome(std::fs::write(directory.join("planted"), b"").map(drop)),
    );
    report(
        "plant_directory",
        outcome(std::fs::create_dir(directory.join("planted-dir"))),
    );
    report("unlink_socket", outcome(std::fs::remove_file(&socket)));
    report(
        "rename_socket",
        outcome(std::fs::rename(&socket, directory.join("moved"))),
    );

    let xauthority = std::env::var("XAUTHORITY").unwrap_or_default();
    report(
        "cookie",
        std::fs::read(&xauthority)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_else(|error| format!("error:{error}")),
    );
    report(
        "write_cookie",
        outcome(
            std::fs::OpenOptions::new()
                .write(true)
                .open(&xauthority)
                .map(drop),
        ),
    );
    report("display", std::env::var("DISPLAY").unwrap_or_default());
    report("xauthority", xauthority);
    report("pwd", std::env::var("PWD").unwrap_or_default());
    let mut environment = std::env::vars_os()
        .map(|(key, _)| key.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    environment.sort();
    report("environment", environment.join(","));

    // Every open descriptor except the one listing them.
    let listing = std::fs::read_dir("/proc/self/fd").unwrap();
    let mut descriptors = listing
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().to_string_lossy().into_owned();
            let target = std::fs::read_link(format!("/proc/self/fd/{name}")).ok()?;
            (!target.starts_with("/proc")).then(|| name.parse::<i32>().unwrap())
        })
        .collect::<Vec<_>>();
    descriptors.sort_unstable();
    report(
        "descriptors",
        descriptors
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(","),
    );
}
