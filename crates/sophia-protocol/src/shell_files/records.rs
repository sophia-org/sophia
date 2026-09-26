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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ShellFileKind {
    Limits = 1,
    Outputs = 2,
    Negotiated = 16,
    Refused = 17,
    Submitted = 18,
    ObjectPublished = 19,
    AllocationResult = 32,
    Negotiate = 256,
    AllocationRequest = 257,
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
    Records(crate::IpcCodecError),
    Identity,
    Value,
}
