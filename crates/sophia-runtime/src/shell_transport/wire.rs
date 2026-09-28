//! The one wire of a component epoch and the typed boundary owners use.
//!
//! Owners ask for a family of typed client records and hand over typed
//! Session records; each wire maps a request to its own selection and
//! encoding. Nothing here names a frame kind, header or file offset.
use sophia_protocol::{
    CatalogActivation, CatalogCandidateBegin, ContentCandidateChunk, ContentCandidateEnd,
    NativeLauncherActivation, NativeLauncherCandidateBegin, NativeLauncherInputAck,
    ShellContentRecord, ShellIndicatorActivation, TransactionId,
};

use super::native_launcher::NativeContentRecord;
use super::{ShellComponentTransport, ShellTransportError, files, socket};

/// Boxed: one wire per component epoch, allocated at negotiation.
pub(super) enum Wire {
    Socket(Box<socket::SocketWire>),
    Files(Box<files::ShellFileWire>),
}

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
    pub(super) fn socket(&self) -> Option<&socket::SocketWire> {
        match self.wire.as_ref()? {
            Wire::Socket(socket) => Some(socket.as_ref()),
            Wire::Files(_) => None,
        }
    }

    pub(super) fn socket_mut(&mut self) -> Option<&mut socket::SocketWire> {
        match self.wire.as_mut()? {
            Wire::Socket(socket) => Some(socket.as_mut()),
            Wire::Files(_) => None,
        }
    }

    pub(super) fn files_mut(&mut self) -> Option<&mut files::ShellFileWire> {
        match self.wire.as_mut()? {
            Wire::Files(files) => Some(files.as_mut()),
            Wire::Socket(_) => None,
        }
    }

    /// An empty inbound answer: nothing yet, or the peer's stream has ended.
    pub(super) fn nothing_inbound<T>(&self) -> Result<Option<T>, ShellTransportError> {
        if self.peer_closed {
            Err(ShellTransportError::NotConnected)
        } else {
            Ok(None)
        }
    }

    /// Starts one bounded owner visit on the wire.
    pub(super) fn begin_inbound_visit(&mut self) {
        if let Some(socket) = self.socket_mut() {
            socket.begin_visit();
        }
    }

    /// Whether the wire holds no unread or untaken client input: on the
    /// socket, no partial or complete frame; on the file wire, no accepted
    /// record or candidate part the owners have not taken.
    pub(super) fn inbound_idle(&self) -> bool {
        match self.wire.as_ref() {
            None => true,
            Some(Wire::Socket(socket)) => socket.input_idle(),
            Some(Wire::Files(files)) => files.export().inbound_is_empty(),
        }
    }

    /// The oldest queued content record of `want`, left queued.
    pub(super) fn peek_content(
        &self,
        want: ContentWant,
    ) -> Result<Option<(TransactionId, ShellContentRecord)>, ShellTransportError> {
        match self.wire.as_ref() {
            None => Ok(None),
            Some(Wire::Socket(socket)) => socket.peek_content(want),
            Some(Wire::Files(files)) => Ok(files
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
            Some(Wire::Socket(socket)) => socket.take_content(want),
            Some(Wire::Files(files)) if want == ContentWant::CandidatePart => {
                Ok(files.export_mut().take_candidate_part())
            }
            Some(Wire::Files(files)) => Ok(files
                .export_mut()
                .take_content(|record| want.selects(record))),
        }
    }

    /// Drops the rest of a candidate whose earlier part was answered with a
    /// rejecting outcome. Only a wire that delivers whole candidates retains
    /// such a rest; a socket peer sends each part separately.
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
            Some(Wire::Socket(socket)) => socket.peek_native_input_ack(),
            Some(Wire::Files(files)) => Ok(files.export().peek_native_input_ack()),
        }
    }

    pub(super) fn take_native_input_ack(&mut self) {
        match self.wire.as_mut() {
            None => {}
            Some(Wire::Socket(socket)) => socket.take_native_input_ack(),
            Some(Wire::Files(files)) => {
                files.export_mut().take_native_input_ack();
            }
        }
    }

    pub(super) fn peek_native_activate(
        &self,
    ) -> Result<Option<(TransactionId, NativeLauncherActivation)>, ShellTransportError> {
        match self.wire.as_ref() {
            None => Ok(None),
            Some(Wire::Socket(socket)) => socket.peek_native_activate(),
            Some(Wire::Files(files)) => Ok(files.export().peek_native_activate()),
        }
    }

    pub(super) fn take_native_activate(&mut self) {
        match self.wire.as_mut() {
            None => {}
            Some(Wire::Socket(socket)) => socket.take_native_activate(),
            Some(Wire::Files(files)) => {
                files.export_mut().take_native_activate();
            }
        }
    }

    pub(super) fn peek_catalog_activate(
        &self,
    ) -> Result<Option<(TransactionId, CatalogActivation)>, ShellTransportError> {
        match self.wire.as_ref() {
            None => Ok(None),
            Some(Wire::Socket(socket)) => socket.peek_catalog_activate(),
            Some(Wire::Files(files)) => Ok(files.export().peek_catalog_activate()),
        }
    }

    pub(super) fn take_catalog_activate(&mut self) {
        match self.wire.as_mut() {
            None => {}
            Some(Wire::Socket(socket)) => socket.take_catalog_activate(),
            Some(Wire::Files(files)) => {
                files.export_mut().take_catalog_activate();
            }
        }
    }

    pub(super) fn peek_indicator_activate(
        &self,
    ) -> Result<Option<(TransactionId, ShellIndicatorActivation)>, ShellTransportError> {
        match self.wire.as_ref() {
            None => Ok(None),
            Some(Wire::Socket(socket)) => socket.peek_indicator_activate(),
            Some(Wire::Files(files)) => Ok(files.export().peek_indicator_activate()),
        }
    }

    pub(super) fn take_indicator_activate(&mut self) {
        match self.wire.as_mut() {
            None => {}
            Some(Wire::Socket(socket)) => socket.take_indicator_activate(),
            Some(Wire::Files(files)) => {
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
            Some(Wire::Socket(socket)) => socket.peek_native_content(),
            Some(Wire::Files(files)) => {
                super::native_launcher::peek_native_file_record(files.export_mut())
            }
        }
    }

    /// Removes exactly the record `peek_native_content` last reported.
    pub(super) fn take_native_content(&mut self) {
        match self.wire.as_mut() {
            None => {}
            Some(Wire::Socket(socket)) => socket.take_native_content(),
            Some(Wire::Files(files)) => {
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
            Some(Wire::Socket(socket)) => socket.peek_catalog_candidate(),
            Some(Wire::Files(files)) => {
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
            Some(Wire::Socket(socket)) => socket.take_catalog_candidate(),
            Some(Wire::Files(files)) => {
                files.export_mut().take_catalog_candidate_part();
            }
        }
    }
}
