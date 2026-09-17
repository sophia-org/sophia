//! Single-shell compatibility owner. Independent components retain their own
//! ShellComponentTransport and borrow one Session-owned ContentEpochRegistry.
use std::path::Path;

use super::{ShellComponentTransport, ShellTransportError};
use crate::ContentEpochRegistry;

#[path = "legacy_methods.rs"]
mod legacy_methods;

pub struct ShellSessionTransport {
    pub(super) state: ShellComponentTransport,
    pub(super) content_epochs: ContentEpochRegistry,
}

impl ShellSessionTransport {
    pub fn bind_for_supervised_uid(
        directory: impl AsRef<Path>,
        expected_uid: u32,
    ) -> Result<Self, ShellTransportError> {
        Ok(Self {
            state: ShellComponentTransport::bind_for_supervised_uid(directory, expected_uid)?,
            content_epochs: ContentEpochRegistry::new(64 * 1024 * 1024)?,
        })
    }
}
