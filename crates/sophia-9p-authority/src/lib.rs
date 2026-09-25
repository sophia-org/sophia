//! Sophia 9P Filesystem Frontend (`sophia-9p-authority`).
//!
//! This crate implements a synthetic filesystem display frontend serving
//! the Plan 9 9P2000 / 9P2000.L protocol over local Unix sockets or FUSE mounts.
//!
//! Client applications create windows, stream pixel updates, and receive user
//! input using standard filesystem primitives (`open`, `read`, `write`, `close`).
//!
//! For architectural details and normative contracts, see
//! `docs/sophia-9p-authority.md` and `docs/protocol-frontend-candidates.md`.

pub mod egress;
pub mod fs;
pub mod ingress;
pub mod protocol;
pub mod types;

use fs::SyntheticTree;
use types::{RMessage, TMessage, Tag};

/// The central 9P Protocol Authority service managing client connections
/// and synthetic directory trees.
#[derive(Clone, Debug, Default)]
pub struct NinePAuthority {
    pub tree: SyntheticTree,
}

impl NinePAuthority {
    pub fn new() -> Self {
        Self {
            tree: SyntheticTree::new(),
        }
    }

    /// Dispatches a single decoded `TMessage` and produces the corresponding `RMessage`.
    pub fn handle_message(&mut self, _tag: Tag, msg: TMessage) -> Result<RMessage, String> {
        match msg {
            TMessage::Version { msize, version } => {
                let accepted_version = if version.starts_with("9P2000") {
                    version
                } else {
                    "unknown".to_string()
                };
                Ok(RMessage::Version {
                    msize,
                    version: accepted_version,
                })
            }
            TMessage::Attach { fid, .. } => {
                let qid = self.tree.attach(fid)?;
                Ok(RMessage::Attach { qid })
            }
            TMessage::Walk {
                fid,
                newfid,
                wnames,
            } => {
                let wqids = self.tree.walk(fid, newfid, &wnames)?;
                Ok(RMessage::Walk { wqids })
            }
            TMessage::Open { fid, mode } => {
                let (qid, iounit) = self.tree.open(fid, mode)?;
                Ok(RMessage::Open { qid, iounit })
            }
            TMessage::Read { fid, offset, count } => {
                let data = self.tree.read(fid, offset, count)?;
                Ok(RMessage::Read { data })
            }
            TMessage::Write { fid, offset, data } => {
                let count = self.tree.write(fid, offset, &data)?;
                Ok(RMessage::Write { count })
            }
            TMessage::Clunk { fid } => {
                self.tree.clunk(fid)?;
                Ok(RMessage::Clunk)
            }
            TMessage::Flush { .. } => Ok(RMessage::Flush),
            TMessage::Auth { .. } => Err("Authentication not required".to_string()),
            TMessage::Create { .. } => Err("Permission denied".to_string()),
            TMessage::Remove { .. } => Err("Permission denied".to_string()),
            TMessage::Stat { .. } => Err("Stat not implemented".to_string()),
            TMessage::Wstat { .. } => Err("Wstat not implemented".to_string()),
        }
    }
}
