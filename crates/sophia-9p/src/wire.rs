//! The 9P2000.L wire codec: frame lengths, request decoding and reply
//! encoding. Every length a peer supplies is checked against the bytes that
//! are present, and no value is ever narrowed silently.

use crate::records::{Attr, Errno, Fid, OpenFlags, Qid, Reply, Request, Tag};

/// size[4] type[1] tag[2].
pub const HEADER: usize = 7;
/// What an `Rread` adds to its data: size[4] type[1] tag[2] count[4].
pub const READ_OVERHEAD: u32 = 11;
/// The I/O header Linux reserves in an `iounit`: size[4] type[1] tag[2]
/// fid[4] offset[8] count[4], plus one.
pub const IO_HEADER: u32 = 24;

/// Message types, in the numbering of the 9P2000.L specification.
pub mod kind {
    pub const RLERROR: u8 = 7;
    pub const TSTATFS: u8 = 8;
    pub const TLOPEN: u8 = 12;
    pub const RLOPEN: u8 = 13;
    pub const TLCREATE: u8 = 14;
    pub const TSYMLINK: u8 = 16;
    pub const TMKNOD: u8 = 18;
    pub const TRENAME: u8 = 20;
    pub const TREADLINK: u8 = 22;
    pub const TGETATTR: u8 = 24;
    pub const RGETATTR: u8 = 25;
    pub const TSETATTR: u8 = 26;
    pub const TXATTRWALK: u8 = 30;
    pub const TXATTRCREATE: u8 = 32;
    pub const TREADDIR: u8 = 40;
    pub const TFSYNC: u8 = 50;
    pub const TLOCK: u8 = 52;
    pub const TGETLOCK: u8 = 54;
    pub const TLINK: u8 = 70;
    pub const TMKDIR: u8 = 72;
    pub const TRENAMEAT: u8 = 74;
    pub const TUNLINKAT: u8 = 76;
    pub const TVERSION: u8 = 100;
    pub const RVERSION: u8 = 101;
    pub const TAUTH: u8 = 102;
    pub const TATTACH: u8 = 104;
    pub const RATTACH: u8 = 105;
    pub const TFLUSH: u8 = 108;
    pub const RFLUSH: u8 = 109;
    pub const TWALK: u8 = 110;
    pub const RWALK: u8 = 111;
    pub const TREAD: u8 = 116;
    pub const RREAD: u8 = 117;
    pub const TWRITE: u8 = 118;
    pub const RWRITE: u8 = 119;
    pub const TCLUNK: u8 = 120;
    pub const RCLUNK: u8 = 121;
    pub const TREMOVE: u8 = 122;
}

/// A frame whose size field cannot be honoured. The stream cannot be trusted
/// past it, so the connection ends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameError {
    /// Smaller than a header.
    Short(u32),
    /// Larger than the message size in force.
    Oversize { size: u32, msize: u32 },
}

/// A frame whose body does not have its type's shape. The frame itself was
/// well delimited, so it is answered and the stream continues.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Malformed {
    pub kind: u8,
    pub tag: Tag,
    pub errno: Errno,
}

/// A reply that cannot be framed: the caller let it exceed what a frame holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Unframeable;

/// The length of the frame that begins with `prefix`, if `msize` admits it.
pub fn frame_length(prefix: [u8; 4], msize: u32) -> Result<usize, FrameError> {
    let size = u32::from_le_bytes(prefix);
    if (size as usize) < HEADER {
        return Err(FrameError::Short(size));
    }
    if size > msize {
        return Err(FrameError::Oversize { size, msize });
    }
    Ok(size as usize)
}

/// The type and tag of a frame at least [`HEADER`] bytes long.
pub fn header(frame: &[u8]) -> Option<(u8, Tag)> {
    let bytes = frame.get(..HEADER)?;
    Some((bytes[4], Tag(u16::from_le_bytes([bytes[5], bytes[6]]))))
}

/// Decodes one complete frame, exactly as long as its size field says.
pub fn decode(frame: &[u8]) -> Result<(Tag, Request<'_>), Malformed> {
    let Some((kind, tag)) = header(frame) else {
        return Err(Malformed {
            kind: 0,
            tag: Tag::NOTAG,
            errno: Errno::EPROTO,
        });
    };
    let malformed = |errno| Malformed { kind, tag, errno };
    let mut body = Fields {
        bytes: &frame[HEADER..],
    };
    let request = body
        .request(kind)
        .ok_or(malformed(Errno::EPROTO))?
        .map_err(malformed)?;
    if !body.bytes.is_empty() {
        return Err(malformed(Errno::EPROTO));
    }
    Ok((tag, request))
}

/// Appends the frame for `reply` to `out`.
pub fn encode(tag: Tag, reply: &Reply, out: &mut Vec<u8>) -> Result<(), Unframeable> {
    let start = out.len();
    out.extend_from_slice(&[0; 4]);
    let kind = match reply {
        Reply::Version { msize, version } => {
            out.extend_from_slice(&msize.to_le_bytes());
            put_string(out, version)?;
            kind::RVERSION
        }
        Reply::Lerror(errno) => {
            out.extend_from_slice(&errno.0.to_le_bytes());
            kind::RLERROR
        }
        Reply::Attach(qid) => {
            put_qid(out, qid);
            kind::RATTACH
        }
        Reply::Flush => kind::RFLUSH,
        Reply::Walk(qids) => {
            let count = u16::try_from(qids.len()).map_err(|_| Unframeable)?;
            out.extend_from_slice(&count.to_le_bytes());
            for qid in qids {
                put_qid(out, qid);
            }
            kind::RWALK
        }
        Reply::Lopen { qid, iounit } => {
            put_qid(out, qid);
            out.extend_from_slice(&iounit.to_le_bytes());
            kind::RLOPEN
        }
        Reply::Read(data) => {
            let count = u32::try_from(data.len()).map_err(|_| Unframeable)?;
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(data);
            kind::RREAD
        }
        Reply::Write(count) => {
            out.extend_from_slice(&count.to_le_bytes());
            kind::RWRITE
        }
        Reply::Clunk => kind::RCLUNK,
        Reply::Getattr(attr) => {
            put_attr(out, attr);
            kind::RGETATTR
        }
    };
    // The type and tag go between the size and the body.
    out.splice(
        start + 4..start + 4,
        [kind].into_iter().chain(tag.0.to_le_bytes()),
    );
    let size = u32::try_from(out.len() - start).map_err(|_| Unframeable)?;
    out[start..start + 4].copy_from_slice(&size.to_le_bytes());
    Ok(())
}

fn put_string(out: &mut Vec<u8>, value: &[u8]) -> Result<(), Unframeable> {
    let length = u16::try_from(value.len()).map_err(|_| Unframeable)?;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(value);
    Ok(())
}

fn put_qid(out: &mut Vec<u8>, qid: &Qid) {
    out.push(qid.kind.wire());
    out.extend_from_slice(&qid.version.to_le_bytes());
    out.extend_from_slice(&qid.path.to_le_bytes());
}

/// valid[8] qid[13] mode[4] uid[4] gid[4] nlink[8] rdev[8] size[8]
/// blksize[8] blocks[8], four timestamps of sec[8] nsec[8], gen[8] and
/// data_version[8]. Only the fields `valid` names are meaningful.
fn put_attr(out: &mut Vec<u8>, attr: &Attr) {
    out.extend_from_slice(&attr.valid.to_le_bytes());
    put_qid(out, &attr.qid);
    out.extend_from_slice(&attr.mode.to_le_bytes());
    out.extend_from_slice(&[0; 8]); // uid, gid
    out.extend_from_slice(&attr.nlink.to_le_bytes());
    out.extend_from_slice(&[0; 8]); // rdev
    out.extend_from_slice(&attr.size.to_le_bytes());
    out.extend_from_slice(&[0; 8 * 2]); // blksize, blocks
    out.extend_from_slice(&[0; 8 * 8]); // atime, mtime, ctime, btime
    out.extend_from_slice(&[0; 8 * 2]); // gen, data_version
}

/// A cursor over a body. Each read either consumes exactly its field or
/// reports that the body is too short.
struct Fields<'frame> {
    bytes: &'frame [u8],
}

impl<'frame> Fields<'frame> {
    fn take(&mut self, count: usize) -> Option<&'frame [u8]> {
        if self.bytes.len() < count {
            return None;
        }
        let (head, rest) = self.bytes.split_at(count);
        self.bytes = rest;
        Some(head)
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    fn u16(&mut self) -> Option<u16> {
        self.array().map(u16::from_le_bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        self.array().map(u32::from_le_bytes)
    }

    fn u64(&mut self) -> Option<u64> {
        self.array().map(u64::from_le_bytes)
    }

    fn fid(&mut self) -> Option<Fid> {
        self.u32().map(Fid)
    }

    fn string(&mut self) -> Option<&'frame [u8]> {
        let length = self.u16()?;
        self.take(usize::from(length))
    }

    /// `None` when the body is too short; `Some(Err)` when it is complete but
    /// not acceptable as it stands.
    fn request(&mut self, kind: u8) -> Option<Result<Request<'frame>, Errno>> {
        let request = match kind {
            kind::TVERSION => Request::Version {
                msize: self.u32()?,
                version: self.string()?,
            },
            kind::TATTACH => Request::Attach {
                fid: self.fid()?,
                afid: self.fid()?,
                uname: self.string()?,
                aname: self.string()?,
                n_uname: self.u32()?,
            },
            kind::TFLUSH => Request::Flush {
                old: Tag(self.u16()?),
            },
            kind::TWALK => {
                let fid = self.fid()?;
                let newfid = self.fid()?;
                let count = usize::from(self.u16()?);
                if count > crate::records::Limits::MAX_WALK {
                    // Complete but unacceptable; the names are not read.
                    self.bytes = &[];
                    return Some(Err(Errno::EINVAL));
                }
                let mut names = Vec::with_capacity(count);
                for _ in 0..count {
                    names.push(self.string()?);
                }
                Request::Walk { fid, newfid, names }
            }
            kind::TLOPEN => Request::Lopen {
                fid: self.fid()?,
                flags: OpenFlags(self.u32()?),
            },
            kind::TREAD => Request::Read {
                fid: self.fid()?,
                offset: self.u64()?,
                count: self.u32()?,
            },
            kind::TWRITE => {
                let fid = self.fid()?;
                let offset = self.u64()?;
                let count = self.u32()?;
                let data = self.take(usize::try_from(count).ok()?)?;
                Request::Write { fid, offset, data }
            }
            kind::TCLUNK => Request::Clunk { fid: self.fid()? },
            kind::TREMOVE => Request::Remove { fid: self.fid()? },
            kind::TGETATTR => Request::Getattr {
                fid: self.fid()?,
                mask: self.u64()?,
            },
            kind::TAUTH
            | kind::TSTATFS
            | kind::TLCREATE
            | kind::TSYMLINK
            | kind::TMKNOD
            | kind::TRENAME
            | kind::TREADLINK
            | kind::TSETATTR
            | kind::TXATTRWALK
            | kind::TXATTRCREATE
            | kind::TREADDIR
            | kind::TFSYNC
            | kind::TLOCK
            | kind::TGETLOCK
            | kind::TLINK
            | kind::TMKDIR
            | kind::TRENAMEAT
            | kind::TUNLINKAT => self.refuse(kind, Errno::EOPNOTSUPP),
            _ => self.refuse(kind, Errno::ENOSYS),
        };
        Some(Ok(request))
    }

    /// A refused request's body is not interpreted, so none of it is left over.
    fn refuse(&mut self, kind: u8, errno: Errno) -> Request<'frame> {
        self.bytes = &[];
        Request::Refused { kind, errno }
    }
}
