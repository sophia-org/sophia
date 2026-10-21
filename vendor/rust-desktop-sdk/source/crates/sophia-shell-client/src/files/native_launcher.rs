//! Launcher admission at r7 or r9 ISB. The role and exact mask are required.
use super::FileWire;
use crate::{ShellClientError, ShellClientOptions};
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;

const NATIVE_CAPS: u64 = SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
    | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
    | SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER;

pub(super) fn validate_negotiated(
    native: bool,
    options: &ShellClientOptions,
    value: &ShellFileNegotiated,
) -> Result<(), ShellClientError> {
    if native {
        let ring = options.required_capabilities & crate::IMAGE_CAPABILITY_OWNED_RING != 0;
        let image = options.required_capabilities & crate::IMAGE_CAPABILITY_SEALED != 0;
        let (revision, capabilities) = if ring {
            (
                sophia_shell_protocol::SOPHIA_SHELL_OWNED_RING_REVISION,
                NATIVE_CAPS | crate::IMAGE_CAPABILITY_OWNED_RING,
            )
        } else if image {
            (9, NATIVE_CAPS | crate::IMAGE_CAPABILITY_SEALED)
        } else {
            (7, NATIVE_CAPS)
        };
        if value.welcome.selected_revision != revision {
            return Err(ShellClientError::UnsupportedRevision);
        }
        if options.required_capabilities != capabilities
            || value.welcome.capabilities != capabilities
        {
            return Err(ShellClientError::MissingCapability);
        }
        if !value.limits_published {
            return Err(ShellClientError::Protocol(
                "native launcher requires Limits",
            ));
        }
    } else if value.welcome.capabilities & SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER != 0 {
        return Err(ShellClientError::MissingCapability);
    }
    Ok(())
}

impl FileWire {
    pub(crate) fn require_native_launcher(&self) -> Result<(), ShellClientError> {
        if !self.native_launcher {
            return Err(ShellClientError::MissingCapability);
        }
        Ok(())
    }

    pub(super) fn encode_native_record(
        &mut self,
        value: &ShellFileNativeLauncherRecord,
    ) -> Result<Vec<u8>, ShellClientError> {
        self.require_native_launcher()?;
        let kind = shell_file_native_launcher_kind(&value.record)
            .ok_or(ShellClientError::WrongDirection)?;
        if shell_file_class(kind) != ShellFileClass::Candidate {
            return Err(ShellClientError::WrongDirection);
        }
        let header = self.next_header(kind);
        Ok(encode_shell_file_native_launcher_transaction(
            header, value,
        )?)
    }
}
