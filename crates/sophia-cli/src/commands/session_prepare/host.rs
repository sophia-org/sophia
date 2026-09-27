//! Bounded host-policy check. The external checker owns desktop detection;
//! Sophia owns invocation, output bounds and refusal before device takeover.
//! The checker is operator-trusted: a child that changes its process group or
//! session can escape cleanup. An executable symlink is accepted; checking the
//! path does not seal it against replacement before exec.
use super::{BTreeMap, Result, required};
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, WaitIdStatus};
use std::{
    io::{ErrorKind, Read},
    os::{fd::AsFd, unix::process::CommandExt},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const DEADLINE: Duration = Duration::from_secs(10);
const CLEANUP_GRACE: Duration = Duration::from_secs(2);
const STDOUT_CAP: usize = 4096;
const STDERR_CAP: usize = 16 * 1024;

struct Checker(Child);

impl Checker {
    fn group(&self) -> Pid {
        Pid::from_raw(self.0.id() as i32).expect("spawned child has a process ID")
    }

    fn status(&self) -> Result<Option<WaitIdStatus>> {
        // Keep the exited leader waitable: its PID pins the group number until
        // Drop finishes signalling. Child::try_wait would reap it too early.
        Ok(rustix::process::waitid(
            WaitId::Pid(self.group()),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        )?)
    }
}

impl Drop for Checker {
    fn drop(&mut self) {
        // Always allow the group its grace period, even after successful exit.
        // A probe for group existence would include our unreaped leader. Do not
        // reap until after the last signal: that would allow PGID reuse.
        let group = self.group();
        let _ = rustix::process::kill_process_group(group, Signal::TERM);
        std::thread::sleep(CLEANUP_GRACE);
        let _ = rustix::process::kill_process_group(group, Signal::KILL);
        let _ = self.0.wait();
    }
}

fn diagnostic(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    for ch in String::from_utf8_lossy(bytes).chars() {
        if ch.is_control() && !matches!(ch, '\n' | '\t') {
            text.extend(ch.escape_default());
        } else {
            text.push(ch);
        }
    }
    text
}

fn nonblocking(pipe: &impl AsFd) -> Result<()> {
    let flags = rustix::fs::fcntl_getfl(pipe)?;
    rustix::fs::fcntl_setfl(pipe, flags | rustix::fs::OFlags::NONBLOCK)?;
    Ok(())
}

/// One bounded read per stream per visit, so a noisy stream cannot starve its
/// peer or the deadline. Read one byte beyond the cap to detect overflow.
fn read(pipe: &mut impl Read, bytes: &mut Vec<u8>, cap: usize, name: &str) -> Result<bool> {
    let mut buffer = [0; 4096];
    let remaining = (cap + 1 - bytes.len()).min(buffer.len());
    match pipe.read(&mut buffer[..remaining]) {
        Ok(0) => Ok(true),
        Ok(count) => {
            bytes.extend_from_slice(&buffer[..count]);
            if bytes.len() > cap {
                return Err(format!("host preflight {name} exceeds {cap} bytes").into());
            }
            Ok(false)
        }
        Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {
            Ok(false)
        }
        Err(error) => Err(error.into()),
    }
}

fn tty_name(tty: &str) -> bool {
    if matches!(tty, "/dev/tty" | "/dev/console") {
        return true;
    }
    let suffix = tty
        .strip_prefix("/dev/tty")
        .or_else(|| tty.strip_prefix("/dev/pts/"));
    suffix.is_some_and(|text| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
}

pub(super) fn run(options: &BTreeMap<String, String>, extra: &[String]) -> Result<()> {
    if !extra.is_empty()
        || options
            .keys()
            .any(|name| !matches!(name.as_str(), "tty" | "allow-active"))
    {
        return Err("check-host accepts only --tty and optional --allow-active=true|false".into());
    }
    let tty = required(options, "tty")?;
    if !tty_name(tty) {
        return Err(
            "check-host requires an absolute /dev/ttyN, /dev/pts/N, /dev/tty or /dev/console name"
                .into(),
        );
    }
    let allow_active = match options.get("allow-active").map(String::as_str) {
        None | Some("false") => false,
        Some("true") => true,
        _ => return Err("--allow-active must be true or false".into()),
    };
    let checker = std::env::var_os("SOPHIA_SESSION_PREFLIGHT")
        .ok_or("SOPHIA_SESSION_PREFLIGHT must name an explicit host checker")?;
    let path = Path::new(&checker);
    if !path.is_absolute()
        || !path.is_file()
        || rustix::fs::access(path, rustix::fs::Access::EXEC_OK).is_err()
    {
        return Err("SOPHIA_SESSION_PREFLIGHT must be an absolute regular executable file".into());
    }
    let mut child = Checker(
        Command::new(path)
            .arg(format!("--tty={tty}"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()?,
    );
    let mut stdout = child.0.stdout.take().ok_or("missing host checker stdout")?;
    let mut stderr = child.0.stderr.take().ok_or("missing host checker stderr")?;
    nonblocking(&stdout)?;
    nonblocking(&stderr)?;
    let (mut out, mut err) = (
        Vec::with_capacity(STDOUT_CAP + 1),
        Vec::with_capacity(STDERR_CAP + 1),
    );
    let (mut out_done, mut err_done) = (false, false);
    let deadline = Instant::now() + DEADLINE;
    let status = loop {
        if !out_done {
            out_done = read(&mut stdout, &mut out, STDOUT_CAP, "stdout")?;
        }
        if !err_done {
            err_done = read(&mut stderr, &mut err, STDERR_CAP, "stderr")?;
        }
        if let Some(status) = child.status()?
            && out_done
            && err_done
        {
            break status;
        }
        if Instant::now() >= deadline {
            return Err("host preflight exceeded ten seconds".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    // Signal remaining group members before publishing the verdict.
    drop(child);
    let diagnostic = diagnostic(&err);
    let expected = format!("sophia_session_preflight schema=1 status=clear tty={tty}\n");
    if status.exit_status() == Some(0) && out == expected.as_bytes() {
        if !diagnostic.is_empty() {
            eprintln!("host preflight checker: {diagnostic}");
        }
        print!("{expected}");
        return Ok(());
    }
    if status.exit_status() == Some(1) && out.is_empty() && allow_active {
        eprintln!(
            "host preflight active-session refusal explicitly overridden: {}",
            diagnostic
        );
        println!("sophia_session_preflight schema=1 status=overridden tty={tty}");
        return Ok(());
    }
    Err(
        format!("host preflight refused ({status:?}) or returned an invalid verdict: {diagnostic}")
            .into(),
    )
}
