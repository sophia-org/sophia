pub const WM_FILE_API_VERSION: u16 = 1;
pub const WM_FILE_HEADER_BYTES: usize = 32;
pub const WM_FILE_MAX_BYTES: usize = 1024 * 1024;
pub const WM_FILE_MAX_SECTIONS: usize = 32;
pub const WM_FILE_SECTION_HEADER_BYTES: usize = 16;
pub const WM_FILE_SUBMIT_BYTES: usize = 24;
pub const WM_FILE_ACK_BYTES: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum WmFileKind {
    Limits = 1,
    Snapshot = 2,
    Negotiated = 16,
    Submitted = 17,
    ProfilePrepare = 18,
    ProfileActivate = 19,
    ProfileRollback = 20,
    ConfigurationOutcome = 21,
    Cycle = 22,
    ProjectionOutcome = 23,
    SessionOperationOutcome = 24,
    PresentationReceipt = 25,
    Negotiate = 256,
    ProfilePrepared = 257,
    ProfileActive = 258,
    ProfileRolledBack = 259,
    Configuration = 260,
    Dirty = 261,
    Projection = 262,
    SessionOperation = 263,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmFileClass {
    Object,
    Event,
    Candidate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmFileHeader {
    pub kind: WmFileKind,
    pub connection_epoch: u64,
    pub submission_id: u64,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmFileRecord<'a> {
    pub header: WmFileHeader,
    /// Raw bounded bytes, not a validated semantic payload.
    pub body: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmFileSection<'a> {
    pub kind: u16,
    pub count: u32,
    /// Context-specific row size, kind and aggregate limits are validated by
    /// the neutral snapshot/projection codec before exposing domain records.
    pub bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmFileSubmit {
    pub connection_epoch: u64,
    pub submission_id: u64,
    pub candidate_bytes: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmFileAck {
    pub connection_epoch: u64,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmFileCodecError {
    Length,
    Version,
    Kind,
    Class,
    Identity,
    Reserved,
    Sections,
}

pub const WM_FILE_SNAPSHOT_PREFIX_BYTES: usize = 32;
pub const WM_FILE_PROJECTION_PREFIX_BYTES: usize = 40;
pub const WM_FILE_CONFIGURATION_PREFIX_BYTES: usize = 48;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmFileSnapshot {
    pub transaction: crate::TransactionId,
    pub snapshot: crate::PolicyDecodedSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmFileConfiguration {
    pub transaction: crate::TransactionId,
    pub configuration: crate::PolicyConfiguration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WmFilePayloadError {
    Envelope(WmFileCodecError),
    Records(crate::IpcCodecError),
    Identity,
    Capabilities { missing: u64 },
    Value,
}

pub const WM_FILE_CYCLE_PREFIX_BYTES: usize = 48;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmFileCycle {
    pub snapshot_transaction: crate::TransactionId,
    pub request_transaction: crate::TransactionId,
    pub request: crate::PolicyProjectionRequest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmFileSessionOperation {
    pub transaction: crate::TransactionId,
    pub request: crate::PolicySessionOperationRequest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmFileConfigurationOutcome {
    pub transaction: crate::TransactionId,
    pub generation: u64,
    pub outcome: crate::PolicyProjectionOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmFileProjectionOutcome {
    pub transaction: crate::TransactionId,
    pub request_id: u64,
    pub scene_generation: u64,
    pub outcome: crate::PolicyProjectionOutcome,
    pub expect_session_operation: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmFileSessionOperationOutcome {
    pub transaction: crate::TransactionId,
    pub outcome: crate::PolicySessionOperationOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmFilePresentationReceipt {
    pub transaction: crate::TransactionId,
    pub receipt: crate::PolicyPresentationReceipt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmFileSubmitted {
    pub submission_id: u64,
    pub candidate_kind: WmFileKind,
}
