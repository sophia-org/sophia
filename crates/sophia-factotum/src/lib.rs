//! `sophia-factotum`: a Rust port of 9front's factotum, the authentication
//! agent that holds a user's keys and runs authentication conversations so
//! that the programs asking never handle the secrets themselves.
//!
//! Portions are ported from 9front (`sys/src/cmd/auth/factotum`,
//! `sys/src/libauth`, parts of `sys/src/libc`), which is distributed under
//! the MIT licence:
//!
//! ```text
//! Copyright © 2021 Plan 9 Foundation
//! Copyright © 20XX 9front authors
//!
//! Permission is hereby granted, free of charge, to any person obtaining a
//! copy of this software and associated documentation files (the
//! "Software"), to deal in the Software without restriction, including
//! without limitation the rights to use, copy, modify, merge, publish,
//! distribute, sublicense, and/or sell copies of the Software, and to permit
//! persons to whom the Software is furnished to do so, subject to the
//! following conditions:
//!
//! The above copyright notice and this permission notice shall be included
//! in all copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
//! THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
//! FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
//! DEALINGS IN THE SOFTWARE.
//! ```
//!
//! Every byte inside its files (rpc replies, ctl lines, protocol messages,
//! AuthInfo) is 9front's. What differs is recorded in the factotum ADR
//! (`docs/notes/decisions/hhbejm8k-…`): Sophia serves 9P2000.L, so file-level
//! errors are errno values; the agent serves only its owner; protocols are
//! `pass` and the Linux `pam` login (with `p9any` and `dp9ik` to follow); and
//! `confirm` and `needkey` stay closed until a trusted prompt exists.

pub mod attr;
pub mod conversation;
pub mod ctl;
pub mod keyring;
pub mod logbuf;
pub mod pam_wire;
pub mod proto;
pub mod secret;
