use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sophia_config::{ConfigDomain, ConfigGeneration, DesktopAuthority};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(super) fn run(arguments: &[String]) -> Result<()> {
    let mut profile = None;
    let mut default_wm = None;
    let mut policy_checker = None;
    let mut allow_deferred = false;
    for argument in arguments {
        if let Some(value) = argument.strip_prefix("--desktop-profile=") {
            if profile.replace(PathBuf::from(value)).is_some() {
                return Err("duplicate --desktop-profile".into());
            }
        } else if let Some(value) = argument.strip_prefix("--default-wm=") {
            if default_wm.replace(PathBuf::from(value)).is_some() {
                return Err("duplicate --default-wm".into());
            }
        } else if let Some(value) = argument.strip_prefix("--policy-checker=") {
            if policy_checker.replace(PathBuf::from(value)).is_some() {
                return Err("duplicate --policy-checker".into());
            }
        } else if argument == "--allow-deferred-policy" {
            if std::mem::replace(&mut allow_deferred, true) {
                return Err("duplicate --allow-deferred-policy".into());
            }
        } else {
            return Err(format!("unknown session profile preflight option {argument:?}").into());
        }
    }
    if policy_checker.is_some() == allow_deferred {
        return Err("choose exactly one of --policy-checker or --allow-deferred-policy".into());
    }
    let profile = profile.ok_or("--desktop-profile is required")?;
    if !profile.is_absolute() || !profile.is_file() {
        return Err("--desktop-profile requires an absolute existing file".into());
    }
    let prepared =
        sophia_config::load_prepared_desktop_profile(Some(&profile), ConfigGeneration::INITIAL)?;
    let components = &prepared.candidates.session.components;
    for component in &components.shell_components {
        let role = match component.role {
            sophia_config::ShellComponentRole::Descriptor => "descriptor",
            sophia_config::ShellComponentRole::Bar => "bar",
            sophia_config::ShellComponentRole::ApplicationLauncher => "application-launcher",
            sophia_config::ShellComponentRole::Dock => "dock",
        };
        // IDs and roles are bounded by profile parsing. This checks current
        // paths, not binary identity or authority to launch them later.
        let label = format!("selected shell component {} ({role})", component.id);
        require_executable(&component.executable, &label)?;
        if let Some(config) = &component.config {
            // Match ShellComponentLaunch::new: the client owns the contents,
            // and its private path is canonicalized again at actual launch.
            config
                .canonicalize()
                .map_err(|error| format!("{label} private config cannot be resolved: {error}"))?;
        }
    }
    let selected = match &components.window_manager {
        Some(wm) => Some(wm.executable.clone()),
        None => {
            let source = sophia_config::discover_default_config_source(ConfigDomain::Core, None);
            sophia_config::load_core_snapshot(&source, ConfigGeneration::INITIAL)?
                .external_wm
                .map(|wm| wm.executable)
        }
    };
    if let Some(selected) = selected.as_ref().or(default_wm.as_ref()) {
        require_executable(selected, "selected window manager")?;
    }
    if allow_deferred {
        println!("Selected WM will validate its policy during session activation.");
        println!("sophia_session_profile_preflight schema=1 status=accepted policy=deferred");
        return Ok(());
    }
    let policy_checker = policy_checker.expect("validated policy choice");
    if !policy_checker.is_absolute() {
        return Err("--policy-checker requires an absolute executable path".into());
    }
    require_executable(&policy_checker, "policy checker")?;
    if let (Some(selected), Some(default)) = (&selected, &default_wm)
        && !same_file(selected, default)
    {
        return Err("selected WM differs from --default-wm; select its checker explicitly without --default-wm, or use --allow-deferred-policy".into());
    }

    // Only the WM-owned fragment reaches the checker. The private directory protects
    // that fragment even when the caller's temporary root is shared.
    let directory = PrivateDirectory::new()?;
    let policy = directory.0.join("policy.kdl");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&policy)?;
    writeln!(file, "schema 1\npolicy {{")?;
    for value in &prepared.profile.candidates[&DesktopAuthority::Policy].values {
        writeln!(file, "    {}", value.encoded)?;
    }
    writeln!(file, "}}")?;
    drop(file);
    check_policy(&policy_checker, &policy, Duration::from_secs(10))?;
    println!("sophia_session_profile_preflight schema=1 status=accepted policy=validated");
    Ok(())
}

fn require_executable(path: &Path, role: &str) -> Result<()> {
    if !path.is_file() || rustix::fs::access(path, rustix::fs::Access::EXEC_OK).is_err() {
        return Err(format!("{role} is not executable: {}", path.display()).into());
    }
    Ok(())
}

fn same_file(left: &Path, right: &Path) -> bool {
    fs::metadata(left)
        .ok()
        .zip(fs::metadata(right).ok())
        .is_some_and(|(left, right)| left.dev() == right.dev() && left.ino() == right.ino())
}

fn check_policy(executable: &Path, policy: &Path, timeout: Duration) -> Result<()> {
    let mut child = Command::new(executable)
        .arg(policy)
        // Only the exit status is a verdict. A checker cannot fill output
        // pipes or retain the caller's standard input after it exits.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(format!("policy validation failed: {status}").into())
            };
        }
        if Instant::now() >= deadline {
            if let Some(group) = rustix::process::Pid::from_raw(child.id() as i32) {
                let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err("policy validation exceeded ten seconds".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct PrivateDirectory(PathBuf);

impl PrivateDirectory {
    fn new() -> Result<Self> {
        let mut random = [0_u8; 16];
        rustix::rand::getrandom(&mut random, rustix::rand::GetRandomFlags::empty())?;
        let suffix = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let path = std::env::temp_dir().join(format!("sophia-profile-preflight-{suffix}"));
        DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path))
    }
}

impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0.join("policy.kdl"));
        let _ = fs::remove_dir(&self.0);
    }
}
