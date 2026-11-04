use super::*;

impl ShellFiles {
    pub(super) fn frame_negotiation(
        &self,
        limits: Option<&ContentLimits>,
        capabilities: u64,
        revision: u16,
    ) -> Result<bool, Errno> {
        if revision != SHELL_FRAME_REVISION && capabilities & SHELL_FRAME_CAPABILITY == 0 {
            return Ok(false);
        }
        let required = SHELL_FRAME_CAPABILITY
            | crate::owned_ring_protocol::CAPABILITY
            | sophia_protocol::SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
        // Keep native-launcher/catalog, image/draw extensions and unknown bits out of this profile.
        let supported =
            ((1 << 11) - 1) | crate::owned_ring_protocol::CAPABILITY | SHELL_FRAME_CAPABILITY;
        if !cfg!(feature = "experimental-native-frame-transaction")
            || revision != SHELL_FRAME_REVISION
            || capabilities & required != required
            || capabilities & !supported != 0
            || self.catalog_allowed
            || self.descriptor
            || self.image.as_ref().is_none_or(|image| {
                image.config.discovery.source_kinds != crate::owned_ring_protocol::SOURCE_KIND
            })
            || limits.is_none_or(|limits| {
                limits.grant.connection_epoch != self.epoch || limits.validate().is_err()
            })
        {
            return Err(Errno::EACCES);
        }
        Ok(true)
    }

    pub(super) fn submission_revision(&self) -> u16 {
        if cfg!(feature = "experimental-native-frame-transaction")
            && self.negotiated
            && self.content
            && self.image_disclosed()
            && self.capabilities & SHELL_FRAME_CAPABILITY != 0
        {
            SHELL_FRAME_REVISION
        } else if self.image_disclosed() {
            9
        } else {
            8
        }
    }

    #[cfg(feature = "experimental-native-frame-transaction")]
    pub(super) fn decode_sft_input(&self, bytes: &[u8]) -> Result<Inbound, Errno> {
        if self.submission_revision() != SHELL_FRAME_REVISION {
            return Err(Errno::EACCES);
        }
        let max_bytes = self
            .content_limits
            .as_ref()
            .ok_or(Errno::EACCES)?
            .max_candidate_bytes;
        let record = decode_shell_frame_record(
            bytes,
            ShellFileClass::Candidate,
            SHELL_FRAME_REVISION,
            max_bytes as usize,
        )
        .map_err(|_| Errno::EINVAL)?;
        Ok(Inbound::Frame(Box::new(record)))
    }
}
