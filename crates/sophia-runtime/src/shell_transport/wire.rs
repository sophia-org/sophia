//! The one wire of a component epoch and the typed boundary owners use.
//!
//! Owners ask for a family of typed client records and hand over typed
//! Session records; the file export selects each record family. Nothing here
//! names an IPC frame kind or file offset.
use sophia_protocol::{
    CatalogActivation, CatalogCandidateBegin, ContentCandidateChunk, ContentCandidateEnd,
    NativeLauncherActivation, NativeLauncherCandidateBegin, NativeLauncherInputAck,
    ShellContentRecord, ShellIndicatorActivation, TransactionId,
};

use super::native_launcher::NativeContentRecord;
use super::{ShellComponentTransport, ShellTransportError, files};

/// A family of content records one owner services.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ContentWant {
    /// Resource Begin, Chunk, End, Cancel and Retire.
    Resource,
    AllocationRequest,
    /// Frame demands and their cancellations.
    Demand,
    ActionAck,
    /// The Begin, Chunk and End of a base content candidate, in order.
    CandidatePart,
}

impl ContentWant {
    pub(super) fn selects(self, record: &ShellContentRecord) -> bool {
        match self {
            Self::Resource => matches!(
                record,
                ShellContentRecord::ResourceBegin(_)
                    | ShellContentRecord::ResourceChunk(_)
                    | ShellContentRecord::ResourceEnd(_)
                    | ShellContentRecord::ResourceCancel(_)
                    | ShellContentRecord::ResourceRetire(_)
            ),
            Self::AllocationRequest => matches!(record, ShellContentRecord::AllocationRequest(_)),
            Self::Demand => matches!(
                record,
                ShellContentRecord::FrameDemand(_) | ShellContentRecord::FrameDemandCancel(_)
            ),
            Self::ActionAck => matches!(record, ShellContentRecord::ActionAck(_)),
            Self::CandidatePart => matches!(
                record,
                ShellContentRecord::CandidateBegin(_)
                    | ShellContentRecord::CandidateChunk(_)
                    | ShellContentRecord::CandidateEnd(_)
            ),
        }
    }
}

/// The parts of one native-launcher candidate, in wire order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::shell_transport) enum NativeCandidatePart {
    Begin(NativeLauncherCandidateBegin),
    Chunk(ContentCandidateChunk),
    End(ContentCandidateEnd),
}

/// The parts of one catalog candidate, in wire order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::shell_transport) enum CatalogCandidatePart {
    Begin(CatalogCandidateBegin),
    Chunk(ContentCandidateChunk),
    End(ContentCandidateEnd),
}

impl CatalogCandidatePart {
    pub(super) fn grant(&self) -> sophia_protocol::ContentGrant {
        match self {
            Self::Begin(value) => value.content.grant,
            Self::Chunk(value) => value.grant,
            Self::End(value) => value.grant,
        }
    }
}

impl ShellComponentTransport {
    pub(super) fn files_mut(&mut self) -> Option<&mut files::ShellFileWire> {
        self.wire.as_deref_mut()
    }

    /// An empty inbound answer: nothing yet, or the peer's stream has ended.
    pub(super) fn nothing_inbound<T>(&self) -> Result<Option<T>, ShellTransportError> {
        if self.peer_closed {
            Err(ShellTransportError::NotConnected)
        } else {
            Ok(None)
        }
    }

    /// Whether the export holds no accepted record or candidate part the
    /// owners have not taken. This does not inspect the peer's outgoing queue.
    pub(super) fn inbound_idle(&self) -> bool {
        match self.wire.as_ref() {
            None => true,
            Some(files) => files.export().inbound_is_empty(),
        }
    }

    /// The oldest queued content record of `want`, left queued.
    pub(super) fn peek_content(
        &self,
        want: ContentWant,
    ) -> Result<Option<(TransactionId, ShellContentRecord)>, ShellTransportError> {
        match self.wire.as_ref() {
            None => Ok(None),
            Some(files) => Ok(files
                .export()
                .peek_content(|record| want.selects(record))
                .map(|(transaction, record)| (transaction, record.clone()))),
        }
    }

    /// Removes the oldest queued content record of `want`, which is the one
    /// `peek_content` reports when nothing changed in between.
    pub(super) fn take_content(
        &mut self,
        want: ContentWant,
    ) -> Result<Option<(TransactionId, ShellContentRecord)>, ShellTransportError> {
        match self.wire.as_mut() {
            None => Ok(None),
            Some(files) if want == ContentWant::CandidatePart => {
                Ok(files.export_mut().take_candidate_part())
            }
            Some(files) => Ok(files
                .export_mut()
                .take_content(|record| want.selects(record))),
        }
    }

    /// Drops the rest of a candidate whose earlier part was answered with a
    /// rejecting outcome. File submission accepts a whole candidate, so its
    /// later parts remain retained until the owner takes or discards them.
    pub(super) fn discard_candidate_rest(&mut self, family: files::CandidateFamily) {
        if let Some(files) = self.files_mut() {
            let export = files.export_mut();
            match family {
                files::CandidateFamily::Base => export.discard_candidate_parts(),
                files::CandidateFamily::Native => export.discard_native_candidate_parts(),
                files::CandidateFamily::Catalog => export.discard_catalog_candidate_parts(),
            }
        }
    }

    pub(super) fn peek_native_input_ack(
        &self,
    ) -> Result<Option<(TransactionId, NativeLauncherInputAck)>, ShellTransportError> {
        match self.wire.as_ref() {
            None => Ok(None),
            Some(files) => Ok(files.export().peek_native_input_ack()),
        }
    }

    pub(super) fn take_native_input_ack(&mut self) {
        match self.wire.as_mut() {
            None => {}
            Some(files) => {
                files.export_mut().take_native_input_ack();
            }
        }
    }

    pub(super) fn peek_native_activate(
        &self,
    ) -> Result<Option<(TransactionId, NativeLauncherActivation)>, ShellTransportError> {
        match self.wire.as_ref() {
            None => Ok(None),
            Some(files) => Ok(files.export().peek_native_activate()),
        }
    }

    pub(super) fn take_native_activate(&mut self) {
        match self.wire.as_mut() {
            None => {}
            Some(files) => {
                files.export_mut().take_native_activate();
            }
        }
    }

    pub(super) fn peek_catalog_activate(
        &self,
    ) -> Result<Option<(TransactionId, CatalogActivation)>, ShellTransportError> {
        match self.wire.as_ref() {
            None => Ok(None),
            Some(files) => Ok(files.export().peek_catalog_activate()),
        }
    }

    pub(super) fn take_catalog_activate(&mut self) {
        match self.wire.as_mut() {
            None => {}
            Some(files) => {
                files.export_mut().take_catalog_activate();
            }
        }
    }

    pub(super) fn peek_indicator_activate(
        &self,
    ) -> Result<Option<(TransactionId, ShellIndicatorActivation)>, ShellTransportError> {
        match self.wire.as_ref() {
            None => Ok(None),
            Some(files) => Ok(files.export().peek_indicator_activate()),
        }
    }

    pub(super) fn take_indicator_activate(&mut self) {
        match self.wire.as_mut() {
            None => {}
            Some(files) => {
                files.export_mut().take_indicator_activate();
            }
        }
    }

    /// The oldest native-launcher content record within this visit's
    /// allowance, left queued.
    pub(super) fn peek_native_content(
        &mut self,
    ) -> Result<Option<(TransactionId, NativeContentRecord)>, ShellTransportError> {
        match self.wire.as_mut() {
            None => Ok(None),
            Some(files) => super::native_launcher::peek_native_file_record(files.export_mut()),
        }
    }

    /// Removes exactly the record `peek_native_content` last reported.
    pub(super) fn take_native_content(&mut self) {
        match self.wire.as_mut() {
            None => {}
            Some(files) => {
                super::native_launcher::take_native_file_record(files.export_mut());
            }
        }
    }

    /// The oldest catalog candidate part within this visit's allowance, left
    /// queued. Another candidate family at the front is a protocol violation.
    pub(super) fn peek_catalog_candidate(
        &mut self,
    ) -> Result<Option<(TransactionId, CatalogCandidatePart)>, ShellTransportError> {
        match self.wire.as_mut() {
            None => Ok(None),
            Some(files) => {
                let export = files.export_mut();
                if let Some(part) = export.peek_catalog_candidate_part() {
                    return Ok(Some(part));
                }
                match export.peek_candidate_family() {
                    None => Ok(None),
                    Some(files::CandidateFamily::Catalog) => {
                        unreachable!("peek_catalog_candidate_part already covers this")
                    }
                    Some(_) => Err(ShellTransportError::WrongContentRecord),
                }
            }
        }
    }

    pub(super) fn take_catalog_candidate(&mut self) {
        match self.wire.as_mut() {
            None => {}
            Some(files) => {
                files.export_mut().take_catalog_candidate_part();
            }
        }
    }
}
