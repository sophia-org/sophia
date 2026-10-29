//! Explicit, untimed observer delivery. This is not a public wire protocol.
mod stream;
pub use stream::write_frame;
#[cfg(feature = "native-session")]
mod owner;
#[cfg(feature = "native-session")]
pub(crate) use owner::{CaptureLaunch, CaptureStore};

use sophia_protocol::{OutputId, output_capture::OutputCaptureFrameInfo};
use std::io;

const MAGIC: &[u8; 8] = b"SCAP0001";
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

struct Frame {
    request: u64,
    info: OutputCaptureFrameInfo,
    pixels: Vec<u8>,
}

#[cfg(test)]
#[path = "../../tests/support/pixel_delivery_stream.rs"]
mod tests;
