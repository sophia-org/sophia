#[cfg(feature = "libdrm-events")]
use super::*;
#[cfg(feature = "libdrm-events")]
use crate::prelude::*;

#[cfg(feature = "libdrm-events")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LibdrmNativeOutputSlot {
    raw: u16,
}

#[cfg(feature = "libdrm-events")]
impl LibdrmNativeOutputSlot {
    pub const fn new(raw: u16) -> Option<Self> {
        if raw == 0 {
            return None;
        }

        Some(Self { raw })
    }

    pub const fn raw(self) -> u16 {
        self.raw
    }
}

#[cfg(feature = "libdrm-events")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Which logical output a CRTC's flip belongs to, and which head it is.
///
/// The slot already distinguishes heads -- it is assigned per selection, and a
/// selection is one connector. The route translates that private slot into the
/// opaque head identity Engine records may carry; the connector integer stays
/// behind the backend's head table.
pub struct LibdrmNativeOutputRoute {
    pub slot: LibdrmNativeOutputSlot,
    pub output: OutputId,
    pub head: sophia_engine::RenderHeadId,
}

#[cfg(feature = "libdrm-events")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LibdrmNativeCrtcRoute {
    crtc: drm::control::crtc::Handle,
    slot: LibdrmNativeOutputSlot,
    last_frame_serial: u64,
}

#[cfg(feature = "libdrm-events")]
impl LibdrmNativeCrtcRoute {
    pub const fn new(crtc: drm::control::crtc::Handle, slot: LibdrmNativeOutputSlot) -> Self {
        Self {
            crtc,
            slot,
            last_frame_serial: 0,
        }
    }

    pub(crate) const fn slot(self) -> LibdrmNativeOutputSlot {
        self.slot
    }

    fn observe_frame(&mut self, native_frame: u32) -> u64 {
        let next_serial = u64::from(native_frame)
            .max(self.last_frame_serial.saturating_add(1))
            .max(1);
        self.last_frame_serial = next_serial;
        next_serial
    }
}

#[cfg(feature = "libdrm-events")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LibdrmNativePageFlipCallback {
    pub output_slot: LibdrmNativeOutputSlot,
    pub frame_serial: u64,
    kernel_ust_usec: Option<u64>,
    kernel_sequence: Option<u32>,
}

#[cfg(feature = "libdrm-events")]
impl LibdrmNativePageFlipCallback {
    pub const fn new(output_slot: LibdrmNativeOutputSlot, frame_serial: u64) -> Self {
        Self {
            output_slot,
            frame_serial,
            kernel_ust_usec: None,
            kernel_sequence: None,
        }
    }

    pub fn new_with_kernel_timestamp(
        output_slot: LibdrmNativeOutputSlot,
        frame_serial: u64,
        timestamp: std::time::Duration,
    ) -> Self {
        Self {
            output_slot,
            frame_serial,
            kernel_ust_usec: Some(u64::try_from(timestamp.as_micros()).unwrap_or(u64::MAX)),
            kernel_sequence: None,
        }
    }

    pub fn new_with_kernel_clock(
        output_slot: LibdrmNativeOutputSlot,
        frame_serial: u64,
        sequence: u32,
        timestamp: std::time::Duration,
    ) -> Self {
        Self {
            kernel_sequence: Some(sequence),
            ..Self::new_with_kernel_timestamp(output_slot, frame_serial, timestamp)
        }
    }

    pub const fn kernel_sequence(self) -> Option<u32> {
        self.kernel_sequence
    }

    pub const fn kernel_ust_usec(self) -> Option<u64> {
        self.kernel_ust_usec
    }

    pub fn decode(self, routes: &[LibdrmNativeOutputRoute]) -> LibdrmNativePageFlipDecodeReport {
        if self.frame_serial == 0 {
            return LibdrmNativePageFlipDecodeReport {
                status: LibdrmNativePageFlipDecodeStatus::InvalidFrameSerial,
                callback: None,
            };
        }

        let Some(route) = routes
            .iter()
            .find(|route| route.slot == self.output_slot)
            .copied()
        else {
            return LibdrmNativePageFlipDecodeReport {
                status: LibdrmNativePageFlipDecodeStatus::UnknownOutputSlot,
                callback: None,
            };
        };

        LibdrmNativePageFlipDecodeReport {
            status: LibdrmNativePageFlipDecodeStatus::Decoded,
            callback: Some(LivePageFlipCallback {
                output: route.output,
                head: route.head,
                frame_serial: self.frame_serial,
            }),
        }
    }
}

#[cfg(feature = "libdrm-events")]
pub fn reduce_native_page_flip_event(
    event: &drm::control::PageFlipEvent,
    routes: &mut [LibdrmNativeCrtcRoute],
) -> Option<LibdrmNativePageFlipCallback> {
    let route = routes.iter_mut().find(|route| route.crtc == event.crtc)?;
    let slot = route.slot();
    let frame_serial = route.observe_frame(event.frame);
    Some(LibdrmNativePageFlipCallback::new_with_kernel_clock(
        slot,
        frame_serial,
        event.frame,
        event.duration,
    ))
}

#[cfg(feature = "libdrm-events")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LibdrmKernelPageFlipTimestamp {
    pub output: OutputId,
    /// Physical head identity. Logical output and sequence are insufficient for
    /// a mirror group because two CRTCs may report the same kernel sequence.
    pub head: sophia_engine::RenderHeadId,
    pub frame_serial: u64,
    /// Raw kernel counter, distinct from the normalized retirement serial.
    /// Absent on synthetic callbacks; those must never advance Present MSC.
    pub kernel_sequence: Option<u32>,
    pub ust_usec: u64,
}
