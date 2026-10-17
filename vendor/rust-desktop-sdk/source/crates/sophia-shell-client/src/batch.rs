//! Atomic local admission of independent content operations.
use crate::wire::Outbound;
use crate::{Admission, ContentReservation, QueueReservation, ShellClientError, ShellConnection};
use sophia_shell_protocol::shell_files::{
    ShellFileDescriptorRecord, ShellFileNativeCandidate, ShellFileNativeLauncherRecord,
};
use sophia_shell_protocol::{
    CatalogCandidateBegin, ContentCandidateChunk, ContentCandidateEnd, ShellIndicatorActivation,
};
use sophia_shell_protocol::{ShellContentRecord, TransactionId};

/// Whole role operations for immediate, prepared or delayed atomic admission.
/// Each item encodes as one unit with its own custody ticket. Role negotiation
/// and record validation still apply; this does not register lifecycle metadata.
#[derive(Clone, Copy)]
pub enum ShellBatchItem<'a> {
    CatalogActivation(TransactionId, &'a sophia_shell_protocol::CatalogActivation),
    Content(ContentBatchItem<'a>),
    Native(&'a ShellFileNativeLauncherRecord),
    NativeCandidate(&'a ShellFileNativeCandidate),
    Descriptor(&'a ShellFileDescriptorRecord),
    IndicatorActivation(TransactionId, &'a ShellIndicatorActivation),
    CatalogCandidate {
        transaction: TransactionId,
        begin: &'a CatalogCandidateBegin,
        chunks: &'a [ContentCandidateChunk],
        end: &'a ContentCandidateEnd,
    },
}
impl ShellBatchItem<'_> {
    pub(crate) fn outbound(self) -> Outbound {
        match self {
            Self::CatalogActivation(tx, value) => Outbound::CatalogActivation(tx, value.clone()),
            Self::Content(item) => item.outbound(),
            Self::Native(value) => Outbound::Native(value.clone()),
            Self::NativeCandidate(value) => Outbound::NativeCandidate(value.clone()),
            Self::Descriptor(value) => Outbound::Descriptor(value.clone()),
            Self::IndicatorActivation(tx, value) => Outbound::IndicatorActivation(tx, *value),
            Self::CatalogCandidate {
                transaction,
                begin,
                chunks,
                end,
            } => Outbound::CatalogCandidateGroup {
                transaction,
                begin: begin.clone(),
                chunks: chunks.to_vec(),
                end: end.clone(),
            },
        }
    }
    fn is_control(self) -> bool {
        match self {
            Self::CatalogActivation(..) => true,
            Self::Content(item) => item.is_control(),
            Self::Native(value) => matches!(value.record, sophia_shell_protocol::ShellNativeLauncherRecord::InputAck(_) | sophia_shell_protocol::ShellNativeLauncherRecord::Activate(_)),
            Self::Descriptor(value) => matches!(value.record, sophia_shell_protocol::shell_files::ShellDescriptorRecord::DescriptorActivationAck(_) | sophia_shell_protocol::shell_files::ShellDescriptorRecord::LauncherActivationAck(_)),
            _ => false,
        }
    }
}

/// One operation in a bounded content batch. A whole candidate is one file
/// record; Begin/Chunk/End fragments are not independent candidate submissions.
#[derive(Clone, Copy)]
pub enum ContentBatchItem<'a> {
    Record(TransactionId, &'a ShellContentRecord),
    Candidate(TransactionId, &'a [ShellContentRecord]),
}

impl ContentBatchItem<'_> {
    pub(crate) fn outbound(self) -> Outbound {
        match self {
            Self::Record(tx, record) => Outbound::Content(tx, record.clone()),
            Self::Candidate(tx, records) => Outbound::ContentGroup(tx, records.to_vec()),
        }
    }

    fn is_control(self) -> bool {
        matches!(self, Self::Record(_, ShellContentRecord::ActionAck(_)))
    }
}

impl ShellConnection {
    fn validate_shell_batch(&self, items: &[ShellBatchItem<'_>]) -> Result<(), ShellClientError> {
        for item in items {
            if let ShellBatchItem::CatalogActivation(_, value) = item {
                self.require_catalog()?;
                if value.action.grant.connection_epoch != self.connection_epoch() {
                    return Err(ShellClientError::WrongDirection);
                }
            }
            if let ShellBatchItem::CatalogCandidate { begin, chunks, .. } = item {
                self.require_catalog()?;
                if chunks.len() > crate::MAX_QUEUED_FRAMES / 2 - 2 {
                    return Err(ShellClientError::QueueSaturated);
                }
                if begin.content.grant.connection_epoch != self.connection_epoch() {
                    return Err(ShellClientError::WrongDirection);
                }
            }
        }
        Ok(())
    }
    /// Atomically admit whole role records. Input is copied. Mixed batches use
    /// bulk capacity; all-control batches retain ACK/activation headroom.
    pub fn enqueue_shell_batch(
        &mut self,
        items: &[ShellBatchItem<'_>],
    ) -> Result<Admission, ShellClientError> {
        Ok(self.reserve_shell_batch(items)?.commit())
    }
    /// Prepare all items before local effects. Drop cancels all staged claims;
    /// commit performs no allocation or I/O. Remote outcomes remain independent.
    pub fn reserve_shell_batch(
        &mut self,
        items: &[ShellBatchItem<'_>],
    ) -> Result<ContentReservation<'_>, ShellClientError> {
        self.wire.io_interest()?;
        self.validate_shell_batch(items)?;
        let units = self.wire.encode_shell_batch(items)?;
        self.wire.reserve_encoded()?;
        let prepared = self.output.prepare(
            units,
            items.iter().all(|v| v.is_control()),
            &mut self.ledger,
        )?;
        Ok(ContentReservation {
            connection: self,
            prepared: Some(prepared),
        })
    }
    /// Commit role records against a serviceable capacity token. Validation,
    /// sizing or allocation failure retains its credits; success consumes it.
    /// No lifecycle helper state is registered by this lower-level operation.
    pub fn commit_reserved_shell_batch(
        &mut self,
        token: &QueueReservation,
        items: &[ShellBatchItem<'_>],
    ) -> Result<Admission, ShellClientError> {
        if let Err(error) = self.wire.io_interest() {
            self.output.release_reservation();
            return Err(error);
        }
        self.output.validate_reservation(token)?;
        self.validate_shell_batch(items)?;
        let units = self.wire.encode_shell_batch(items)?;
        self.wire.reserve_encoded()?;
        let prepared = self.output.prepare_reserved(
            token,
            units,
            items.iter().all(|v| v.is_control()),
            &mut self.ledger,
        )?;
        let admission = self.output.commit(prepared, &mut self.ledger);
        self.wire.commit_encoded();
        Ok(admission)
    }
    /// Admit all operations or none, with one ticket per item and no socket
    /// I/O. Each operation still has its own remote custody and outcome; this
    /// is not an atomic server transaction. Mixed batches use bulk credits;
    /// only all-ACK batches use the control allowance. Input is copied.
    pub fn enqueue_content_batch(
        &mut self,
        items: &[ContentBatchItem<'_>],
    ) -> Result<Admission, ShellClientError> {
        Ok(self.reserve_content_batch(items)?.commit())
    }

    /// Prepare an atomic batch before a local effect. Drop cancels every item
    /// and staged upload claim. Commit cannot allocate, perform I/O or fail for
    /// capacity. As with other prepared handles, the connection stays borrowed.
    /// This does not register ContentLifecycle metadata.
    pub fn reserve_content_batch(
        &mut self,
        items: &[ContentBatchItem<'_>],
    ) -> Result<ContentReservation<'_>, ShellClientError> {
        self.wire.io_interest()?;
        let units = self.wire.encode_content_batch(items)?;
        self.wire.reserve_encoded()?;
        let prepared = self.output.prepare(
            units,
            items.iter().all(|item| item.is_control()),
            &mut self.ledger,
        )?;
        Ok(ContentReservation {
            connection: self,
            prepared: Some(prepared),
        })
    }

    /// Commit an atomic content batch against delayed-content capacity. Size,
    /// encoding or allocation failure preserves the reservation and issues no
    /// tickets. Successful commit releases unused credits. An observed terminal
    /// connection releases the reservation and admits nothing.
    pub fn commit_reserved_content_batch(
        &mut self,
        token: &QueueReservation,
        items: &[ContentBatchItem<'_>],
    ) -> Result<Admission, ShellClientError> {
        if let Err(error) = self.wire.io_interest() {
            self.output.release_reservation();
            return Err(error);
        }
        self.output.validate_reservation(token)?;
        let units = self.wire.encode_content_batch(items)?;
        self.wire.reserve_encoded()?;
        let prepared = self.output.prepare_reserved(
            token,
            units,
            items.iter().all(|item| item.is_control()),
            &mut self.ledger,
        )?;
        let admission = self.output.commit(prepared, &mut self.ledger);
        self.wire.commit_encoded();
        Ok(admission)
    }
}
