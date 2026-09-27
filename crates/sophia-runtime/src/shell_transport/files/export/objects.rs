//! The negotiated capability gates and the three snapshot-object slots
//! (Outputs, Catalog, Indicators): which root names this attach discloses,
//! which kind maps to which slot, and the shared fits-then-qid-then-announce
//! publication sequence every one of them uses.
use super::*;

impl ShellFiles {
    pub(super) fn supports_native_launcher(&self) -> bool {
        self.capabilities & sophia_protocol::SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER != 0
    }

    pub(super) fn supports_persistent_catalog(&self) -> bool {
        self.capabilities & sophia_protocol::SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG != 0
    }

    pub(super) fn supports_indicators(&self) -> bool {
        self.capabilities & sophia_protocol::SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS != 0
    }

    pub(super) fn supports_indicator_activation(&self) -> bool {
        self.capabilities & sophia_protocol::SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION != 0
    }

    /// The root vocabulary this attach discloses: the fixed names, plus
    /// `catalog` for the launcher/dock profile and `indicators` once bit 9
    /// is negotiated (bar's revision/bit selection is offer-dependent, so it
    /// is unknown, and the name absent, before negotiation completes).
    pub(super) fn root_entries(&self) -> Vec<(&'static [u8], Node)> {
        let mut entries: Vec<(&'static [u8], Node)> = vec![
            (b"api", Node::Api),
            (b"limits", Node::Limits),
            (b"outputs", Node::Outputs),
            (b"events", Node::Events),
            (b"transaction", Node::Transaction),
            (b"submit", Node::Submit),
            (b"ack", Node::Ack),
            (b"upload", Node::Uploads),
        ];
        if self.catalog_allowed {
            entries.push((b"catalog", Node::Catalog));
        }
        if self.supports_indicators() {
            entries.push((b"indicators", Node::Indicators));
        }
        entries
    }

    pub(super) fn object_slot(&self, kind: ShellFileKind) -> Option<&ObjectSlot> {
        match kind {
            ShellFileKind::Outputs => Some(&self.outputs),
            ShellFileKind::Catalog => Some(&self.catalog),
            ShellFileKind::Indicators => Some(&self.indicators),
            _ => None,
        }
    }

    pub(super) fn object_slot_mut(&mut self, kind: ShellFileKind) -> Option<&mut ObjectSlot> {
        match kind {
            ShellFileKind::Outputs => Some(&mut self.outputs),
            ShellFileKind::Catalog => Some(&mut self.catalog),
            ShellFileKind::Indicators => Some(&mut self.indicators),
            _ => None,
        }
    }

    /// Makes one snapshot object current and journals its publication, as one
    /// step: the event's room is checked before a qid is spent, and nothing
    /// changes on refusal. `Ok(false)` means the journal has no room yet.
    pub(in crate::shell_transport) fn publish_object(
        &mut self,
        kind: ShellFileKind,
        body: &[u8],
        credited: bool,
    ) -> Result<bool, Errno> {
        if self.revoked {
            return Err(Errno::ESTALE);
        }
        if self.object_slot(kind).is_none() {
            return Err(Errno::EINVAL);
        }
        let bytes = encode_shell_file_record(
            ShellFileHeader {
                kind,
                connection_epoch: self.epoch,
                submission_id: 0,
                sequence: 0,
            },
            body,
        )
        .map_err(|_| Errno::EINVAL)?;
        let generation = match kind {
            ShellFileKind::Outputs => {
                let facts = decode_shell_file_outputs(&bytes).map_err(|_| Errno::EINVAL)?;
                let ShellContentRecord::OutputFacts(facts) = facts.record else {
                    return Err(Errno::EINVAL);
                };
                facts.facts_generation
            }
            ShellFileKind::Catalog => {
                decode_shell_file_catalog(&bytes)
                    .map_err(|_| Errno::EINVAL)?
                    .catalog
                    .catalog
                    .generation
            }
            ShellFileKind::Indicators => {
                decode_shell_file_indicators(&bytes)
                    .map_err(|_| Errno::EINVAL)?
                    .snapshot
                    .generation
            }
            _ => return Err(Errno::EINVAL),
        };
        let event = SHELL_FILE_HEADER_BYTES + 24;
        if !self.journal.fits(event, credited) {
            return Ok(false);
        }
        let qid = self.allocate_qid()?;
        let published = encode_shell_file_object_published_body(ShellFileObjectPublished {
            object: kind,
            generation,
            qid,
        })
        .map_err(|_| Errno::EINVAL)?;
        self.journal
            .append(ShellFileKind::ObjectPublished, &published, credited)?;
        self.object_slot_mut(kind).expect("checked above").current =
            Some(Arc::new(Object { kind, qid, bytes }));
        Ok(true)
    }
}
