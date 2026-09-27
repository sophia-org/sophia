//! The production protected launcher around the verified Bemenu copy:
//! ProcessSupervisor (Shell role) with a Bubblewrap ProtectionDomainSpec, the
//! production DEFAULT_BUBBLEWRAP_PATH lookup, and the real supervisor evidence.
//!
//! ProcessLaunchSpec cannot redirect stdio, so the program is /usr/bin/sh with
//! quoted positional arguments that only exec: `env -i` leaves exactly PATH and
//! SOPHIA_SHELL_9P_SOCKET (no shell-added PWD/SHLVL), then `nice -n 19` execs
//! the binary with stdout/stderr on files in a bound log directory. Every exec
//! keeps the PID, so the evidence's peer PID is Bemenu's; `verify_domain`
//! proves that after exec rather than assuming it.
//!
//! Fonts: Bemenu's native entry adds only /usr/share/fonts and
//! /usr/local/share/fonts to a private Fontconfig configuration (no /etc/fonts,
//! no home, FONTCONFIG_FILE never read). The domain mounts a directory holding
//! one in-tree test font at /usr/share/fonts, an empty directory over
//! /usr/local/share/fonts when the host has one, and has no /etc/fonts at all,
//! so the application's configured font directories contain only the fixture.
//! Other files under the supervisor's read-only /usr remain reachable; this
//! does not claim a filesystem-wide prohibition on other font files.
use sophia_runtime::*;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const FONT: &str = "JetBrainsMonoNL-Regular.ttf";
const FONT_SHA256: &str = "fb3b2575d7b0657359707993288f12a7360344d39387bb26050e276d61f6bd2a";
const LOCAL_FONTS: &str = "/usr/local/share/fonts";
const LOG_CAP: u64 = 64 * 1024;
const EXEC_TIMEOUT: Duration = Duration::from_secs(15);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const LAUNCHER: &str = "exec /usr/bin/env -i PATH=/usr/bin SOPHIA_SHELL_9P_SOCKET=\"$2\" \
     /usr/bin/nice -n 19 \"$1\" --serve >\"$3\" 2>\"$4\"";

pub struct Peer {
    supervisor: ProcessSupervisor,
    evidence: ProtectionDomainEvidence,
    stdout: PathBuf,
    stderr: PathBuf,
    exited: bool,
}

/// One pinned in-tree test font, copied into a private directory.
pub fn fonts(repo: &Path, directory: &Path) -> PathBuf {
    let bytes = std::fs::read(repo.join("assets/fonts").join(FONT)).unwrap();
    assert_eq!(
        super::artifact::sha256(&bytes),
        FONT_SHA256,
        "test font pin"
    );
    std::fs::create_dir(directory).unwrap();
    std::fs::write(directory.join(FONT), bytes).unwrap();
    std::fs::set_permissions(directory.join(FONT), std::fs::Permissions::from_mode(0o444)).unwrap();
    directory.to_path_buf()
}

pub fn launch(root: &Path, binary: &Path, socket: &Path, fonts: &Path) -> Peer {
    let logs = root.join("logs");
    std::fs::create_dir(&logs).unwrap();
    let stdout = logs.join("stdout");
    let stderr = logs.join("stderr");
    let mut domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])
        .unwrap()
        .inherited_fds([])
        .unwrap()
        .path(ProtectionPath::read_only(binary.parent().unwrap()))
        .unwrap()
        .path(ProtectionPath::read_only(socket.parent().unwrap()))
        .unwrap()
        .path(ProtectionPath::read_write(&logs))
        .unwrap()
        .path(ProtectionPath::read_only_at(fonts, "/usr/share/fonts"))
        .unwrap();
    if Path::new(LOCAL_FONTS).exists() {
        let empty = root.join("no-fonts");
        std::fs::create_dir(&empty).unwrap();
        domain = domain
            .path(ProtectionPath::read_only_at(empty, LOCAL_FONTS))
            .unwrap();
    }
    let spec = ProcessLaunchSpec::new("/usr/bin/sh")
        .arg("-c")
        .arg(LAUNCHER)
        .arg("bemenu-launch")
        .arg(binary)
        .arg(socket)
        .arg(&stdout)
        .arg(&stderr)
        .protection_domain(domain);
    let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::Shell, spec);
    supervisor
        .apply(SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::Shell,
            delay: Duration::ZERO,
        })
        .unwrap_or_else(|e| panic!("bemenu live gate: protected launch failed: {e:?}"));
    let evidence = supervisor
        .protection_evidence()
        .cloned()
        .expect("bemenu live gate: protected launch has no evidence");
    assert_eq!(evidence.backend, ProtectionBackendKind::Bubblewrap);
    assert!(
        evidence
            .roles
            .contains(&ProtectionDomainRole::MetadataShell)
    );
    assert_eq!(supervisor.peer_id(), Some(evidence.peer_pid));
    Peer {
        supervisor,
        evidence,
        stdout,
        stderr,
        exited: false,
    }
}

fn proc(pid: u32, rest: &str) -> PathBuf {
    PathBuf::from(format!("/proc/{pid}/{rest}"))
}

fn names(directory: &Path) -> Vec<String> {
    let mut names = std::fs::read_dir(directory)
        .unwrap_or_else(|e| panic!("{}: {e}", directory.display()))
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    names
}

impl Peer {
    pub fn evidence(&self) -> &ProtectionDomainEvidence {
        &self.evidence
    }

    pub fn pid(&self) -> u32 {
        self.evidence.peer_pid
    }

    pub fn stderr(&self) -> String {
        std::fs::read_to_string(&self.stderr).unwrap_or_default()
    }

    /// Panics on early exit or oversized output; the supervisor then kills and
    /// reaps the domain when this value is dropped.
    pub fn check(&mut self) {
        if !self.exited
            && let Some(event) = self.supervisor.poll().unwrap()
        {
            assert_eq!(event, SupervisorEvent::ProcessExited);
            self.exited = true;
            panic!("bemenu exited early:\n{}", self.stderr());
        }
        for path in [&self.stdout, &self.stderr] {
            let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            assert!(size < LOG_CAP, "bemenu output exceeded {LOG_CAP} bytes");
        }
    }

    /// Prove what actually runs inside the domain, after every exec:
    /// the verified copy with exactly one endpoint at nice 19, as PID 1 of a
    /// private PID namespace, with one font visible and no host devices.
    pub fn verify_domain(&mut self, binary: &Path, socket: &Path, fonts: &Path) {
        let pid = self.pid();
        let expected = std::fs::metadata(binary).unwrap();
        let deadline = Instant::now() + EXEC_TIMEOUT;
        loop {
            self.check();
            if std::fs::metadata(proc(pid, "exe"))
                .is_ok_and(|m| (m.dev(), m.ino()) == (expected.dev(), expected.ino()))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "bemenu live gate: PID {pid} never executed the verified copy"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let cmdline = std::fs::read(proc(pid, "cmdline")).unwrap();
        let mut want = binary.as_os_str().as_encoded_bytes().to_vec();
        want.extend_from_slice(b"\0--serve\0");
        assert_eq!(cmdline, want, "bemenu argv");
        let environ = std::fs::read(proc(pid, "environ")).unwrap();
        let mut environ = environ
            .split(|b| *b == 0)
            .filter(|v| !v.is_empty())
            .map(|v| String::from_utf8(v.to_vec()).unwrap())
            .collect::<Vec<_>>();
        environ.sort();
        assert_eq!(
            environ,
            [
                "PATH=/usr/bin".to_owned(),
                format!("SOPHIA_SHELL_9P_SOCKET={}", socket.display())
            ],
            "bemenu environment: exactly one endpoint"
        );
        let stat = std::fs::read_to_string(proc(pid, "stat")).unwrap();
        let fields = stat
            .rsplit_once(") ")
            .unwrap()
            .1
            .split(' ')
            .collect::<Vec<_>>();
        assert_eq!(fields[16], "19", "bemenu nice value");
        let status = std::fs::read_to_string(proc(pid, "status")).unwrap();
        let nspid = status
            .lines()
            .find_map(|line| line.strip_prefix("NSpid:"))
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>();
        assert!(
            nspid.len() >= 2 && nspid.last() == Some(&"1"),
            "bemenu is not PID 1 of a private PID namespace: {nspid:?}"
        );
        let mountinfo = std::fs::read_to_string(proc(pid, "mountinfo")).unwrap();
        assert!(
            mountinfo
                .lines()
                .any(|line| line.split(' ').nth(4) == Some("/usr/share/fonts")),
            "no font mount in the domain"
        );
        let root = proc(pid, "root");
        assert_eq!(
            names(&root.join("usr/share/fonts")),
            [FONT],
            "visible fonts"
        );
        assert_eq!(
            std::fs::read(root.join("usr/share/fonts").join(FONT)).unwrap(),
            std::fs::read(fonts.join(FONT)).unwrap()
        );
        let local = root.join("usr/local/share/fonts");
        assert!(
            !local.exists() || names(&local).is_empty(),
            "local fonts visible"
        );
        assert!(!root.join("etc/fonts").exists(), "/etc/fonts visible");
        // Only a bound scratch path may appear under /home (when TMPDIR is there).
        let bound = socket
            .strip_prefix("/home")
            .ok()
            .and_then(|rest| rest.iter().next())
            .map(|first| first.to_string_lossy().into_owned());
        assert!(
            names(&root.join("home"))
                .iter()
                .all(|name| Some(name) == bound.as_ref()),
            "home visible"
        );
        let devices = names(&root.join("dev"));
        for hidden in ["dri", "input", "fb0", "snd", "uinput", "tty0"] {
            assert!(
                !devices.iter().any(|d| d == hidden),
                "device {hidden} visible: {devices:?}"
            );
        }
    }

    /// Graceful stop through Bemenu's SIGTERM handler, bounded; returns
    /// (stdout, stderr). Failure leaves the kill and reap to the supervisor.
    pub fn stop(&mut self) -> (String, String) {
        let pid = rustix::process::Pid::from_raw(self.pid() as i32).unwrap();
        rustix::process::kill_process(pid, rustix::process::Signal::TERM).unwrap();
        let deadline = Instant::now() + STOP_TIMEOUT;
        while !self.exited {
            if let Some(event) = self.supervisor.poll().unwrap() {
                assert_eq!(event, SupervisorEvent::ProcessExited);
                self.exited = true;
            }
            assert!(
                Instant::now() < deadline,
                "bemenu ignored SIGTERM:\n{}",
                self.stderr()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        (
            std::fs::read_to_string(&self.stdout).unwrap(),
            self.stderr(),
        )
    }
}
