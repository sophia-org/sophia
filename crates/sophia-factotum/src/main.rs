//! `sophia-factotum`: the authentication agent Session starts.
//!
//! Session passes its end of a socket pair as standard input. The agent
//! hardens itself before anything secret can arrive, checks that the peer
//! is its parent running as the same user, and serves factotum's files on
//! that connection until Session hangs up.
//!
//! ```text
//! sophia-factotum --owner USER --pam-helper /ABSOLUTE/PATH
//!                 [--pam-service NAME] [--pam-deadline-ms N]
//!                 [--test-mode --pam-confdir DIR]
//! ```
//!
//! `--test-mode` accepts a helper the user owns and lets PAM read a private
//! configuration directory. Session never passes it.

use sophia_factotum::agent::{AgentConfig, check_helper, harden, serve, session_is_parent};
use sophia_factotum::pam_helper::PamHelper;
use sophia_factotum::proto::Settings;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

/// The memory the agent may lock for all future pages.
const MEMLOCK_BUDGET: u64 = 32 << 20;
const WORKERS: usize = 4;

struct Arguments {
    owner: String,
    helper: PathBuf,
    service: String,
    deadline: Duration,
    test_mode: bool,
    confdir: Option<PathBuf>,
}

fn arguments() -> Result<Arguments, String> {
    let mut owner = None;
    let mut helper = None;
    let mut service = "sophia-lock".to_owned();
    let mut deadline = Duration::from_secs(30);
    let mut test_mode = false;
    let mut confdir = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(flag) = arguments.next() {
        let mut value = || arguments.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--owner" => owner = Some(value()?),
            "--pam-helper" => helper = Some(PathBuf::from(value()?)),
            "--pam-service" => service = value()?,
            "--pam-deadline-ms" => {
                let millis = value()?
                    .parse::<u64>()
                    .map_err(|_| "--pam-deadline-ms needs a number".to_owned())?;
                deadline = Duration::from_millis(millis.clamp(100, 120_000));
            }
            "--pam-confdir" => confdir = Some(PathBuf::from(value()?)),
            "--test-mode" => test_mode = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if confdir.is_some() && !test_mode {
        return Err("--pam-confdir needs --test-mode".into());
    }
    Ok(Arguments {
        owner: owner.ok_or("--owner is required")?,
        helper: helper.ok_or("--pam-helper is required")?,
        service,
        deadline,
        test_mode,
        confdir,
    })
}

fn main() -> ExitCode {
    // Before anything else, and before any secret can arrive.
    let hardening = harden(MEMLOCK_BUDGET);
    let arguments = match arguments() {
        Ok(arguments) => arguments,
        Err(error) => {
            eprintln!("sophia-factotum: {error}");
            return ExitCode::from(2);
        }
    };
    if !hardening.undumpable || !hardening.no_core {
        eprintln!("sophia-factotum: refusing to run without dump protection");
        return ExitCode::from(1);
    }
    if !hardening.locked_future {
        eprintln!("sophia-factotum: memory locked for current pages only");
    }
    if arguments.test_mode {
        eprintln!("sophia-factotum: TEST MODE: helper ownership and PAM confdir are not checked");
    } else if let Err(error) = check_helper(&arguments.helper) {
        eprintln!("sophia-factotum: {error}");
        return ExitCode::from(1);
    }
    let session = match std::io::stdin()
        .as_fd()
        .try_clone_to_owned()
        .map(UnixStream::from)
    {
        Ok(session) => session,
        Err(_) => {
            eprintln!("sophia-factotum: standard input is not Session's socket");
            return ExitCode::from(1);
        }
    };
    if !session_is_parent(&session) {
        eprintln!("sophia-factotum: the socket's peer is not the parent process");
        return ExitCode::from(1);
    }
    let config = AgentConfig {
        settings: Settings {
            owner: arguments.owner,
            pam_service: arguments.service,
        },
        helper: PamHelper {
            path: arguments.helper,
            confdir: arguments.confdir,
            deadline: arguments.deadline,
        },
        workers: WORKERS,
    };
    match serve(config, session) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("sophia-factotum: {error}");
            ExitCode::from(1)
        }
    }
}
