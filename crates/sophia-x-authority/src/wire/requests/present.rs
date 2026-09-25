/// Decoded Present requests; payloads retain their protocol representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XPresentRequest {
    PresentQueryVersion {
        major_version: u32,
        minor_version: u32,
    },
    PresentPixmap {
        transaction: TransactionId,
        window: XResourceId,
        pixmap: XResourceId,
        serial: u32,
        valid_region: u32,
        update_region: u32,
        x_offset: i16,
        y_offset: i16,
        target_crtc: u32,
        wait_fence: Option<XResourceId>,
        idle_fence: Option<XResourceId>,
        options: u32,
        target_msc: u64,
        divisor: u64,
        remainder: u64,
        notifies: Vec<(XResourceId, u32)>,
    },
    PresentSelectInput {
        event_id: XResourceId,
        window: XResourceId,
        event_mask: u32,
    },
    /// A request for one MSC notification: the client asks to be told when the
    /// window's frame counter reaches a target, and blocks on the answer.
    PresentNotifyMsc {
        window: XResourceId,
        serial: u32,
        target_msc: u64,
        divisor: u64,
        remainder: u64,
    },
    /// A Present request Sophia decodes but does not implement.
    ///
    /// Kept as a request rather than a parse failure so the answer is a normal
    /// client-visible X11 error naming its own minor opcode.
    PresentUnimplemented {
        minor_opcode: u8,
    },
    PresentQueryCapabilities {
        target: XResourceId,
    },
}
