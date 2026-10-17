//! Whole typed units carried between a shell and its session, independent of
//! their encoding. The file wire owns 9P custody and whole-object reads.

use sophia_shell_protocol::{
    CatalogActivation, CatalogActivationOutcome, CatalogCandidateBegin, ContentActionAck,
    ContentCandidateChunk, ContentCandidateEnd, ShellContentRecord, ShellIndicatorActivation,
    ShellIndicatorActivationOutcome, ShellIndicatorSnapshot, ShellPersistentCatalog, TransactionId,
};

/// One whole client-to-session unit. Named for what it means, never for how
/// many frames a wire needs to carry it.
pub(crate) enum Outbound {
    CatalogActivation(TransactionId, CatalogActivation),
    NativeActionResponse {
        transaction: TransactionId,
        ack: ContentActionAck,
        activation: Option<(
            TransactionId,
            sophia_shell_protocol::NativeLauncherActivation,
        )>,
    },
    Native(sophia_shell_protocol::shell_files::ShellFileNativeLauncherRecord),
    NativeCandidate(sophia_shell_protocol::shell_files::ShellFileNativeCandidate),
    NativeResponse {
        transaction: TransactionId,
        ack: sophia_shell_protocol::NativeLauncherInputAck,
        activation: Option<(
            TransactionId,
            sophia_shell_protocol::NativeLauncherActivation,
        )>,
    },
    Descriptor(sophia_shell_protocol::shell_files::ShellFileDescriptorRecord),
    /// One content record.
    Content(TransactionId, ShellContentRecord),
    /// A complete bounded group of content records owned atomically (for
    /// example a candidate's Begin/Chunk*/End), encoded as one file record.
    ContentGroup(TransactionId, Vec<ShellContentRecord>),
    /// One r9 ImageBegin candidate: the producer's 96-byte body.
    ImageBegin([u8; 96]),
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
            Outbound::Native(value) => matches!(value.record,
                sophia_shell_protocol::ShellNativeLauncherRecord::InputAck(_)
                | sophia_shell_protocol::ShellNativeLauncherRecord::Activate(_)),
            Outbound::NativeCandidate(_) => false,
            Outbound::NativeResponse { .. } | Outbound::NativeActionResponse { .. } => true,
            Outbound::Descriptor(value) => matches!(value.record,
                sophia_shell_protocol::shell_files::ShellDescriptorRecord::DescriptorActivationAck(_)
                | sophia_shell_protocol::shell_files::ShellDescriptorRecord::LauncherActivationAck(_)),
            Outbound::Content(_, record) => matches!(record, ShellContentRecord::ActionAck(_)),
            Outbound::ContentGroup(..)
            | Outbound::ImageBegin(..)
            | Outbound::IndicatorActivation(..)
            | Outbound::CatalogCandidateGroup { .. } => false,
            Outbound::ActionResponse { .. } | Outbound::CatalogActionResponse { .. } | Outbound::CatalogActivation(..) => true,
        }
    }
}

/// One complete session-to-client value decoded from file records.
pub(crate) enum Inbound {
    NativePublication(sophia_shell_protocol::shell_files::ShellFileObjectPublished),
    Native(sophia_shell_protocol::shell_files::ShellFileNativeLauncherRecord),
    Descriptor(sophia_shell_protocol::shell_files::ShellFileDescriptorRecord),
    ApplicationCatalog(
        TransactionId,
        sophia_shell_protocol::ShellApplicationCatalog,
    ),
    Content(TransactionId, ShellContentRecord),
    Indicators(TransactionId, ShellIndicatorSnapshot),
    IndicatorOutcome(TransactionId, ShellIndicatorActivationOutcome),
    Catalog(TransactionId, ShellPersistentCatalog),
    CatalogOutcome(TransactionId, CatalogActivationOutcome),
}
