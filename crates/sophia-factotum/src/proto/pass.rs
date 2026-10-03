//! `pass`: a repository for a password, with no server side.
//!
//! Ported from 9front `sys/src/cmd/auth/factotum/pass.c` (MIT, Copyright (c)
//! 2021 Plan 9 Foundation and 9front authors).

use super::{Env, Phase, ProtocolId, ProtocolState, Status};
use crate::attr::quote;
use crate::conversation::Conversation;

/// The one phase. 9front never installs a name for it, so a phase error
/// prints `0`.
const HAVE_PASS: u8 = 0;

/// `passinit`: finds a key with `user` and `!password` matching the start
/// attributes and merges the key's public attributes into the
/// conversation's.
pub(super) fn init(cx: &mut Conversation, env: &mut Env<'_>) -> Status {
    let key = match env.find_key(cx, ProtocolId::Pass.key_prompt()) {
        Ok(key) => key,
        Err(status) => return status,
    };
    cx.attrs.set_attrs(&key.attrs);
    cx.state = Some(ProtocolState::Pass { key });
    cx.phase = Phase::Proto(HAVE_PASS);
    Status::Ok
}

/// `passread`: yields `%q %q` of user and password, every time it is asked.
pub(super) fn read(cx: &mut Conversation, max: usize) -> (Status, Vec<u8>) {
    let Some(ProtocolState::Pass { key }) = cx.state.as_ref() else {
        return (cx.phase_error("read"), Vec::new());
    };
    if cx.phase != Phase::Proto(HAVE_PASS) {
        return (cx.phase_error("read"), Vec::new());
    }
    let (Some(user), Some(password)) = (key.attrs.value("user"), key.private.value("!password"))
    else {
        return (Status::Failure("passread cannot happen".into()), Vec::new());
    };
    let reply = format!("{} {}", quote(user), quote(password)).into_bytes();
    if reply.len() > max {
        let wanted = u32::try_from(reply.len()).unwrap_or(u32::MAX);
        let mut reply = reply;
        zeroize::Zeroize::zeroize(&mut reply);
        return (Status::Toosmall(wanted), Vec::new());
    }
    (Status::Ok, reply)
}
