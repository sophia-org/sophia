//! Passive protocol records: identifiers, error numbers, the bounds a server
//! enforces, decoded requests and the replies a connection sends. Nothing here
//! decides anything; the rules live in [`crate::connection`].

/// A request tag, chosen by the client to match its reply.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Tag(pub u16);

impl Tag {
    /// The tag a version request should carry. On any other request it is a
    /// protocol violation.
    pub const NOTAG: Self = Self(u16::MAX);
}

/// A client-chosen handle on a server node, scoped to one connection.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Fid(pub u32);

impl Fid {
    /// No fid: the only valid authentication fid, since no export authenticates
    /// through the protocol.
    pub const NOFID: Self = Self(u32::MAX);
}

/// What a qid names. Only the two node kinds an export serves exist here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QidKind {
    Directory,
    File,
}

impl QidKind {
    /// The type byte on the wire.
    pub const fn wire(self) -> u8 {
        match self {
            Self::Directory => 0x80,
            Self::File => 0x00,
        }
    }
}

/// The server's identity for a node. `path` must never name two nodes, across
/// epochs as well, so a stale identity cannot pass for a fresh one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Qid {
    pub kind: QidKind,
    pub version: u32,
    pub path: u64,
}

/// A Linux error number, as `Rlerror` carries it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Errno(pub u32);

impl Errno {
    pub const EPERM: Self = Self(1);
    pub const ENOENT: Self = Self(2);
    pub const EIO: Self = Self(5);
    pub const EBADF: Self = Self(9);
    pub const EAGAIN: Self = Self(11);
    pub const EACCES: Self = Self(13);
    pub const ENOTDIR: Self = Self(20);
    pub const EISDIR: Self = Self(21);
    pub const EINVAL: Self = Self(22);
    pub const EMFILE: Self = Self(24);
    pub const ENOSPC: Self = Self(28);
    pub const ENOSYS: Self = Self(38);
    pub const EPROTO: Self = Self(71);
    pub const EOPNOTSUPP: Self = Self(95);
    pub const ESTALE: Self = Self(116);
}

/// `Tlopen` flags, in the generic Linux values 9P2000.L carries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpenFlags(pub u32);

/// The access an open requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenAccess {
    Read,
    Write,
    ReadWrite,
}

impl OpenAccess {
    pub const fn reads(self) -> bool {
        matches!(self, Self::Read | Self::ReadWrite)
    }

    pub const fn writes(self) -> bool {
        matches!(self, Self::Write | Self::ReadWrite)
    }
}

impl OpenFlags {
    pub const ACCESS_MASK: u32 = 0o3;
    pub const TRUNCATE: u32 = 0o1000;
    pub const APPEND: u32 = 0o2000;
    pub const DIRECTORY: u32 = 0o200000;
    /// Bits a client may set that change nothing a synthetic node serves.
    pub const IGNORED: u32 = 0o400 // O_NOCTTY
        | 0o4000 // O_NONBLOCK: the protocol, not the flag, decides blocking
        | 0o100000 // O_LARGEFILE
        | 0o2000000; // O_CLOEXEC
    /// Every bit an open may carry; any other is refused.
    pub const ACCEPTED: u32 =
        Self::ACCESS_MASK | Self::TRUNCATE | Self::APPEND | Self::DIRECTORY | Self::IGNORED;

    /// The requested access, or `None` for the invalid access mode 3.
    pub const fn access(self) -> Option<OpenAccess> {
        match self.0 & Self::ACCESS_MASK {
            0 => Some(OpenAccess::Read),
            1 => Some(OpenAccess::Write),
            2 => Some(OpenAccess::ReadWrite),
            _ => None,
        }
    }

    pub const fn truncate(self) -> bool {
        self.0 & Self::TRUNCATE != 0
    }

    pub const fn append(self) -> bool {
        self.0 & Self::APPEND != 0
    }

    pub const fn directory(self) -> bool {
        self.0 & Self::DIRECTORY != 0
    }

    /// Bits outside [`Self::ACCEPTED`].
    pub const fn unknown(self) -> u32 {
        self.0 & !Self::ACCEPTED
    }
}

/// The attributes `Rgetattr` reports. Fields the server does not know are not
/// reported valid and are sent as zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Attr {
    pub valid: u64,
    pub qid: Qid,
    /// File type and permission bits, as `st_mode`.
    pub mode: u32,
    pub nlink: u64,
    pub size: u64,
}

impl Attr {
    pub const MODE: u64 = 0x1;
    pub const NLINK: u64 = 0x2;
    pub const INO: u64 = 0x100;
    pub const SIZE: u64 = 0x200;
}

/// Every bound a connection and its driver enforce on what peers can make
/// them hold. Built only through [`Limits::new`], which refuses a combination
/// under which a bound could not be kept.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    max_msize: u32,
    min_msize: u32,
    max_pending: usize,
    max_fids: usize,
    max_unsent: usize,
    max_connections: usize,
}

/// Why a set of limits was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitsError {
    /// The minimum message size cannot carry a full walk request or an error
    /// reply, or exceeds the maximum.
    MessageSize,
    /// The maximum message size is larger than this core will buffer.
    MessageSizeCeiling,
    /// Unsent output must hold at least one reply of the largest message size,
    /// or a reserved reply could never be admitted.
    Unsent,
    /// A count bound of zero admits nothing.
    Zero,
}

impl Limits {
    /// The largest walk the protocol allows.
    pub const MAX_WALK: usize = 16;
    /// The smallest message size accepted as a bound: room for the headers of
    /// every reply this core sends and a useful payload.
    pub const MSIZE_FLOOR: u32 = 512;
    /// The largest message size accepted as a bound.
    pub const MSIZE_CEILING: u32 = 1 << 24;

    pub fn new(
        max_msize: u32,
        min_msize: u32,
        max_pending: usize,
        max_fids: usize,
        max_unsent: usize,
        max_connections: usize,
    ) -> Result<Self, LimitsError> {
        if max_msize > Self::MSIZE_CEILING {
            return Err(LimitsError::MessageSizeCeiling);
        }
        if min_msize < Self::MSIZE_FLOOR || min_msize > max_msize {
            return Err(LimitsError::MessageSize);
        }
        if max_unsent < max_msize as usize {
            return Err(LimitsError::Unsent);
        }
        if max_pending == 0 || max_fids == 0 || max_connections == 0 {
            return Err(LimitsError::Zero);
        }
        Ok(Self {
            max_msize,
            min_msize,
            max_pending,
            max_fids,
            max_unsent,
            max_connections,
        })
    }

    /// The largest message size negotiated, and the largest frame accepted
    /// before negotiation.
    pub const fn max_msize(&self) -> u32 {
        self.max_msize
    }

    /// A client offering less is refused.
    pub const fn min_msize(&self) -> u32 {
        self.min_msize
    }

    /// Requests waiting on an export at once. Past it a request that would
    /// wait is answered `EAGAIN`, so input never stalls behind waiting reads
    /// and a flush can always arrive.
    pub const fn max_pending(&self) -> usize {
        self.max_pending
    }

    /// Fids one connection may hold.
    pub const fn max_fids(&self) -> usize {
        self.max_fids
    }

    /// Reply bytes not yet written, counting the room reserved for replies
    /// not yet produced. A request whose largest reply does not fit waits,
    /// unprocessed, until the peer reads.
    pub const fn max_unsent(&self) -> usize {
        self.max_unsent
    }

    /// Connections one driver serves at once.
    pub const fn max_connections(&self) -> usize {
        self.max_connections
    }
}

impl Default for Limits {
    /// The C1 bounds: 64 KiB messages (the pinned Go client's default), 4 KiB
    /// minimum, 32 waiting requests, 256 fids, four messages of unsent output
    /// and 16 connections.
    fn default() -> Self {
        Self {
            max_msize: 64 * 1024,
            min_msize: 4096,
            max_pending: 32,
            max_fids: 256,
            max_unsent: 4 * 64 * 1024,
            max_connections: 16,
        }
    }
}

/// A decoded request. Byte strings borrow the frame they were read from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Request<'frame> {
    Version {
        msize: u32,
        version: &'frame [u8],
    },
    Attach {
        fid: Fid,
        afid: Fid,
        uname: &'frame [u8],
        aname: &'frame [u8],
        n_uname: u32,
    },
    Flush {
        old: Tag,
    },
    Walk {
        fid: Fid,
        newfid: Fid,
        names: Vec<&'frame [u8]>,
    },
    Lopen {
        fid: Fid,
        flags: OpenFlags,
    },
    Read {
        fid: Fid,
        offset: u64,
        count: u32,
    },
    Write {
        fid: Fid,
        offset: u64,
        data: &'frame [u8],
    },
    Clunk {
        fid: Fid,
    },
    Remove {
        fid: Fid,
    },
    Getattr {
        fid: Fid,
        mask: u64,
    },
    /// A well-framed request this server does not perform. It is answered
    /// with the error and changes nothing.
    Refused {
        kind: u8,
        errno: Errno,
    },
}

/// A reply a connection sends.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Reply {
    Version { msize: u32, version: &'static [u8] },
    Lerror(Errno),
    Attach(Qid),
    Flush,
    Walk(Vec<Qid>),
    Lopen { qid: Qid, iounit: u32 },
    Read(Vec<u8>),
    Write(u32),
    Clunk,
    Getattr(Attr),
}
