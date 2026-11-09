//! Exact transport receipt carried directly by the owner-to-peer pipe.
use crate::{
    capture::{HEADER, Identity},
    custody::{Outcome, Receipt},
};
use std::io;

pub const BYTES: usize = HEADER + 16;

impl Receipt {
    pub fn encode(self) -> [u8; BYTES] {
        let mut bytes = [0; BYTES];
        bytes[..HEADER].copy_from_slice(&self.identity.header());
        let (kind, ack): (u64, u64) = match self.outcome {
            Outcome::Accepted { applied_ack } => (0, applied_ack),
            Outcome::Refused(error) => (u64::from(error.0), 0),
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
            1..=4095 if ack == 0 => Outcome::Refused(sophia_9p::Errno(kind as u32)),
            _ => return Err(invalid()),
        };
        Ok(Self { identity, outcome })
    }
}

impl crate::wake::Reader {
    /// One receipt per read; short/error input closes, never stitches messages.
    pub fn receipt(&self) -> io::Result<Option<Receipt>> {
        let mut bytes = [0; BYTES];
        let count = match rustix::io::read(self, &mut bytes) {
            Err(rustix::io::Errno::AGAIN) => return Ok(None),
            result => result?,
        };
        if count != BYTES {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "short receipt",
            ));
        }
        Receipt::decode(&bytes).map(Some)
    }
}
impl crate::wake::Writer {
    /// Full/short/error is not delivery: close without undoing export custody.
    pub fn receipt(&self, receipt: Receipt) -> io::Result<()> {
        if rustix::io::write(self, &receipt.encode())? != BYTES {
            return Err(io::Error::other("short receipt write"));
        }
        Ok(())
    }
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid bounded receipt")
}
