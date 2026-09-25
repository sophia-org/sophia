/// Decoded Dri3 requests; payloads retain their protocol representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XDri3Request {
    Dri3QueryVersion {
        major_version: u32,
        minor_version: u32,
    },
    Dri3Open {
        drawable: XResourceId,
        provider: u32,
    },
    Dri3PixmapFromBuffer {
        pixmap: XResourceId,
        drawable: XResourceId,
        size_bytes: u32,
        width: u16,
        height: u16,
        stride: u16,
        depth: u8,
        bits_per_pixel: u8,
    },
    Dri3PixmapFromBuffers {
        pixmap: XResourceId,
        window: XResourceId,
        num_buffers: u8,
        width: u16,
        height: u16,
        strides: [u32; sophia_protocol::DMA_BUF_MAX_PLANES],
        offsets: [u32; sophia_protocol::DMA_BUF_MAX_PLANES],
        depth: u8,
        bits_per_pixel: u8,
        modifier: u64,
    },
    Dri3FenceFromFd {
        drawable: XResourceId,
        fence: XResourceId,
        initially_triggered: bool,
    },
    Dri3SetDrmDeviceInUse {
        window: XResourceId,
        major: u32,
        minor: u32,
    },
    Dri3GetSupportedModifiers {
        window: XResourceId,
        depth: u8,
        bits_per_pixel: u8,
    },
    Dri3BufferFromPixmap {
        pixmap: XResourceId,
    },
    Dri3BuffersFromPixmap {
        pixmap: XResourceId,
    },
    /// A DRI3 request Sophia decodes but does not implement.
    ///
    /// Kept as a request rather than a parse failure so the answer is a normal
    /// client-visible X11 error naming its own minor opcode, which is what the
    /// compatibility matrix requires of anything unsupported.
    Dri3Unimplemented {
        minor_opcode: u8,
    },
}
