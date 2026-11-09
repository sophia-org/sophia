//! Pread-only capture. The producer alone owns a writable mapping.
use rustix::fs::{SealFlags, fcntl_get_seals};
use sophia_protocol::shell_files::SHELL_FILE_MAX_TRANSACTION_BYTES;
use std::fs::File;
use std::io;
use std::os::unix::fs::FileExt;

pub const HEADER: usize = 64;
pub const MAX_RECORD: usize = SHELL_FILE_MAX_TRANSACTION_BYTES;
pub const EXTENT: usize = HEADER + MAX_RECORD;
const MAGIC: u64 = 0x315153544653;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Identity {
    pub sequence: u64,
    pub epoch: u64,
    pub writer: u64,
    pub submission: u64,
    pub ack: u64,
    pub bytes: u64,
}

impl Identity {
    pub fn header(self) -> [u8; HEADER] {
        let words = [
            MAGIC,
            self.sequence,
            self.epoch,
            self.writer,
            self.submission,
            self.ack,
            self.bytes,
            0,
        ];
        let mut header = [0; HEADER];
        for (dst, word) in header.chunks_exact_mut(8).zip(words) {
            dst.copy_from_slice(&word.to_le_bytes());
        }
        header
    }
    pub(crate) fn decode(header: &[u8; HEADER]) -> io::Result<Self> {
        let word = |i: usize| {
            u64::from_le_bytes(
                header[i * 8..i * 8 + 8]
                    .try_into()
                    .expect("eight-byte word"),
            )
        };
        if word(0) != MAGIC
            || word(7) != 0
            || word(1) == 0
            || word(2) == 0
            || word(3) == 0
            || word(4) == 0
            || word(6) < 32
            || word(6) > MAX_RECORD as u64
        {
            return Err(invalid("invalid bounded request header"));
        }
        Ok(Self {
            sequence: word(1),
            epoch: word(2),
            writer: word(3),
            submission: word(4),
            ack: word(5),
            bytes: word(6),
        })
    }
}

#[derive(Debug)]
pub struct Captured {
    pub identity: Identity,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy)]
pub enum ReadPlan {
    HeaderThenPayload,
    FullExtent,
}

impl Captured {
    pub fn buffer() -> Self {
        Self {
            identity: Identity {
                sequence: 0,
                epoch: 0,
                writer: 0,
                submission: 0,
                ack: 0,
                bytes: 0,
            },
            bytes: Vec::with_capacity(EXTENT),
        }
    }
}

pub struct Source(File);
impl Source {
    pub fn admit(file: File) -> io::Result<Self> {
        let seals = fcntl_get_seals(&file)?;
        if !seals.contains(SealFlags::GROW | SealFlags::SHRINK)
            || file.metadata()?.len() != EXTENT as u64
        {
            return Err(invalid("source requires exact extent and size seals"));
        }
        Ok(Self(file))
    }
    pub fn scan(&self, last: u64, buffer: &mut Captured, plan: ReadPlan) -> io::Result<bool> {
        scan_with(last, buffer, plan, |buf, offset| {
            self.0.read_at(buf, offset)
        })
    }
    pub fn capture(&self, expected_sequence: u64) -> io::Result<Captured> {
        capture_with(expected_sequence, |buf, offset| self.0.read_at(buf, offset))
    }
}

/// Header/body capture with at most one local EINTR retry per read.
/// Short successful reads are never stitched; other errors close the carrier.
pub fn capture_with(
    expected_sequence: u64,
    mut read: impl FnMut(&mut [u8], u64) -> io::Result<usize>,
) -> io::Result<Captured> {
    let mut buffer = Captured::buffer();
    let last = expected_sequence
        .checked_sub(1)
        .ok_or_else(|| invalid("zero expected sequence"))?;
    if !scan_with(last, &mut buffer, ReadPlan::HeaderThenPayload, &mut read)? {
        return Err(invalid("expected a new publication"));
    }
    Ok(buffer)
}

/// False is idle (initial zero header or last processed sequence). Gaps close.
pub fn scan_with(
    last: u64,
    buffer: &mut Captured,
    plan: ReadPlan,
    mut read: impl FnMut(&mut [u8], u64) -> io::Result<usize>,
) -> io::Result<bool> {
    let mut header = [0; HEADER];
    match plan {
        ReadPlan::HeaderThenPayload => exact(&mut read, &mut header, 0)?,
        ReadPlan::FullExtent => {
            buffer.bytes.resize(EXTENT, 0);
            exact(&mut read, &mut buffer.bytes, 0)?;
            header.copy_from_slice(&buffer.bytes[..HEADER]);
        }
    }
    if last == 0 && header == [0; HEADER] {
        return Ok(false);
    }
    let identity = Identity::decode(&header)?;
    if identity.sequence == last {
        return Ok(false);
    }
    if identity.sequence == u64::MAX || last.checked_add(1) != Some(identity.sequence) {
        return Err(invalid("gap or exhausted transport sequence"));
    }
    let len = identity.bytes as usize;
    match plan {
        ReadPlan::HeaderThenPayload => {
            buffer.bytes.resize(len, 0);
            exact(&mut read, &mut buffer.bytes, HEADER as u64)?;
        }
        ReadPlan::FullExtent => {
            buffer.bytes.copy_within(HEADER..HEADER + len, 0);
            buffer.bytes.truncate(len);
        }
    }
    buffer.identity = identity;
    Ok(true)
}

fn exact(
    read: &mut impl FnMut(&mut [u8], u64) -> io::Result<usize>,
    bytes: &mut [u8],
    offset: u64,
) -> io::Result<()> {
    // One local retry after EINTR; never retry a short successful read.
    let count = match read(bytes, offset) {
        Err(error) if error.kind() == io::ErrorKind::Interrupted => read(bytes, offset)?,
        other => other?,
    };
    if count != bytes.len() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "short snapshot read",
        ));
    }
    Ok(())
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
