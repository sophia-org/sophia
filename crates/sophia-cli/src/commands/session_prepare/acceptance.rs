//! Validate with the same installed executable, environment and argument vector
//! that will own the session, without starting a bus or acquiring devices.
use super::{BTreeMap, Result, bounded, proofs};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    process::{Command, Stdio},
};

fn private_log(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(path)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

pub(super) fn run(options: &BTreeMap<String, String>, extra: &[String]) -> Result<()> {
    // Do not permit this parser-only command to become an arbitrary command runner.
    if extra.get(..2) != Some(&["session".to_owned(), "run".to_owned()]) {
        return Err("check-launch requires the prepared session run vector".into());
    }
    let state = proofs::private_state(options)?;
    let output = state.join("session-args-check.log");
    let errors = state.join("session-args-check.err");
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(extra)
        .arg("--validate-session-args")
        .stdin(Stdio::null())
        .stdout(private_log(&output)?)
        .stderr(private_log(&errors)?);
    bounded::check(&mut command, "assembled session arguments")?;
    let accepted = BufReader::new(File::open(output)?)
        .lines()
        .collect::<std::io::Result<Vec<_>>>()?
        .iter()
        .any(|line| line.starts_with("sophia_live_session_args schema=1 status=accepted "));
    if !accepted {
        return Err("session parser did not publish an acceptance record".into());
    }
    println!("sophia_session_launch schema=1 status=accepted");
    Ok(())
}
