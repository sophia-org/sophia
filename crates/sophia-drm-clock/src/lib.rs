//! Safe, read-only adapter for DRM_IOCTL_CRTC_GET_SEQUENCE.
//!
//! This reads the kernel's full 64-bit counter and most recent first-pixel
//! timestamp. It neither waits for nor predicts a future vblank, and does not
//! consume page-flip events from the card's event queue.

use std::{io, os::fd::AsFd};

type RawSequence = drm_ffi::drm_crtc_get_sequence;
const GET_SEQUENCE: rustix::ioctl::Opcode =
    rustix::ioctl::opcode::read_write::<RawSequence>(b'd', 0x3b);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CrtcSequence {
    pub active: bool,
    pub sequence: u64,
    /// Nanoseconds in the kernel vblank timestamp's clock domain. The owner
    /// must establish DRM_CAP_TIMESTAMP_MONOTONIC before treating it as UST.
    pub timestamp_nsec: u64,
}

fn decode(raw: RawSequence) -> io::Result<CrtcSequence> {
    let timestamp_nsec = u64::try_from(raw.sequence_ns)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "negative CRTC timestamp"))?;
    if raw.active > 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid CRTC active flag",
        ));
    }
    Ok(CrtcSequence {
        active: raw.active != 0,
        sequence: raw.sequence,
        timestamp_nsec,
    })
}

/// Query an owned DRM card's CRTC object ID (not its index in the resources
/// array). Unsupported queries remain errors; there is no simulated counter
/// or fallback timestamp in this adapter. Inactive CRTCs are reported as such.
pub fn query(card: impl AsFd, crtc_id: u32) -> io::Result<CrtcSequence> {
    if crtc_id == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "zero CRTC object ID",
        ));
    }
    let mut raw = RawSequence {
        crtc_id,
        ..RawSequence::default()
    };
    // SAFETY: the opcode is DRM_IOWR(0x3b, drm_crtc_get_sequence), using the
    // generated kernel ABI type. The fully initialized, pointer-free value
    // lives through this synchronous ioctl; the kernel retains no reference.
    unsafe {
        rustix::ioctl::ioctl(
            card,
            rustix::ioctl::Updater::<GET_SEQUENCE, RawSequence>::new(&mut raw),
        )?;
    }
    decode(raw)
}

#[cfg(test)]
mod tests;
