//! `sophia-factotum-pam`: verifies one password through PAM for
//! `sophia-factotum`, then exits.
//!
//! The agent executes this helper for each `pam` attempt, so no PAM module
//! ever shares the agent's address space. It reads one request frame on
//! stdin into a locked page, runs one PAM transaction, writes one reply on
//! stdout. It never prints PAM's messages, the user's password or its
//! length. `--confdir DIR` points PAM at a private configuration directory
//! and is set only by tests.

#![deny(unsafe_code)]

#[allow(unsafe_code)]
mod ffi;

use sophia_factotum::pam_wire::{
    FLAG_DISALLOW_NULL, FLAG_SETCRED, HelperReply, MAX_REQUEST, RequestView,
};
use sophia_factotum::proto::PamVerdict;
use std::ffi::CString;
use std::io::{Read, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    ffi::harden();
    let confdir = match confdir_argument() {
        Ok(confdir) => confdir,
        Err(()) => return ExitCode::from(2),
    };
    let reply = match verify(confdir.as_deref()) {
        Some(reply) => reply,
        // A malformed request or an unusable page: no verdict is given and
        // the agent treats the exit as a helper failure.
        None => return ExitCode::from(1),
    };
    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(&reply.encode())
        .and_then(|()| stdout.flush())
        .is_err()
    {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
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
    let mut stdin = std::io::stdin().lock();
    let mut prefix = [0; 4];
    stdin.read_exact(&mut prefix).ok()?;
    let length = RequestView::declared_length(prefix).ok()?;
    buffer[..4].copy_from_slice(&prefix);
    stdin.read_exact(&mut buffer[4..length]).ok()?;
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
