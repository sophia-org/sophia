//! Native focus, input and terminal custody through the production 9P export.
//! Protection and renderer completion are supplied; no physical input or display.
use sophia_protocol::*;
use sophia_runtime::*;
#[allow(dead_code)]
#[path = "support/native_files_peer.rs"]
mod files;
#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;
use files::*;
#[path = "support/native_launcher_focus.rs"]
mod focus;
