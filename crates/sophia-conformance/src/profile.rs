//! The live-session argument vector for each tool profile.
//!
//! One place, typed, and checked against the session that consumes it. The
//! shell runners used to assemble these inline, where a profile naming a
//! window manager that could not serve, or a desktop default the session could
//! not satisfy, was discovered only after DRM had been taken.

use std::process::Command;

/// A tool profile's session shape.
///
/// Only what differs between profiles: everything they share is added by
/// `session_args`, so a profile cannot forget it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Profile {
    pub name: &'static str,
    /// The startup application's identity within the session.
    pub startup: &'static str,
    /// Whether an explicitly selected WM protocol client serves this profile.
    pub window_manager: bool,
    /// Whether the session ends when its startup application does. A profile
    /// with no window manager has no logout shortcut, because shortcuts are
    /// resolved against a policy client's configuration, so this is its only
    /// ordinary exit.
    pub exit_with_startup: bool,
}

pub const PROFILES: [Profile; 3] = [
    Profile {
        name: "managed",
        startup: "terminal",
        window_manager: true,
        exit_with_startup: false,
    },
    Profile {
        name: "native",
        startup: "terminal",
        window_manager: false,
        exit_with_startup: true,
    },
    Profile {
        name: "standalone",
        startup: "standalone",
        window_manager: false,
        exit_with_startup: true,
    },
];

pub fn find(name: &str) -> Result<Profile, String> {
    PROFILES
        .iter()
        .copied()
        .find(|profile| profile.name == name)
        .ok_or_else(|| {
            let names = PROFILES
                .iter()
                .map(|profile| profile.name)
                .collect::<Vec<_>>()
                .join(" ");
            format!("unknown profile {name:?}; expected one of: {names}")
        })
}

/// Options a runner supplies that are not properties of the profile.
#[derive(Clone, Debug)]
pub struct Options {
    pub display: String,
    pub startup_executable: String,
    pub startup_arguments: Vec<String>,
    pub desktop_profile: Option<String>,
    pub core_config: Option<String>,
    pub wm_process: Option<String>,
    pub wm_arguments: Vec<String>,
}

impl Options {
    fn from_arguments(arguments: &[String]) -> Result<(Profile, Self), String> {
        let mut profile = None;
        let mut options = Self {
            display: ":77".to_owned(),
            startup_executable: "/usr/bin/true".to_owned(),
            startup_arguments: Vec::new(),
            desktop_profile: None,
            core_config: None,
            wm_process: None,
            wm_arguments: Vec::new(),
        };
        for argument in arguments {
            let (key, value) = argument
                .split_once('=')
                .ok_or_else(|| format!("expected key=value, got {argument:?}"))?;
            match key {
                "--profile" => profile = Some(find(value)?),
                "--display" => options.display = value.to_owned(),
                "--startup" => options.startup_executable = value.to_owned(),
                "--startup-arg" => options.startup_arguments.push(value.to_owned()),
                "--desktop-profile" => options.desktop_profile = Some(value.to_owned()),
                "--core-config" => options.core_config = Some(value.to_owned()),
                "--wm-process" => options.wm_process = Some(value.to_owned()),
                "--wm-arg" => options.wm_arguments.push(value.to_owned()),
                other => return Err(format!("unknown option {other:?}")),
            }
        }
        let profile = profile.ok_or("--profile is required")?;
        Ok((profile, options))
    }
}

/// The argument vector, without the `session run` command itself.
pub fn session_args(profile: Profile, options: &Options) -> Result<Vec<String>, String> {
    if profile.window_manager && options.wm_process.is_none() {
        return Err(format!(
            "profile {:?} is served by a policy client; pass --wm-process",
            profile.name
        ));
    }
    if options.wm_process.is_none() && !options.wm_arguments.is_empty() {
        // Silently dropping these is how the native chrome proof came to pass
        // a `--wm-config` to nothing at all. An argument for a process that
        // does not exist is a mistake, not a no-op.
        return Err(format!(
            "profile {:?} was given window-manager arguments but no --wm-process",
            profile.name
        ));
    }
    if !profile.window_manager && options.wm_process.is_some() {
        // Adding a WM would change the declared session shape, including its
        // chrome and shortcut authority.
        return Err(format!("profile {:?} runs no window manager", profile.name));
    }

    let mut arguments = vec![
        "--session-mode=normal".to_owned(),
        format!("--display={}", options.display),
        "--native-scanout".to_owned(),
        "--startup-ready-timeout-ms=8000".to_owned(),
    ];
    match (&options.core_config, &options.desktop_profile) {
        (None, None) => arguments.push("--no-config".to_owned()),
        (core, desktop) => {
            if let Some(core) = core {
                arguments.push(format!("--config={core}"));
            }
            if let Some(desktop) = desktop {
                arguments.push(format!("--desktop-profile={desktop}"));
            }
        }
    }
    arguments.push(format!(
        "--session-app={}={}",
        profile.startup, options.startup_executable
    ));
    arguments.push(format!("--session-start={}", profile.startup));
    if profile.exit_with_startup {
        arguments.push("--exit-when-startup-exits".to_owned());
    }
    for argument in &options.startup_arguments {
        arguments.push(format!("--session-app-arg={}={argument}", profile.startup));
    }
    if let Some(process) = &options.wm_process {
        arguments.push(format!("--wm-process={process}"));
        for argument in &options.wm_arguments {
            arguments.push(format!("--wm-process-arg={argument}"));
        }
    }
    Ok(arguments)
}

/// The configuration root every validation runs against.
///
/// Committed rather than generated, so the thing being validated against is
/// reviewable and identical everywhere.
fn decoy_config_home() -> Result<std::path::PathBuf, String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("the conformance manifest has no workspace root")?
        .join("tools/fixtures/decoy-config-home");
    if !root.join("sophia/desktop.kdl").is_file() {
        return Err(format!(
            "the decoy configuration fixture is missing: {}",
            root.display()
        ));
    }
    Ok(root)
}

/// An empty home, so configuration discovery has exactly one place to look.
fn isolated_home() -> Result<std::path::PathBuf, String> {
    let home = std::env::temp_dir().join(format!("sophia-profile-home-{}", std::process::id()));
    std::fs::create_dir_all(&home)
        .map_err(|error| format!("could not make an isolated home: {error}"))?;
    Ok(home)
}

/// Ask the session whether it would accept this vector.
///
/// Validation crosses the same CLI boundary used by an installed session, before
/// anything takes DRM.
fn validate(arguments: &[String]) -> Result<(), String> {
    let binary = session_binary()?;
    let output = Command::new(&binary)
        .arg("session")
        .arg("run")
        .arg("--validate-session-args")
        .args(arguments)
        // Part of the session's environment contract, set by every runner.
        // Validating without it would be validating a different session than
        // the one that runs, and `--native-scanout` is gated on it.
        .env("SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE", "1")
        // Configuration discovery is pointed at a profile built to be refused.
        //
        // A vector that does not pin its configuration discovers whatever the
        // operator has in ~/.config/sophia. On a developer's machine that is a
        // real profile naming a shell and shortcuts a proof session cannot
        // provide, and in normal mode the session refuses to start -- which
        // happened, on a TTY, with the session already down and greetd back.
        // Validating against the operator's own profile could not catch it:
        // the vector was fine on any machine without one.
        //
        // Pointing discovery at a decoy makes the leak a property of the
        // vector rather than of the machine. A profile that pins its
        // configuration never reads this file; one that does not, refuses
        // here, offline.
        .env("XDG_CONFIG_HOME", decoy_config_home()?)
        // HOME is isolated as well, so the decoy is the *only* configuration
        // this validation can discover. Without this the operator's own
        // ~/.config/sophia stays reachable and refuses a leaking vector too --
        // which looks identical from here, and would let the decoy stop
        // working without anything noticing.
        .env("HOME", isolated_home()?)
        .output()
        .map_err(|error| format!("could not run {binary}: {error}"))?;
    let accepted = String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.starts_with("sophia_live_session_args schema=1 status=accepted"));
    if output.status.success() && accepted {
        return Ok(());
    }
    if output.status.success() {
        // The binary exited cleanly without saying it validated anything,
        // which means it does not know the flag and ran a session instead.
        // A stale release binary did exactly that here: it started a real
        // session, waited for a policy endpoint, and timed out. A validation
        // that might not be a validation is worse than none.
        return Err(format!(
            "{binary} does not support --validate-session-args; rebuild it"
        ));
    }
    Err(format!(
        "the session refused this vector: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

fn session_binary() -> Result<String, String> {
    if let Ok(binary) = std::env::var("SOPHIA_BIN") {
        return Ok(binary);
    }
    let root = env!("CARGO_MANIFEST_DIR");
    let workspace = std::path::Path::new(root)
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("the xtask manifest has no workspace root")?;
    let target =
        workspace.join(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(|| "target".into()));
    // Newest wins. Preferring release found a binary from before the flag
    // existed, which ignored it and started a session; whichever was built
    // last is the one the caller most likely meant.
    let mut newest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for profile in ["release", "debug"] {
        let candidate = target.join(profile).join("sophia");
        let Ok(modified) = candidate.metadata().and_then(|data| data.modified()) else {
            continue;
        };
        if newest
            .as_ref()
            .is_none_or(|(previous, _)| modified > *previous)
        {
            newest = Some((modified, candidate));
        }
    }
    newest
        .map(|(_, path)| path.to_string_lossy().into_owned())
        .ok_or_else(|| "no sophia binary found; build it or set SOPHIA_BIN".to_owned())
}

/// The validated vector for the profile named in `arguments`.
pub fn resolve(arguments: &[String]) -> Result<Vec<String>, String> {
    let (profile, options) = Options::from_arguments(arguments)?;
    let vector = session_args(profile, &options)?;
    validate(&vector)?;
    Ok(vector)
}

/// Every profile's vector, validated, with the length each produced.
/// Prove the decoy profile still refuses a vector that reaches it.
///
/// Every profile below is validated against a configuration root built to be
/// refused, which only means anything while it is still refusing. If the
/// schema moves under the fixture, or the shell rule that rejects it softens,
/// the file becomes an inert directory and the profile validations quietly stop
/// testing discovery at all -- passing, and proving nothing.
///
/// So the check runs the one vector that must fail: normal mode with no
/// configuration pinned. That is the exact shape that took a gate down at a
/// greetd prompt.
fn require_discovery_leak_is_visible() -> Result<(), String> {
    let leaking = [
        "--session-mode=normal".to_owned(),
        "--display=:77".to_owned(),
        "--session-app=probe=/usr/bin/true".to_owned(),
        "--session-start=probe".to_owned(),
        "--exit-when-startup-exits".to_owned(),
    ];
    if validate(&leaking).is_ok() {
        return Err(
            "a normal-mode vector with no configuration pinned was accepted, so the decoy \nprofile is no longer refused and every profile check below proves nothing about \nconfiguration discovery. Check tools/fixtures/decoy-config-home/sophia/desktop.kdl \nagainst the current desktop-profile schema."
                .to_owned(),
        );
    }
    Ok(())
}

pub fn check_every_profile(arguments: &[String]) -> Result<Vec<(&'static str, usize)>, String> {
    if !arguments.is_empty() {
        return Err("check-profiles takes no arguments".to_owned());
    }
    require_discovery_leak_is_visible()?;
    let mut accepted = Vec::new();
    for profile in PROFILES {
        let mut options = Options {
            display: ":77".to_owned(),
            startup_executable: "/usr/bin/true".to_owned(),
            startup_arguments: Vec::new(),
            desktop_profile: None,
            core_config: None,
            wm_process: None,
            wm_arguments: Vec::new(),
        };
        if profile.window_manager {
            options.wm_process = Some("/usr/bin/true".to_owned());
        }
        let vector = session_args(profile, &options)?;
        validate(&vector).map_err(|error| format!("profile {:?}: {error}", profile.name))?;
        accepted.push((profile.name, vector.len()));
    }
    Ok(accepted)
}
