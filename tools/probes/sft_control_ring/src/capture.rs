//! Pread-only capture. The producer alone owns a writable mapping.
use rustix::fs::{SealFlags, fcntl_get_seals};
use sophia_shell_protocol::shell_files::SHELL_FILE_MAX_TRANSACTION_BYTES;
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
    pub fn capture(&self, expected_sequence: u64) -> io::Result<Captured> {
        capture_with(expected_sequence, |buf, offset| self.0.read_at(buf, offset))
    }
}

/// One bounded header read and one payload read; short/error reads are definitive.
/// The seam allows deterministic EINTR/EIO/mutation tests without retrying bytes.
pub fn capture_with(
    expected_sequence: u64,
    mut read: impl FnMut(&mut [u8], u64) -> io::Result<usize>,
) -> io::Result<Captured> {
    let mut header = [0; HEADER];
    exact(&mut read, &mut header, 0)?;
    let identity = Identity::decode(&header)?;
    if identity.sequence != expected_sequence || expected_sequence == u64::MAX {
        return Err(invalid("unexpected or exhausted transport sequence"));
    }
    let mut bytes = vec![0; identity.bytes as usize];
    exact(&mut read, &mut bytes, HEADER as u64)?;
    Ok(Captured { identity, bytes })
}

fn exact(
    read: &mut impl FnMut(&mut [u8], u64) -> io::Result<usize>,
    bytes: &mut [u8],
    offset: u64,
) -> io::Result<()> {
    if read(bytes, offset)? != bytes.len() {
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
