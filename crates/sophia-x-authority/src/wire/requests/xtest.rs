/// Decoded XTest requests; payloads retain their protocol representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XTestRequest {
    /// `XTestGetVersion`. Both fields are carried and neither is answered
    /// with: the reply is a constant 2.1 whatever was asked for, which is
    /// what the reference server does and what the wire cases demand.
    XTestGetVersion {
        major_version: u8,
        minor_version: u16,
    },
    /// `XTestCompareCursor`. The cursor stays a raw id because zero is None
    /// and one is CurrentCursor, and both are decided before any lookup.
    XTestCompareCursor {
        window: XResourceId,
        cursor: u32,
    },
    /// `XTestFakeInput`, the 36-byte request.
    ///
    /// `event_type` has the send-event bit masked off, as the reference
    /// server does, while `sent_event_type` keeps the byte that arrived so a
    /// refusal can name it. `root` is raw because zero is None, meaning the
    /// pointer's current screen. `events` counts the 32-byte records in the
    /// body: more than one is only meaningful to the XInput path, so the
    /// count is carried here and judged against the type by the dispatcher.
    XTestFakeInput {
        event_type: u8,
        sent_event_type: u8,
        detail: u8,
        delay: u32,
        root: u32,
        root_x: i16,
        root_y: i16,
        events: usize,
    },
    /// `XTestGrabControl`. The boolean is strict, so the byte is carried
    /// unvalidated and refused where a value can be reported.
    XTestGrabControl {
        impervious: u8,
    },
    /// An XTEST minor the extension does not define. XTEST has had four
    /// requests since it was defined and will not grow any.
    XTestUnimplemented {
        minor_opcode: u8,
    },
}
