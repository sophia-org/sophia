const X_ERROR: u8 = 0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XErrorCode {
    BadRequest,
    BadValue,
    BadWindow,
    BadPixmap,
    BadDrawable,
    BadAtom,
    BadFont,
    BadMatch,
    BadAccess,
    BadAlloc,
    BadColor,
    /// A cursor that does not exist. Core error six, which nothing refused
    /// until a request arrived that names a cursor.
    BadCursor,
    BadGraphicsContext,
    BadIdChoice,
    BadName,
    BadLength,
    BadImplementation,
    /// XInput's BadDevice, at the extension's first error code.
    XiBadDevice,
    /// RENDER's own errors, at `X_RENDER_FIRST_ERROR` plus each one's offset.
    /// The protocol defines five, in this order.
    RenderPictFormat,
    RenderPicture,
    RenderPictOp,
    RenderGlyphSet,
    RenderGlyph,
    /// GLX's own errors, at `X_GLX_FIRST_ERROR` plus each one's offset.
    GlxBadDrawable,
    GlxBadPixmap,
    GlxBadFbConfig,
}

impl XErrorCode {
    pub const fn wire_code(self) -> u8 {
        match self {
            Self::BadRequest => 1,
            Self::BadValue => 2,
            Self::BadWindow => 3,
            Self::BadPixmap => 4,
            Self::BadDrawable => 9,
            Self::BadAtom => 5,
            Self::BadFont => 7,
            Self::BadMatch => 8,
            Self::BadAccess => 10,
            Self::BadAlloc => 11,
            Self::BadColor => 12,
            Self::BadCursor => 6,
            Self::BadGraphicsContext => 13,
            Self::BadIdChoice => 14,
            Self::BadName => 15,
            Self::BadLength => 16,
            Self::BadImplementation => 17,
            Self::XiBadDevice => crate::X_INPUT_FIRST_ERROR,
            Self::RenderPictFormat => X_RENDER_FIRST_ERROR + X_RENDER_PICT_FORMAT_ERROR_OFFSET,
            Self::RenderPicture => X_RENDER_FIRST_ERROR + X_RENDER_PICTURE_ERROR_OFFSET,
            Self::RenderPictOp => X_RENDER_FIRST_ERROR + X_RENDER_PICT_OP_ERROR_OFFSET,
            Self::RenderGlyphSet => X_RENDER_FIRST_ERROR + X_RENDER_GLYPH_SET_ERROR_OFFSET,
            Self::RenderGlyph => X_RENDER_FIRST_ERROR + X_RENDER_GLYPH_ERROR_OFFSET,
            Self::GlxBadDrawable => {
                crate::X_GLX_FIRST_ERROR + crate::X_GLX_BAD_DRAWABLE_ERROR_OFFSET
            }
            Self::GlxBadPixmap => crate::X_GLX_FIRST_ERROR + crate::X_GLX_BAD_PIXMAP_ERROR_OFFSET,
            Self::GlxBadFbConfig => {
                crate::X_GLX_FIRST_ERROR + crate::X_GLX_BAD_FB_CONFIG_ERROR_OFFSET
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XClientError {
    pub code: XErrorCode,
    pub sequence: u16,
    pub resource_id: u32,
    pub minor_code: u16,
    pub major_code: u8,
}

pub fn encode_x_client_error(
    byte_order: XByteOrder,
    error: XClientError,
) -> [u8; X_CLIENT_OUTPUT_RECORD_LEN] {
    let mut out = [0; X_CLIENT_OUTPUT_RECORD_LEN];
    out[0] = X_ERROR;
    out[1] = error.code.wire_code();
    put_u16(byte_order, &mut out[2..4], error.sequence);
    put_u32(byte_order, &mut out[4..8], error.resource_id);
    put_u16(byte_order, &mut out[8..10], error.minor_code);
    out[10] = error.major_code;
    out
}

pub fn x_error_from_wire_parse(
    error: &XWireParseError,
    sequence: u16,
    major_code: u8,
    minor_code: u16,
) -> XClientError {
    // A Value error carries the refused value where other errors carry a
    // resource id; a client reading it learns which argument was wrong.
    let (code, resource_id) = match error {
        XWireParseError::Truncated { .. }
        | XWireParseError::InvalidLength { .. }
        | XWireParseError::TrailingBytes(_)
        | XWireParseError::BeyondMaximumLength { .. } => (XErrorCode::BadLength, 0),
        XWireParseError::UnknownOpcode(_) => (XErrorCode::BadRequest, 0),
        XWireParseError::InvalidPropertyMode(value)
        | XWireParseError::InvalidPropertyFormat(value)
        | XWireParseError::InvalidEventType(value) => (XErrorCode::BadValue, u32::from(*value)),
        XWireParseError::InvalidValue(value) => (XErrorCode::BadValue, *value),
        XWireParseError::PropertyValueTooLarge { .. } => (XErrorCode::BadValue, 0),
        XWireParseError::ResourceIdOutsideClientRange { resource_id } => {
            (XErrorCode::BadIdChoice, *resource_id)
        }
    };

    XClientError {
        code,
        sequence,
        resource_id,
        minor_code,
        major_code,
    }
}

/// Converts a runtime refusal while preserving the failing request's opcode.
/// `minor_code` is zero only for core requests.
pub fn x_error_from_runtime(
    error: XAuthorityRuntimeError,
    sequence: u16,
    major_code: u8,
    minor_code: u16,
    resource_id: u32,
) -> XClientError {
    let code = match error {
        XAuthorityRuntimeError::InvalidResource
        | XAuthorityRuntimeError::UnknownResource
        | XAuthorityRuntimeError::WrongResourceKind
        | XAuthorityRuntimeError::InvalidSurface => XErrorCode::BadWindow,
        XAuthorityRuntimeError::InvalidNamespace
        | XAuthorityRuntimeError::CrossNamespaceDenied
        | XAuthorityRuntimeError::StaleGeneration
        | XAuthorityRuntimeError::UnknownRequestorNamespace
        | XAuthorityRuntimeError::MissingSourceNamespace
        | XAuthorityRuntimeError::SameNamespace
        | XAuthorityRuntimeError::PortalRejected => XErrorCode::BadAccess,
        XAuthorityRuntimeError::UnknownSourceOwner => XErrorCode::BadAtom,
        XAuthorityRuntimeError::FocusAuthorityUnavailable => XErrorCode::BadImplementation,
        XAuthorityRuntimeError::InvalidValue => XErrorCode::BadValue,
        XAuthorityRuntimeError::WindowNotViewable => XErrorCode::BadMatch,
    };

    XClientError {
        code,
        sequence,
        resource_id,
        minor_code,
        major_code,
    }
}

pub fn x_selection_failure_event(
    sequence: u16,
    time: XTimestamp,
    requestor: XResourceId,
    selection: u32,
    target: u32,
) -> XClientEvent {
    XClientEvent::SelectionNotify {
        sequence,
        synthetic: false,
        time,
        requestor,
        selection,
        target,
        property: X_ATOM_NONE,
    }
}
