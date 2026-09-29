use crate::TransactionId;

pub const SOPHIA_IPC_MAGIC: u32 = 0x4850_4f53;
pub const SOPHIA_IPC_VERSION: u16 = 1;
pub const SOPHIA_IPC_HEADER_LEN: usize = 24;
pub const SOPHIA_IPC_MAX_PAYLOAD_LEN: usize = 64 * 1024;
pub const SOPHIA_IPC_MAX_ITEMS: usize = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IpcMessageKind {
    BrokerHealth = 3,
    XAuthorityRequest = 4,
    XAuthorityResponse = 5,
    PortalBrokerRequest = 6,
    PortalBrokerResponse = 7,
    PortalClipboardPayload = 8,
    WmV1ClientHello = 32,
    WmV1ServerWelcome = 33,
    WmV1SnapshotBegin = 34,
    WmV1SnapshotChunk = 35,
    WmV1SnapshotEnd = 36,
    WmV1ProjectionRequest = 37,
    WmV1ProjectionBegin = 38,
    WmV1ProjectionChunk = 39,
    WmV1ProjectionEnd = 40,
    WmV1ProjectionOutcome = 41,
    WmV1PolicyConfiguration = 42,
    WmV1PolicyConfigurationOutcome = 43,
    WmV1PolicyDirty = 44,
    WmV1SessionOperationRequest = 45,
    WmV1SessionOperationOutcome = 46,
    WmV1ProfilePrepare = 47,
    WmV1ProfilePrepared = 48,
    WmV1ProfileActivate = 49,
    WmV1ProfileActive = 50,
    WmV1ProfileRollback = 51,
    WmV1ProfileRolledBack = 52,
    WmV1OutputActionRequest = 53,
    WmV1PresentationActionRequest = 54,
    WmV1PresentationOutcome = 55,
    OutputV1ClientHello = 64,
    OutputV1ServerWelcome = 65,
    OutputV1Snapshot = 66,
    OutputV1Proposal = 67,
    OutputV1Outcome = 68,
    BrokerV1ClientHello = 80,
    BrokerV1ServerWelcome = 81,
    BrokerV1Request = 82,
    BrokerV1Response = 83,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IpcFrameHeader {
    pub message_kind: IpcMessageKind,
    pub transaction: TransactionId,
    pub payload_len: u32,
}

// Kept only as a source-compatible name for legacy socket callers. The
// shared record codecs and their errors are owned outside the IPC module.
pub use crate::BinaryCodecError as IpcCodecError;
