//! `sophia-factotum-pam`: verifies one password through PAM for
//! `sophia-factotum`, then exits.
//!
//! The agent executes this helper for each `pam` attempt, so no PAM module
//! ever shares the agent's address space. It reads one request frame from
//! stdin straight into a locked page, runs one PAM transaction, writes one
//! reply on stdout. Before PAM runs, the reply pipe moves to a private
//! descriptor and stdout becomes `/dev/null`, so a module that prints can
//! neither forge nor stall the reply. It never prints PAM's messages, the
//! user's password or its length. `--confdir DIR` points PAM at a private
//! configuration directory and is set only by tests.

#![deny(unsafe_code)]

#[allow(unsafe_code)]
mod ffi;

use sophia_factotum::pam_wire::{
    FLAG_DISALLOW_NULL, FLAG_SETCRED, HelperReply, MAX_REQUEST, RequestView,
};
use sophia_factotum::proto::PamVerdict;
use std::ffi::CString;
use std::io::Write;
use std::os::fd::{AsFd, OwnedFd};
use std::process::ExitCode;

fn main() -> ExitCode {
    ffi::harden();
    let confdir = match confdir_argument() {
        Ok(confdir) => confdir,
        Err(()) => return ExitCode::from(2),
    };
    let Ok(reply_pipe) = private_reply_pipe() else {
        return ExitCode::from(1);
    };
    let reply = match verify(confdir.as_deref()) {
        Some(reply) => reply,
        // A malformed request or an unusable page: no verdict is given and
        // the agent treats the exit as a helper failure.
        None => return ExitCode::from(1),
    };
    if std::fs::File::from(reply_pipe)
        .write_all(&reply.encode())
        .is_err()
    {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

/// Moves the reply pipe off descriptor 1 to a close-on-exec descriptor of
/// its own, and points descriptor 1 at `/dev/null`.
fn private_reply_pipe() -> rustix::io::Result<OwnedFd> {
    let stdout = std::io::stdout();
    let reply = rustix::io::fcntl_dupfd_cloexec(stdout.as_fd(), 3)?;
    let null = rustix::fs::open(
        "/dev/null",
        rustix::fs::OFlags::WRONLY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?;
    rustix::stdio::dup2_stdout(&null)?;
    Ok(reply)
}

/// Fills `buffer` from stdin without any buffering outside it. `None` at an
/// early end of input or a read error.
fn read_stdin(buffer: &mut [u8]) -> Option<()> {
    let stdin = std::io::stdin();
    let mut filled = 0;
    while filled < buffer.len() {
        match rustix::io::read(stdin.as_fd(), &mut buffer[filled..]) {
            Ok(0) => return None,
            Ok(count) => filled += count,
            Err(rustix::io::Errno::INTR) => {}
            Err(_) => return None,
        }
    }
    Some(())
}

fn confdir_argument() -> Result<Option<CString>, ()> {
    let mut arguments = std::env::args_os().skip(1);
    match (arguments.next(), arguments.next(), arguments.next()) {
        (None, _, _) => Ok(None),
        (Some(flag), Some(directory), None) if flag == "--confdir" => {
            use std::os::unix::ffi::OsStringExt as _;
            CString::new(directory.into_vec()).map(Some).map_err(|_| ())
        }
        _ => Err(()),
    }
}

fn verify(confdir: Option<&std::ffi::CStr>) -> Option<HelperReply> {
    let mut page = ffi::LockedPage::new(MAX_REQUEST.next_multiple_of(4096))?;
    let buffer = page.as_mut_slice();
    // Every byte of the request, the secret included, is read only into the
    // locked page: no reader buffers it anywhere else.
    read_stdin(&mut buffer[..4])?;
    let prefix = buffer[..4].try_into().ok()?;
    let length = RequestView::declared_length(prefix).ok()?;
    read_stdin(&mut buffer[4..length])?;
    let request = RequestView::decode(&buffer[..length]).ok()?;
    let (verdict, pam_code) = ffi::authenticate(
        request.service,
        request.user,
        request.secret,
        request.flags & FLAG_DISALLOW_NULL != 0,
        request.flags & FLAG_SETCRED != 0,
        confdir,
    );
    // The page, and the secret in it, is zeroed and unmapped when it drops.
    Some(HelperReply {
        verdict: match verdict {
            PamVerdict::TimedOut | PamVerdict::HelperFailed => PamVerdict::Unavailable,
            verdict => verdict,
        },
        pam_code,
    })
}
