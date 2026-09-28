//! Whole typed units carried between a shell and its session, independent of
//! any one transport. Exactly one implementation exists today (the
//! Unix-socket wire in `socket`, the only module that may know a frame
//! exists); a later native 9P file wire adds a second `Wire` variant that
//! encodes and assembles the same units differently.

use std::collections::VecDeque;

use sophia_shell_protocol::{
    CatalogActivation, CatalogActivationOutcome, CatalogCandidateBegin, ContentActionAck,
    ContentCandidateChunk, ContentCandidateEnd, ShellContentRecord, ShellIndicatorActivation,
    ShellIndicatorActivationOutcome, ShellIndicatorSnapshot, ShellPersistentCatalog, TransactionId,
};

use crate::custody::Ledger;
use crate::files::FileWire;
#[cfg(feature = "ipc-compat")]
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

/// The connection's transport. An enum, not a trait object, so the native 9P
/// file wire is a plain additional variant next to `Socket`. `FileWire` owns
/// a whole `Pipeline` plus its own submission/upload/event state, far larger
/// than `SocketWire`; boxing it keeps every `Outbound`/`Inbound` value (and
/// this enum's own stack footprint) from paying for that on the socket path.
pub(crate) enum Wire {
    #[cfg(feature = "ipc-compat")]
    Socket(SocketWire),
    Files(Box<FileWire>),
}

impl Wire {
    pub(crate) fn wait_for_io(
        &self,
        maximum: std::time::Duration,
        _output: &ClientOutbox,
    ) -> Result<(), ShellClientError> {
        match self {
            #[cfg(feature = "ipc-compat")]
            Wire::Socket(socket) => socket.wait_for_io(maximum, _output.front().is_some()),
            Wire::Files(files) => files.wait_for_io(maximum),
        }
    }

    /// Turn one whole outbound unit into the wire's own encoded units (wire
    /// frames for the socket; one file record or one slot write for the file
    /// wire). Outbox accounting applies to those units unchanged. The file
    /// wire's encoding is stateful (it assigns submission ids and upload
    /// slots), hence `&mut self`; call [`Self::commit_encoded`] once the
    /// returned units are actually admitted into the outbox.
    pub(crate) fn encode(&mut self, outbound: Outbound) -> Result<Vec<Vec<u8>>, ShellClientError> {
        match self {
            #[cfg(feature = "ipc-compat")]
            Wire::Socket(_) => crate::socket::encode(outbound),
            Wire::Files(files) => files.encode(outbound),
        }
    }

    /// Hands the wire ownership of exactly the units the last successful
    /// `encode` call staged, now that the outbox has admitted them. A no-op
    /// for the socket wire, whose outbox frames are themselves the units on
    /// the wire.
    pub(crate) fn commit_encoded(&mut self) {
        match self {
            #[cfg(feature = "ipc-compat")]
            Wire::Socket(_) => {}
            Wire::Files(files) => files.commit_encoded(),
        }
    }

    /// Bounded nonblocking progress: write queued encoded units, and
    /// read/decode into typed `Inbound` values.
    pub(crate) fn poll_io(
        &mut self,
        output: &mut ClientOutbox,
        inbox: &mut VecDeque<Inbound>,
        ledger: &mut Ledger,
    ) -> Result<(), ShellClientError> {
        match self {
            #[cfg(feature = "ipc-compat")]
            Wire::Socket(socket) => socket.poll_io(output, inbox, ledger),
            Wire::Files(files) => files.poll_io(output, inbox, ledger),
        }
    }

    /// When the connection next needs servicing even without I/O: a
    /// refused write's retry falls due then.
    pub(crate) fn wake_deadline(&self) -> Option<std::time::Instant> {
        match self {
            #[cfg(feature = "ipc-compat")]
            Wire::Socket(_) => None,
            Wire::Files(files) => files.wake_deadline(),
        }
    }

    pub(crate) fn peer_closed(&self) -> bool {
        match self {
            #[cfg(feature = "ipc-compat")]
            Wire::Socket(socket) => socket.peer_closed(),
            Wire::Files(files) => files.peer_closed(),
        }
    }
}
