/// Decoded Shm requests; payloads retain their protocol representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XShmRequest {
    ShmQueryVersion,
    ShmGetImage {
        drawable: XResourceId,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        plane_mask: u32,
        format: u8,
        segment: XResourceId,
        offset: u32,
    },
    ShmAttach {
        segment: XResourceId,
        shmid: u32,
        read_only: bool,
    },
    /// MIT-SHM 1.2 `AttachFd`. The descriptor is delivered by the socket
    /// layer rather than carried here, which is why this looks lighter than
    /// `ShmAttach` while doing more.
    ShmAttachFd {
        segment: XResourceId,
        read_only: bool,
    },
    /// MIT-SHM 1.2 `CreateSegment`. The server allocates, and the reply hands
    /// the client a descriptor for the memory.
    ShmCreateSegment {
        segment: XResourceId,
        size: u32,
        read_only: bool,
    },
    ShmDetach {
        segment: XResourceId,
    },
    ShmPutImage {
        drawable: XResourceId,
        gc: XResourceId,
        total_width: u16,
        total_height: u16,
        src_x: u16,
        src_y: u16,
        src_width: u16,
        src_height: u16,
        dst_x: i16,
        dst_y: i16,
        depth: u8,
        format: u8,
        send_event: bool,
        segment: XResourceId,
        offset: u32,
    },
    ShmCreatePixmap {
        pixmap: XResourceId,
        drawable: XResourceId,
        width: u16,
        height: u16,
        depth: u8,
        segment: XResourceId,
        offset: u32,
    },
}
