//! 9P2000 message representation for requests (T) and responses (R).

use super::fid::{Fid, Qid};

/// 16-bit message tag identifying matching request and response pairs.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Tag(pub u16);

impl Tag {
    pub const NOTAG: Self = Self(u16::MAX);

    pub const fn new(val: u16) -> Self {
        Self(val)
    }

    pub const fn raw(self) -> u16 {
        self.0
    }
}

/// Incoming client requests (T-messages).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TMessage {
    Version {
        msize: u32,
        version: String,
    },
    Auth {
        afid: Fid,
        uname: String,
        aname: String,
    },
    Attach {
        fid: Fid,
        afid: Fid,
        uname: String,
        aname: String,
    },
    Flush {
        oldtag: Tag,
    },
    Walk {
        fid: Fid,
        newfid: Fid,
        wnames: Vec<String>,
    },
    Open {
        fid: Fid,
        mode: u8,
    },
    Create {
        fid: Fid,
        name: String,
        perm: u32,
        mode: u8,
    },
    Read {
        fid: Fid,
        offset: u64,
        count: u32,
    },
    Write {
        fid: Fid,
        offset: u64,
        data: Vec<u8>,
    },
    Clunk {
        fid: Fid,
    },
    Remove {
        fid: Fid,
    },
    Stat {
        fid: Fid,
    },
    Wstat {
        fid: Fid,
        stat_bytes: Vec<u8>,
    },
}

/// Outgoing server responses (R-messages).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RMessage {
    Version { msize: u32, version: String },
    Auth { aqid: Qid },
    Attach { qid: Qid },
    Error { ename: String },
    Flush,
    Walk { wqids: Vec<Qid> },
    Open { qid: Qid, iounit: u32 },
    Create { qid: Qid, iounit: u32 },
    Read { data: Vec<u8> },
    Write { count: u32 },
    Clunk,
    Remove,
    Stat { stat_bytes: Vec<u8> },
    Wstat,
}
