//! Explicit SFT admission; no revision range or implicit image capability.
use super::FileWire;
use crate::{ShellClientError, ShellClientOptions, wire::Inbound};
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::{SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE, ShellContentRecord};

pub(super) fn validate_negotiated(
    options: &ShellClientOptions,
    value: &ShellFileNegotiated,
) -> Result<(), ShellClientError> {
    let caps = value.welcome.capabilities;
    if (caps | options.required_capabilities)
        & (SHELL_FRAME_CAPABILITY
            | SHELL_FRAME_TERMINAL_ONLY_CAPABILITY
            | SHELL_FRAME_LEAN_CAPABILITY)
        == 0
        && value.welcome.selected_revision != SHELL_FRAME_REVISION
    {
        return Ok(());
    }
    let required = SHELL_FRAME_CAPABILITY
        | crate::IMAGE_CAPABILITY_OWNED_RING
        | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
    let unsupported =
        crate::IMAGE_CAPABILITY_SEALED | crate::IMAGE_CAPABILITY_DMA_BUF | (1 << 16) | (1 << 17);
    if value.welcome.selected_revision != SHELL_FRAME_REVISION
        || options.required_capabilities & required != required
        || caps & required != required
        || caps & unsupported != 0
        || (caps ^ options.required_capabilities)
            & (SHELL_FRAME_TERMINAL_ONLY_CAPABILITY | SHELL_FRAME_LEAN_CAPABILITY)
            != 0
        || (caps & SHELL_FRAME_LEAN_CAPABILITY != 0
            && caps & SHELL_FRAME_TERMINAL_ONLY_CAPABILITY == 0)
        || !value.limits_published
    {
        return Err(ShellClientError::MissingCapability);
    }
    Ok(())
}

pub(super) fn frame_max_bytes(inbox: &std::collections::VecDeque<Inbound>) -> usize {
    inbox
        .iter()
        .find_map(|item| match item {
            Inbound::Content(_, ShellContentRecord::Limits(limits)) => {
                Some(limits.max_candidate_bytes as usize)
            }
            _ => None,
        })
        .unwrap_or(0)
        .min(SHELL_FILE_MAX_CANDIDATE_BYTES)
}

impl FileWire {
    /// Negotiated bit 20: complete `frame-submit` commits fold the ACK and
    /// settle on their Rwrite alone.
    pub(super) fn lean(&self) -> bool {
        self.capabilities & SHELL_FRAME_LEAN_CAPABILITY != 0
    }

    pub(crate) fn require_frame(&self) -> Result<(), ShellClientError> {
        if self.revision != SHELL_FRAME_REVISION || self.capabilities & SHELL_FRAME_CAPABILITY == 0
        {
            return Err(ShellClientError::MissingCapability);
        }
        Ok(())
    }

    pub(super) fn decode_frame_event(
        &self,
        header: ShellFileHeader,
        record: &[u8],
    ) -> Result<(u64, super::events::Event), ShellClientError> {
        self.require_frame()?;
        if header.connection_epoch != self.epoch {
            return Err(ShellClientError::Protocol(
                "frame event from another connection epoch",
            ));
        }
        let value = decode_shell_frame_record(
            record,
            ShellFileClass::Event,
            self.revision,
            SHELL_FILE_MAX_CANDIDATE_BYTES,
        )?;
        if self.capabilities & SHELL_FRAME_TERMINAL_ONLY_CAPABILITY != 0
            && matches!(&value, ShellFrameRecord::Outcome { outcome, .. } if outcome.kind == 1)
        {
            return Err(ShellClientError::Protocol("Prepared in terminal-only mode"));
        }
        Ok((
            header.sequence,
            super::events::Event::Inbound(Inbound::Frame(value)),
        ))
    }

    pub(crate) fn encode_frame(
        &mut self,
        value: &ShellFrameRecord,
    ) -> Result<Vec<Vec<u8>>, ShellClientError> {
        self.require_frame()?;
        if let Some(error) = &self.fatal {
            return Err(error.clone());
        }
        if !matches!(
            value,
            ShellFrameRecord::Open { .. }
                | ShellFrameRecord::Close { .. }
                | ShellFrameRecord::Commit(_)
        ) {
            return Err(ShellClientError::WrongDirection);
        }
        if self.next_submission_id == u64::MAX {
            return Err(ShellClientError::InvalidArgument(
                "submission identity exhausted",
            ));
        }
        self.cancel_encoded();
        let header = self.next_header(value.kind());
        if shell_frame_record_bytes(header, value, self.revision)? > self.frame_max_bytes {
            return Err(ShellClientError::InvalidArgument(
                "frame exceeds granted candidate bytes",
            ));
        }
        let bytes = encode_shell_frame_record(header, value, self.revision)?;
        Ok(self.stage_records(vec![bytes]))
    }
}
