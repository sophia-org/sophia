//! Compatibility codec for the retiring `sophia_shell_v1` Unix-socket frames.
//!
//! The shell client's `ipc-compat` feature uses it to keep the socket wire as
//! a rollback path until that transport is removed (t255). It carries the
//! smallest closure of Sophia's frame codec the socket wire needs: the frame
//! header, the handshake, and the content, application catalog, catalog-action, indicator and
//! persistent-catalog frames. Values and their byte shapes come from
//! [`sophia_shell_protocol`]; nothing here is part of the file contract.
//! `tests/corpus.rs` runs it over Sophia's golden shell corpus as pinned in
//! `spec/golden`; Sophia's gate compares it with Sophia's own codec over that
//! corpus and every truncation and single-byte change of each frame
//! (`crates/sophia-protocol/tests/sdk_ipc_parity.rs`).

mod byte_cursor;
mod cursor;
mod frame;
mod neutral_errors;
mod shell_applications;
mod shell_catalog_actions;
mod shell_catalog_transaction;
mod shell_content;
mod shell_hello;
mod shell_indicators;
mod types;

use sophia_shell_protocol::*;

pub use frame::{decode_frame, encode_frame};
pub use shell_applications::*;
pub use shell_catalog_actions::*;
pub use shell_catalog_transaction::*;
pub use shell_content::*;
pub use shell_hello::*;
pub use shell_indicators::*;
pub use types::*;
