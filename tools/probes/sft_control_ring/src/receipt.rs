//! One owner-written receipt slot. There is no peer-written consumer head.
use crate::{
    capture::{HEADER, Identity},
    custody::{Outcome, Receipt, Refusal},
};
use rustix::fs::{SealFlags, fcntl_get_seals};
use std::{fs::File, io, os::unix::fs::FileExt};

pub const BYTES: usize = HEADER + 16;

impl Receipt {
    pub fn encode(self) -> [u8; BYTES] {
        let mut bytes = [0; BYTES];
        bytes[..HEADER].copy_from_slice(&self.identity.header());
        let (kind, ack): (u64, u64) = match self.outcome {
            Outcome::Accepted { applied_ack } => (0, applied_ack),
            Outcome::Refused(error) => (
                match error {
                    Refusal::Again => 1,
                    Refusal::Invalid => 2,
                    Refusal::Stale => 3,
                    Refusal::Busy => 4,
                    Refusal::Already => 5,
                },
                0,
            ),
        };
        bytes[HEADER..HEADER + 8].copy_from_slice(&kind.to_le_bytes());
        bytes[HEADER + 8..].copy_from_slice(&ack.to_le_bytes());
        bytes
    }
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        let bytes: &[u8; BYTES] = bytes.try_into().map_err(|_| invalid())?;
        let identity = Identity::decode(bytes[..HEADER].try_into().expect("header size"))?;
        let kind = u64::from_le_bytes(bytes[HEADER..HEADER + 8].try_into().expect("word size"));
        let ack = u64::from_le_bytes(bytes[HEADER + 8..].try_into().expect("word size"));
        let outcome = match kind {
            0 if ack == identity.ack => Outcome::Accepted { applied_ack: ack },
            1..=5 if ack == 0 => Outcome::Refused(match kind {
                1 => Refusal::Again,
                2 => Refusal::Invalid,
                3 => Refusal::Stale,
                4 => Refusal::Busy,
                _ => Refusal::Already,
            }),
            _ => return Err(invalid()),
        };
        Ok(Self { identity, outcome })
    }
}

pub struct Source(File);
impl Source {
    pub fn admit(file: File) -> io::Result<Self> {
        if !fcntl_get_seals(&file)?.contains(SealFlags::GROW | SealFlags::SHRINK)
            || file.metadata()?.len() != BYTES as u64
        {
            return Err(invalid());
        }
        Ok(Self(file))
    }
    /// One private pread; no retry can stitch two different publications together.
    pub fn read(&self) -> io::Result<Receipt> {
        let mut bytes = [0; BYTES];
        if self.0.read_at(&mut bytes, 0)? != BYTES {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "short receipt",
            ));
        }
        Receipt::decode(&bytes)
    }
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid bounded receipt")
}
