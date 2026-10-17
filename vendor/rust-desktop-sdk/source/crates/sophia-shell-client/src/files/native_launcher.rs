//! Revision-7 launcher admission. The role and exact mask are both required.
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
        if value.welcome.selected_revision != 7 {
            return Err(ShellClientError::UnsupportedRevision);
        }
        if options.required_capabilities != NATIVE_CAPS || value.welcome.capabilities != NATIVE_CAPS
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
