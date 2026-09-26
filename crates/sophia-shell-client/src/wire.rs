//! Whole typed units carried between a shell and its session, independent of
//! any one transport. Exactly one implementation exists today (the
//! Unix-socket wire in `socket`, the only module that may know a frame
//! exists); a later native 9P file wire adds a second `Wire` variant that
//! encodes and assembles the same units differently.

use std::collections::VecDeque;

use sophia_protocol::{
    CatalogActivation, CatalogActivationOutcome, CatalogCandidateBegin, ContentActionAck,
    ContentCandidateChunk, ContentCandidateEnd, ShellContentRecord, ShellIndicatorActivation,
    ShellIndicatorActivationOutcome, ShellIndicatorSnapshot, ShellPersistentCatalog, TransactionId,
};

use crate::socket::SocketWire;
use crate::{ShellClientError, outbox::ClientOutbox};

/// One whole client-to-session unit. Named for what it means, never for how
/// many frames a wire needs to carry it.
pub(crate) enum Outbound {
    /// One content record.
    Content(TransactionId, ShellContentRecord),
    /// A complete bounded group of content records owned atomically (for
    /// example a candidate's Begin/Chunk*/End). A native wire may encode this
    /// as exactly one record; the socket wire still needs one frame each.
    ContentGroup(TransactionId, Vec<ShellContentRecord>),
    /// One indicator activation naming an exact published generation.
    IndicatorActivation(TransactionId, ShellIndicatorActivation),
    /// An action ACK, optionally paired atomically with the indicator
    /// activation its disposition authorizes.
    ActionResponse {
        transaction: TransactionId,
        ack: ContentActionAck,
        activation: Option<(TransactionId, ShellIndicatorActivation)>,
    },
    /// A complete revision-8 catalog candidate: Begin/Chunk*/common End.
    CatalogCandidateGroup {
        transaction: TransactionId,
        begin: CatalogCandidateBegin,
        chunks: Vec<ContentCandidateChunk>,
        end: ContentCandidateEnd,
    },
    /// An action ACK, optionally paired atomically with the catalog
    /// activation its disposition authorizes.
    CatalogActionResponse {
        transaction: TransactionId,
        ack: ContentActionAck,
        activation: Option<(TransactionId, CatalogActivation)>,
    },
}

impl Outbound {
    /// Whether this unit spends control (ACK/activation) capacity rather
    /// than bulk capacity, matching the r5 outbox split enforced today.
    pub(crate) fn is_control(&self) -> bool {
        match self {
            Outbound::Content(_, record) => matches!(record, ShellContentRecord::ActionAck(_)),
            Outbound::ContentGroup(..)
            | Outbound::IndicatorActivation(..)
            | Outbound::CatalogCandidateGroup { .. } => false,
            Outbound::ActionResponse { .. } | Outbound::CatalogActionResponse { .. } => true,
        }
    }
}

/// One whole session-to-client unit. A multi-frame wire transfer (indicator
/// Begin/.../End, catalog Begin/Entry/Identity/End) is assembled inside the
/// owning wire and only ever surfaces here as one complete value.
pub(crate) enum Inbound {
    Content(TransactionId, ShellContentRecord),
    Indicators(TransactionId, ShellIndicatorSnapshot),
    IndicatorOutcome(TransactionId, ShellIndicatorActivationOutcome),
    Catalog(TransactionId, ShellPersistentCatalog),
    CatalogOutcome(TransactionId, CatalogActivationOutcome),
}

/// The connection's transport. An enum, not a trait object, so a later
/// native 9P file wire is a plain additional variant next to `Socket`.
pub(crate) enum Wire {
    Socket(SocketWire),
}

impl Wire {
    /// Turn one whole outbound unit into the wire's own encoded units (wire
    /// frames for the socket). Outbox accounting applies to those units
    /// unchanged, whatever a future wire's unit shape turns out to be.
    pub(crate) fn encode(&self, outbound: Outbound) -> Result<Vec<Vec<u8>>, ShellClientError> {
        match self {
            Wire::Socket(_) => crate::socket::encode(outbound),
        }
    }

    /// Bounded nonblocking progress: write queued encoded units, and
    /// read/decode into typed `Inbound` values.
    pub(crate) fn poll_io(
        &mut self,
        output: &mut ClientOutbox,
        inbox: &mut VecDeque<Inbound>,
    ) -> Result<(), ShellClientError> {
        match self {
            Wire::Socket(socket) => socket.poll_io(output, inbox),
        }
    }

    pub(crate) fn peer_closed(&self) -> bool {
        match self {
            Wire::Socket(socket) => socket.peer_closed(),
        }
    }
}
