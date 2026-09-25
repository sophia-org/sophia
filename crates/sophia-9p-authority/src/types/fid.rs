//! 9P File Identifier (fid) and Quad-ID (qid) types.

/// A 32-bit Plan 9 File Identifier (FID) chosen by the client.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Fid(pub u32);

impl Fid {
    pub const NOFID: Self = Self(u32::MAX);

    pub const fn new(val: u32) -> Self {
        Self(val)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Plan 9 13-byte unique file identification.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Qid {
    /// Type of file (directory, append-only, normal, etc.).
    pub qtype: u8,
    /// Version number of the file (incremented on modification).
    pub version: u32,
    /// Unique 64-bit path identifier on the server.
    pub path: u64,
}

impl Qid {
    pub const QTDIR: u8 = 0x80;
    pub const QTAPPEND: u8 = 0x40;
    pub const QTEXCL: u8 = 0x20;
    pub const QTMOUNT: u8 = 0x10;
    pub const QTAUTH: u8 = 0x08;
    pub const QTTMP: u8 = 0x04;
    pub const QTSYMLINK: u8 = 0x02;
    pub const QTFILE: u8 = 0x00;

    pub const fn new(qtype: u8, version: u32, path: u64) -> Self {
        Self {
            qtype,
            version,
            path,
        }
    }

    pub const fn is_dir(self) -> bool {
        (self.qtype & Self::QTDIR) != 0
    }
}

/// 9P open modes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum OpenMode {
    Read = 0,
    Write = 1,
    ReadWrite = 2,
    Execute = 3,
    Truncate = 0x10,
}

impl OpenMode {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val & 0x03 {
            0 => Some(Self::Read),
            1 => Some(Self::Write),
            2 => Some(Self::ReadWrite),
            3 => Some(Self::Execute),
            _ => None,
        }
    }
}
