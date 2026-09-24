//! Repository-owned desktop-comparison workloads.

use super::visibility::ProcessIdentity;
use super::{
    READY_TIMEOUT, ScheduledSample, elapsed_micros, parse_proc_stat, read_process_table,
    timing_population, write_new,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

const UNIX_SOCKET_PATH_MAX_BYTES: usize = 107;
const TERMINATION_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) struct WorkloadOwner {
    children: Vec<Child>,
    roots: Vec<u32>,
    observed_processes: BTreeMap<u32, u64>,
    subreaper: WorkloadSubreaper,
    sockets: Vec<PathBuf>,
    socket_namespace: Option<PathBuf>,
    firefox: Option<FixtureServer>,
    resize: Option<thread::JoinHandle<Result<Vec<u64>, String>>>,
    resize_requested: bool,
    launched_at: Instant,
    launch_usec: u64,
    settle_usec: u64,
    finished: bool,
}

impl WorkloadOwner {
    pub(super) fn launch(
        repo: &Path,
        attempt: &Path,
        scheduled: &ScheduledSample,
        duration: Duration,
    ) -> Result<Self, String> {
        let launched = Instant::now();
        let subreaper = WorkloadSubreaper::arm(Path::new("/proc"))?;
        let mut owner = Self {
            children: Vec::new(),
            roots: Vec::new(),
            observed_processes: BTreeMap::new(),
            subreaper,
            sockets: Vec::new(),
            socket_namespace: None,
            firefox: None,
            resize: None,
            resize_requested: scheduled.workload == "resize",
            launched_at: launched,
            launch_usec: 0,
            settle_usec: 0,
            finished: false,
        };
        if matches!(
            scheduled.workload.as_str(),
            "kitty-60s" | "resize" | "soak-2h" | "kitty-burst-16"
        ) {
            owner.socket_namespace = Some(create_kitty_socket_namespace()?);
        }
        match scheduled.workload.as_str() {
            "firefox-local" => owner.launch_firefox(repo, attempt)?,
            "kitty-burst-16" => {
                for index in 0..16 {
                    owner.launch_kitty(duration, index)?;
                }
                for socket in &owner.sockets {
                    wait_kitty(socket)?;
                }
            }
            "kitty-60s" | "resize" | "soak-2h" => {
                owner.launch_kitty(duration, 0)?;
                wait_kitty(&owner.sockets[0])?;
            }
            _ => return Err("prepared schedule contains an unknown workload".to_owned()),
        }
        owner.launch_usec = elapsed_micros(launched);
        owner.settle_usec = owner.launch_usec;
        Ok(owner)
    }

    fn launch_kitty(&mut self, duration: Duration, index: usize) -> Result<(), String> {
        let namespace = self
            .socket_namespace
            .as_deref()
            .ok_or("Kitty workload has no private runtime namespace")?;
        let socket = kitty_socket_path(namespace, index)?;
        let executable = std::env::current_exe()
            .map_err(|error| format!("could not identify xtask executable: {error}"))?;
        let child = Command::new("kitty")
            .args(["--config", "NONE"])
            .arg("--listen-on")
            .arg(format!("unix:{}", socket.display()))
            .arg("--override")
            .arg("allow_remote_control=yes")
            .arg("--class")
            .arg("sophia-desktop-comparison")
            .arg("--title")
            .arg(format!("Sophia comparison {index}"))
            .arg(executable)
            .args([
                "conformance",
                "desktop-comparison",
                "workload",
                "kitty-stream",
                &duration.as_secs().saturating_add(120).to_string(),
            ])
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("could not launch Kitty workload: {error}"))?;
        self.roots.push(child.id());
        self.children.push(child);
        self.sockets.push(socket);
        Ok(())
    }

    fn launch_firefox(&mut self, repo: &Path, attempt: &Path) -> Result<(), String> {
        let fixture =
            fs::read_to_string(repo.join("validation/desktop-comparison/firefox/index.html"))
                .map_err(|error| format!("could not read Firefox fixture: {error}"))?;
        let server = FixtureServer::start(fixture)?;
        let profile = attempt.join("firefox-profile");
        fs::create_dir(&profile)
            .map_err(|error| format!("could not create isolated Firefox profile: {error}"))?;
        fs::copy(
            repo.join("validation/desktop-comparison/firefox/user.js"),
            profile.join("user.js"),
        )
        .map_err(|error| format!("could not stage isolated Firefox profile: {error}"))?;
        let child = Command::new("firefox")
            .args(["--no-remote", "--new-instance", "--profile"])
            .arg(&profile)
            .arg(server.url())
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("could not launch Firefox workload: {error}"))?;
        self.roots.push(child.id());
        self.children.push(child);
        server.wait_ready()?;
        self.firefox = Some(server);
        Ok(())
    }

    pub(super) fn begin_measured_work(&mut self) -> Result<(), String> {
        if self.resize_requested {
            let socket = self.sockets[0].clone();
            self.resize = Some(thread::spawn(move || {
                let mut latencies = Vec::with_capacity(120);
                for index in 0..120 {
                    let started = Instant::now();
                    let (width, height) = if index % 2 == 0 {
                        ("1280", "720")
                    } else {
                        ("1600", "900")
                    };
                    let status = Command::new("kitty")
                        .args([
                            "@",
                            "--to",
                            &format!("unix:{}", socket.display()),
                            "resize-os-window",
                            "--unit",
                            "pixels",
                            "--width",
                            width,
                            "--height",
                            height,
                        ])
                        .status()
                        .map_err(|error| format!("could not execute resize workload: {error}"))?;
                    if !status.success() {
                        return Err("Kitty rejected a resize workload request".to_owned());
                    }
                    latencies.push(elapsed_micros(started).max(1));
                    thread::sleep(Duration::from_millis(50));
                }
                Ok(latencies)
            }));
        }
        Ok(())
    }

    pub(super) fn root_pids(&self) -> impl Iterator<Item = u32> + '_ {
        self.roots.iter().copied()
    }

    pub(super) fn adopted_population_roots(
        &self,
        processes: &BTreeMap<u32, super::ProcStat>,
    ) -> BTreeSet<u32> {
        self.subreaper
            .adopted_from_processes(processes)
            .into_keys()
            .collect()
    }

    pub(super) fn root_identities(&self) -> Result<Vec<ProcessIdentity>, String> {
        self.roots
            .iter()
            .copied()
            .map(ProcessIdentity::read)
            .collect()
    }

    pub(super) fn retain_processes(&mut self, processes: impl IntoIterator<Item = (u32, u64)>) {
        self.observed_processes.extend(processes);
    }

    pub(super) fn mark_visible(&mut self) {
        self.settle_usec = elapsed_micros(self.launched_at);
    }

    pub(super) fn exited_early(&mut self) -> Result<bool, String> {
        for child in &mut self.children {
            if child
                .try_wait()
                .map_err(|error| format!("could not poll comparison workload: {error}"))?
                .is_some()
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn finish(&mut self, attempt: &Path) -> Result<(), String> {
        let resize = match self.resize.take() {
            Some(handle) => handle
                .join()
                .map_err(|_| "resize workload thread panicked".to_owned())??,
            None => Vec::new(),
        };
        terminate_owned_processes(
            &mut self.children,
            &self.observed_processes,
            &self.subreaper,
        )?;
        self.firefox.take();
        self.cleanup_kitty_namespace();
        let (p50, p95, p99, maximum) = timing_population(&resize);
        write_new(
            &attempt.join("workload.log"),
            format!(
                "desktop_comparison_workload schema=1 status=complete launch_usec={} settle_usec={} resize_samples={} resize_p50_usec={} resize_p95_usec={} resize_p99_usec={} resize_max_usec={}\n",
                self.launch_usec,
                self.settle_usec,
                resize.len(),
                p50,
                p95,
                p99,
                maximum,
            )
            .as_bytes(),
        )?;
        self.finished = true;
        Ok(())
    }

    fn cleanup_kitty_namespace(&mut self) {
        for socket in &self.sockets {
            let _ = fs::remove_file(socket);
        }
        if let Some(namespace) = self.socket_namespace.take() {
            let _ = fs::remove_dir(namespace);
        }
    }
}

impl Drop for WorkloadOwner {
    fn drop(&mut self) {
        if !self.finished {
            let _ = terminate_owned_processes(
                &mut self.children,
                &self.observed_processes,
                &self.subreaper,
            );
            self.cleanup_kitty_namespace();
        }
    }
}

fn terminate_owned_processes(
    children: &mut [Child],
    observed_processes: &BTreeMap<u32, u64>,
    subreaper: &WorkloadSubreaper,
) -> Result<(), String> {
    signal_observed_processes(observed_processes, rustix::process::Signal::TERM);
    let group_result = terminate_owned_children(children);
    signal_observed_processes(observed_processes, rustix::process::Signal::KILL);

    let deadline = Instant::now() + TERMINATION_TIMEOUT;
    while Instant::now() < deadline {
        let adopted = subreaper.adopted_processes()?;
        signal_observed_processes(&adopted, rustix::process::Signal::KILL);
        reap_adopted_processes(&adopted);
        if observed_processes
            .iter()
            .all(|(&pid, &start_ticks)| !process_identity_is_live(pid, start_ticks))
            && subreaper.adopted_processes()?.is_empty()
        {
            return group_result;
        }
        thread::sleep(Duration::from_millis(25));
    }
    let survivors = observed_processes
        .iter()
        .filter(|&(&pid, &start_ticks)| process_identity_is_live(pid, start_ticks))
        .count()
        .saturating_add(subreaper.adopted_processes()?.len());
    let retained_result = if survivors == 0 {
        Ok(())
    } else {
        Err(format!(
            "{survivors} owned comparison workload processes survived bounded termination"
        ))
    };
    combine_termination_results(group_result, retained_result)
}

fn reap_adopted_processes(processes: &BTreeMap<u32, u64>) {
    for &pid in processes.keys() {
        let Some(pid) = rustix::process::Pid::from_raw(pid as i32) else {
            continue;
        };
        let _ = rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG);
    }
}

struct WorkloadSubreaper {
    controller: u32,
    original: Option<rustix::process::Pid>,
    preexisting_children: BTreeMap<u32, u64>,
}

impl WorkloadSubreaper {
    fn arm(proc_root: &Path) -> Result<Self, String> {
        let controller = std::process::id();
        let original = rustix::process::child_subreaper()
            .map_err(|error| format!("could not read comparison child-subreaper state: {error}"))?;
        rustix::process::set_child_subreaper(Some(rustix::process::getpid()))
            .map_err(|error| format!("could not own orphaned comparison processes: {error}"))?;
        let processes = match read_process_table(proc_root) {
            Ok(processes) => processes,
            Err(error) => {
                let _ = rustix::process::set_child_subreaper(original);
                return Err(error);
            }
        };
        let preexisting_children = direct_children(&processes, controller, &BTreeMap::new());
        Ok(Self {
            controller,
            original,
            preexisting_children,
        })
    }

    fn adopted_processes(&self) -> Result<BTreeMap<u32, u64>, String> {
        Ok(self.adopted_from_processes(&read_process_table(Path::new("/proc"))?))
    }

    fn adopted_from_processes(
        &self,
        processes: &BTreeMap<u32, super::ProcStat>,
    ) -> BTreeMap<u32, u64> {
        direct_children(processes, self.controller, &self.preexisting_children)
    }
}

impl Drop for WorkloadSubreaper {
    fn drop(&mut self) {
        let _ = rustix::process::set_child_subreaper(self.original);
    }
}

fn direct_children(
    processes: &BTreeMap<u32, super::ProcStat>,
    controller: u32,
    excluded: &BTreeMap<u32, u64>,
) -> BTreeMap<u32, u64> {
    let mut children = BTreeMap::new();
    for (&pid, stat) in processes {
        if stat.ppid != controller || excluded.get(&pid) == Some(&stat.start_ticks) {
            continue;
        }
        children.insert(pid, stat.start_ticks);
    }
    children
}

fn signal_observed_processes(
    observed_processes: &BTreeMap<u32, u64>,
    signal: rustix::process::Signal,
) {
    for (&pid, &start_ticks) in observed_processes {
        if !process_identity_is_live(pid, start_ticks) {
            continue;
        }
        if let Some(pid) = rustix::process::Pid::from_raw(pid as i32) {
            let _ = rustix::process::kill_process(pid, signal);
        }
    }
}

fn process_identity_is_live(pid: u32, expected_start_ticks: u64) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|source| parse_proc_stat(&source).ok())
        .is_some_and(|stat| stat.start_ticks == expected_start_ticks)
}

fn combine_termination_results(
    first: Result<(), String>,
    second: Result<(), String>,
) -> Result<(), String> {
    match (first, second) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(first), Err(second)) => Err(format!("{first}; {second}")),
    }
}

fn terminate_owned_children(children: &mut [Child]) -> Result<(), String> {
    let mut failures = Vec::new();
    for child in children {
        if let Err(error) = terminate_owned_child(child) {
            failures.push(error);
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn terminate_owned_child(child: &mut Child) -> Result<(), String> {
    let group = rustix::process::Pid::from_raw(child.id() as i32)
        .ok_or("comparison workload process-group ID is invalid")?;
    let leader_exited = child
        .try_wait()
        .map_err(|error| format!("could not poll owned comparison workload: {error}"))?
        .is_some();

    // The application and every helper start in a private group. TERM gives
    // toolkits a chance to close their X sockets; KILL then drains helpers that
    // outlive or were orphaned by the group leader. Session quiescence must not
    // depend on the behavior of an unowned descendant.
    let _ = rustix::process::kill_process_group(group, rustix::process::Signal::TERM);
    if leader_exited {
        thread::sleep(Duration::from_millis(25));
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        return Ok(());
    }

    let deadline = Instant::now() + TERMINATION_TIMEOUT;
    while Instant::now() < deadline {
        if child
            .try_wait()
            .map_err(|error| format!("could not poll owned comparison workload: {error}"))?
            .is_some()
        {
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
            return Ok(());
        }
        thread::sleep(Duration::from_millis(25));
    }

    let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
    child
        .wait()
        .map_err(|error| format!("could not reap owned comparison workload: {error}"))?;
    Ok(())
}

fn create_kitty_socket_namespace() -> Result<PathBuf, String> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or("XDG_RUNTIME_DIR is unset; Kitty control sockets need a private runtime")?;
    if !runtime.is_absolute() {
        return Err("XDG_RUNTIME_DIR must be absolute for Kitty control sockets".to_owned());
    }
    let root = runtime.join("sophia-desktop-comparison");
    fs::create_dir_all(&root)
        .map_err(|error| format!("could not create Kitty runtime root: {error}"))?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("could not protect Kitty runtime root: {error}"))?;
    let namespace = root.join(format!("workload-{}", std::process::id()));
    fs::create_dir(&namespace)
        .map_err(|error| format!("could not create private Kitty socket namespace: {error}"))?;
    fs::set_permissions(&namespace, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("could not protect Kitty socket namespace: {error}"))?;
    Ok(namespace)
}

fn kitty_socket_path(namespace: &Path, index: usize) -> Result<PathBuf, String> {
    let socket = namespace.join(format!("kitty-{index}.sock"));
    let bytes = socket.as_os_str().as_bytes().len();
    if bytes > UNIX_SOCKET_PATH_MAX_BYTES {
        return Err(format!(
            "Kitty control socket path is {bytes} bytes; Linux permits at most {UNIX_SOCKET_PATH_MAX_BYTES}"
        ));
    }
    Ok(socket)
}

struct FixtureServer {
    address: std::net::SocketAddr,
    ready: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl FixtureServer {
    fn start(fixture: String) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|error| format!("could not bind local Firefox fixture: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("could not configure local Firefox fixture: {error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("could not read local Firefox fixture address: {error}"))?;
        let ready = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_ready = Arc::clone(&ready);
        let thread_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = serve_fixture(stream, &fixture, &thread_ready);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            address,
            ready,
            stop,
            thread: Some(handle),
        })
    }

    fn url(&self) -> String {
        format!("http://{}/", self.address)
    }

    fn wait_ready(&self) -> Result<(), String> {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if self.ready.load(Ordering::Relaxed) {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(20));
        }
        Err("Firefox fixture did not report readiness within 30 seconds".to_owned())
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.address);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn serve_fixture(
    mut stream: TcpStream,
    fixture: &str,
    ready: &AtomicBool,
) -> Result<(), std::io::Error> {
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    let mut request = [0u8; 2048];
    let count = stream.read(&mut request)?;
    let first = std::str::from_utf8(&request[..count])
        .ok()
        .and_then(|source| source.lines().next())
        .unwrap_or_default();
    let is_ready = first.starts_with("GET /ready ");
    if is_ready {
        ready.store(true, Ordering::Relaxed);
    }
    let body = if is_ready { "ready" } else { fixture };
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        if is_ready {
            "text/plain"
        } else {
            "text/html; charset=utf-8"
        },
        body.len(),
        body,
    )
}

fn wait_kitty(socket: &Path) -> Result<(), String> {
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        if Command::new("kitty")
            .args(["@", "--to", &format!("unix:{}", socket.display()), "ls"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(20));
    }
    Err("Kitty workload did not become remotely controllable within 30 seconds".to_owned())
}

pub fn run_stream(seconds: u64) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut state = 0x9e37_79b9_7f4a_7c15u64 ^ u64::from(std::process::id());
    let mut sequence = 0u64;
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    while Instant::now() < deadline {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        sequence = sequence.saturating_add(1);
        writeln!(output, "{sequence:08} {state:016x}")
            .and_then(|()| output.flush())
            .map_err(|error| format!("Kitty stream output failed: {error}"))?;
        thread::sleep(Duration::from_millis(16));
    }
    Ok(())
}

mod tests;
