pub const SHELL_FILE_API_VERSION: u16 = 1;
pub const SHELL_FILE_HEADER_BYTES: usize = 32;
pub const SHELL_FILE_MAX_TRANSACTION_BYTES: usize = 65_536;
pub const SHELL_FILE_SUBMIT_BYTES: usize = 24;
pub const SHELL_FILE_ACK_BYTES: usize = 16;
pub const SHELL_FILE_MAX_JOURNAL_RECORDS: u16 = 256;
pub const SHELL_FILE_TERMINAL_RESERVE_RECORDS: u16 = 64;
pub const SHELL_FILE_MAX_JOURNAL_BYTES: u32 = 1_048_576;
pub const SHELL_FILE_ASSEMBLY_TIMEOUT_MILLIS: u32 = 12_000;
pub const SHELL_FILE_ACK_PROGRESS_TIMEOUT_MILLIS: u32 = 2_000;
pub const SHELL_FILE_MAX_OBJECT_BYTES: usize = 4_194_304;
/// The `outputs` object cap (docs/sophia-shell-files.md, snapshot objects).
pub const SHELL_FILE_OUTPUTS_MAX_BYTES: usize = 1024;
/// The `indicators` object cap (docs/sophia-shell-files.md, t252 B5 draft
/// table): the whole indicator snapshot, well under the shared 4 MiB object
/// cap.
pub const SHELL_FILE_INDICATORS_MAX_BYTES: usize = 32_768;
/// The number of upload slots a connection may address in a `ResourceBegin`.
pub const SHELL_FILE_MAX_UPLOAD_SLOTS: u16 = 4;
/// One complete `Candidate` record, header included: the content limits'
/// `max_candidate_bytes` prototype cap (docs/sophia-shell-files.md).
pub const SHELL_FILE_MAX_CANDIDATE_BYTES: usize = 8192;
/// Descriptor-role objects have separate retained-snapshot bounds.
pub const SHELL_FILE_DESCRIPTORS_MAX_BYTES: usize = 4096;
pub const SHELL_FILE_TABS_MAX_BYTES: usize = 1_048_576;
pub const SHELL_FILE_SHORTCUTS_MAX_BYTES: usize = 131_072;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ShellFileKind {
    Limits = 1,
    Outputs = 2,
    Catalog = 3,
    Indicators = 4,
    Descriptors = 5,
    Tabs = 6,
    Shortcuts = 7,
    Negotiated = 16,
    Refused = 17,
    Submitted = 18,
    ObjectPublished = 19,
    AllocationResult = 32,
    ResourceStatus = 33,
    ResourceReleased = 34,
    CandidateOutcome = 35,
    FramePermit = 36,
    Action = 37,
    NativeOpening = 38,
    NativeFocus = 39,
    NativeFocusRevoked = 40,
    NativeInput = 41,
    NativeActivationOutcome = 42,
    NativeClosed = 43,
    CatalogActivationOutcome = 44,
    IndicatorActivationOutcome = 45,
    DescriptorOutcome = 46,
    DescriptorActivation = 47,
    ReferenceRequest = 48,
    ReferenceOutcome = 49,
    LauncherRequest = 50,
    LauncherOutcome = 51,
    LauncherActivation = 52,
    LaunchOutcome = 53,
    Negotiate = 256,
    AllocationRequest = 257,
    ResourceBegin = 258,
    ResourceEnd = 259,
    ResourceCancel = 260,
    ResourceRetire = 261,
    Candidate = 262,
    FrameDemand = 263,
    FrameDemandCancel = 264,
    ActionAck = 265,
    NativeAllocationRequest = 266,
    NativeCandidate = 267,
    NativeInputAck = 268,
    NativeActivate = 269,
    CatalogCandidate = 270,
    CatalogActivate = 271,
    IndicatorActivate = 272,
    DescriptorCandidate = 273,
    DescriptorActivationAck = 274,
    TabsCandidate = 275,
    ReferenceCandidate = 276,
    LauncherCandidate = 277,
    LauncherActivationAck = 278,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellFileClass {
    Object,
    Event,
    Candidate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellFileHeader {
    pub kind: ShellFileKind,
    pub connection_epoch: u64,
    pub submission_id: u64,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellFileRecord<'a> {
    pub header: ShellFileHeader,
    pub body: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellFileSubmit {
    pub connection_epoch: u64,
    pub submission_id: u64,
    pub candidate_bytes: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellFileAck {
    pub connection_epoch: u64,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellFileCodecError {
    Length,
    Version,
    Kind,
    Class,
    Identity,
    Reserved,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShellFilePayloadError {
    Envelope(ShellFileCodecError),
    Records(crate::shell::encoding::ValueError),
    Identity,
    Value,
}
