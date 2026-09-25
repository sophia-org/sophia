/// Decoded Extension requests; payloads retain their protocol representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XExtensionRequest {
    /// `XCMiscGetVersion`. The client states its own version and is told the
    /// server's.
    XCMiscGetVersion {
        major: u16,
        minor: u16,
    },
    /// `XCMiscGetXIDRange`: one fresh block of identifiers.
    XCMiscGetXIDRange,
    /// `XCMiscGetXIDList`: individual identifiers, for a client that wants
    /// them counted rather than as a range.
    XCMiscGetXIDList {
        count: u32,
    },
    /// `XF86VidModeQueryVersion`. Carries nothing; the answer is a constant.
    XF86VidModeQueryVersion,
    /// `XF86VidModeGetModeLine`, for one X screen.
    ///
    /// Sophia has one screen spanning every output, so the screen number is
    /// decoded and checked rather than used to select a display.
    XF86VidModeGetModeLine {
        screen: u16,
    },
    /// `XF86VidModeSetClientVersion`. Recorded and answered, because the
    /// library sends it and expects no reply.
    XF86VidModeSetClientVersion {
        major: u16,
        minor: u16,
    },
    /// A minor opcode this server does not implement, kept so the refusal can
    /// name the request rather than the extension.
    XF86VidModeUnimplemented {
        minor_opcode: u8,
    },
    GeQueryVersion {
        major_version: u16,
        minor_version: u16,
    },
    BigRequestsEnable,
}
