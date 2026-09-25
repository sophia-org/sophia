use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use sophia_config::{ConfigDomain, ConfigGeneration, DesktopAuthority};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(super) fn run(arguments: &[String]) -> Result<()> {
    let mut profile = None;
    let mut default_wm = None;
    for argument in arguments {
        if let Some(value) = argument.strip_prefix("--desktop-profile=") {
            if profile.replace(PathBuf::from(value)).is_some() {
                return Err("duplicate --desktop-profile".into());
            }
        } else if let Some(value) = argument.strip_prefix("--default-wm=") {
            if default_wm.replace(PathBuf::from(value)).is_some() {
                return Err("duplicate --default-wm".into());
            }
        } else {
            return Err(format!("unknown session profile preflight option {argument:?}").into());
        }
    }
    let profile = profile.ok_or("--desktop-profile is required")?;
    if !profile.is_absolute() || !profile.is_file() {
        return Err("--desktop-profile requires an absolute existing file".into());
    }
    let prepared =
        sophia_config::load_prepared_desktop_profile(Some(&profile), ConfigGeneration::INITIAL)?;
    let components = &prepared.candidates.session.components;
    if let Some(shell) = &components.shell_client {
        require_executable(shell, "selected native shell")?;
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
    if let Some(selected) = &selected
        && !default_wm
            .as_ref()
            .is_some_and(|default| same_file(selected, default))
    {
        require_executable(selected, "selected window manager")?;
        println!("Selected WM will validate its policy during session activation.");
        println!("sophia_session_profile_preflight schema=1 status=accepted policy=deferred");
        return Ok(());
    }
    let default_wm = default_wm.ok_or("--default-wm is required for Hagia policy validation")?;
    require_executable(&default_wm, "default Hagia policy client")?;

    // Only the WM-owned fragment reaches Hagia. The private directory protects
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
    check_policy(&default_wm, &policy, Duration::from_secs(10))?;
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
        .args(["config", "check"])
        .arg(format!("--config={}", policy.display()))
        .process_group(0)
        .spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(format!("Hagia policy validation failed: {status}").into())
            };
        }
        if Instant::now() >= deadline {
            if let Some(group) = rustix::process::Pid::from_raw(child.id() as i32) {
                let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err("Hagia policy validation exceeded ten seconds".into());
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
