use sophia_protocol::Size;

use crate::{X_TRUE_COLOR_BLUE_MASK, X_TRUE_COLOR_GREEN_MASK, X_TRUE_COLOR_RED_MASK};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XByteOrder {
    LittleEndian,
    BigEndian,
}

impl XByteOrder {
    pub const fn marker(self) -> u8 {
        match self {
            Self::LittleEndian => b'l',
            Self::BigEndian => b'B',
        }
    }

    fn parse(marker: u8) -> Result<Self, XSetupParseError> {
        match marker {
            b'l' => Ok(Self::LittleEndian),
            b'B' => Ok(Self::BigEndian),
            other => Err(XSetupParseError::InvalidByteOrder(other)),
        }
    }

    pub(crate) fn u16(self, bytes: &[u8]) -> u16 {
        let bytes = [bytes[0], bytes[1]];
        match self {
            Self::LittleEndian => u16::from_le_bytes(bytes),
            Self::BigEndian => u16::from_be_bytes(bytes),
        }
    }

    pub(crate) fn u32(self, bytes: &[u8]) -> u32 {
        let bytes = [bytes[0], bytes[1], bytes[2], bytes[3]];
        match self {
            Self::LittleEndian => u32::from_le_bytes(bytes),
            Self::BigEndian => u32::from_be_bytes(bytes),
        }
    }

    pub(crate) fn u64(self, bytes: &[u8]) -> u64 {
        let bytes = [
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ];
        match self {
            Self::LittleEndian => u64::from_le_bytes(bytes),
            Self::BigEndian => u64::from_be_bytes(bytes),
        }
    }

    pub(crate) fn i16(self, bytes: &[u8]) -> i16 {
        let bytes = [bytes[0], bytes[1]];
        match self {
            Self::LittleEndian => i16::from_le_bytes(bytes),
            Self::BigEndian => i16::from_be_bytes(bytes),
        }
    }

    pub(crate) fn push_u16(self, out: &mut Vec<u8>, value: u16) {
        match self {
            Self::LittleEndian => out.extend_from_slice(&value.to_le_bytes()),
            Self::BigEndian => out.extend_from_slice(&value.to_be_bytes()),
        }
    }

    pub(crate) fn push_u32(self, out: &mut Vec<u8>, value: u32) {
        match self {
            Self::LittleEndian => out.extend_from_slice(&value.to_le_bytes()),
            Self::BigEndian => out.extend_from_slice(&value.to_be_bytes()),
        }
    }
}

pub const X_SETUP_CLIENT_PREFIX_LEN: usize = 12;
pub const X_SETUP_REPLY_PREFIX_LEN: usize = 8;
pub const X_SETUP_SUCCESS_BODY_LEN: usize = 32;
pub const X_SETUP_MAX_AUTH_FIELD_LEN: usize = 1024;
pub const X_SETUP_DEFAULT_RESOURCE_ID_BASE: u32 = 0x0020_0000;
pub const X_SETUP_DEFAULT_RESOURCE_ID_MASK: u32 = 0x001f_ffff;
pub const X_SETUP_DEFAULT_MAX_REQUEST_UNITS: u16 = u16::MAX;
pub const X_SETUP_DEFAULT_ROOT: u32 = 0x20;

/// The two values SetInputFocus accepts in place of a window. `None` discards
/// keyboard events until a focus is set again; `PointerRoot` follows the
/// pointer, taking the root of whatever screen it is on at each event. Neither
/// is a resource, so neither is looked up or checked for viewability.
pub const X_FOCUS_NONE: u32 = 0;
pub const X_FOCUS_POINTER_ROOT: u32 = 1;
pub const X_SETUP_DEFAULT_COLORMAP: u32 = 0x21;
pub const X_SETUP_DEFAULT_VISUAL: u32 = 0x22;
pub const X_SETUP_ARGB_VISUAL: u32 = 0x23;
/// The window `_NET_SUPPORTING_WM_CHECK` points at.
///
/// A client asks whether a window manager is running by reading that property
/// off the root and then off the window it names. The window has to exist and
/// has to outlive every client, so the authority owns it here rather than
/// letting a client hold the answer. It is never mapped and never enters the
/// layout; it exists to be pointed at.
pub const X_SETUP_WM_CHECK_WINDOW: u32 = 0x24;
pub const X_SETUP_ROOT_WIDTH: u16 = 1280;
pub const X_SETUP_ROOT_HEIGHT: u16 = 720;

const X_SETUP_SUCCESS: u8 = 1;
const X_SETUP_FAILURE: u8 = 0;
const X_PROTOCOL_MAJOR: u16 = 11;
const X_PROTOCOL_MINOR: u16 = 0;
const X_SETUP_VENDOR: &[u8] = b"Sophia";
const X_SETUP_RELEASE: u32 = 1;
const X_SETUP_PIXMAP_FORMAT_LEN: usize = 8;
const X_SETUP_ROOT_LEN: usize = 40;
const X_SETUP_DEPTH_LEN: usize = 8;
const X_SETUP_VISUAL_LEN: usize = 24;
const X_SETUP_ROOT_DEPTH: u8 = 24;
const X_SETUP_TRUE_COLOR: u8 = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XPixmapFormat {
    pub depth: u8,
    pub bits_per_pixel: u8,
    pub scanline_pad: u8,
}

pub const X_SETUP_PIXMAP_FORMATS: [XPixmapFormat; 7] = [
    XPixmapFormat {
        depth: 1,
        bits_per_pixel: 1,
        scanline_pad: 32,
    },
    XPixmapFormat {
        depth: 4,
        bits_per_pixel: 4,
        scanline_pad: 32,
    },
    XPixmapFormat {
        depth: 8,
        bits_per_pixel: 8,
        scanline_pad: 32,
    },
    XPixmapFormat {
        depth: 15,
        bits_per_pixel: 16,
        scanline_pad: 32,
    },
    XPixmapFormat {
        depth: 16,
        bits_per_pixel: 16,
        scanline_pad: 32,
    },
    XPixmapFormat {
        depth: 24,
        bits_per_pixel: 32,
        scanline_pad: 32,
    },
    XPixmapFormat {
        depth: 32,
        bits_per_pixel: 32,
        scanline_pad: 32,
    },
];

#[must_use]
pub fn x11_pixmap_format(depth: u8) -> Option<XPixmapFormat> {
    X_SETUP_PIXMAP_FORMATS
        .into_iter()
        .find(|format| format.depth == depth)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XSetupRequest {
    pub byte_order: XByteOrder,
    pub major_version: u16,
    pub minor_version: u16,
    pub authorization_protocol_name: Vec<u8>,
    pub authorization_data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XSetupSuccess {
    pub major_version: u16,
    pub minor_version: u16,
    pub release: u32,
    pub resource_id_base: u32,
    pub resource_id_mask: u32,
    pub max_request_units: u16,
    pub vendor: Vec<u8>,
    pub roots: u8,
    pub formats: u8,
    pub root_size: Size,
}

impl XSetupSuccess {
    pub fn minimal() -> Self {
        Self {
            major_version: X_PROTOCOL_MAJOR,
            minor_version: X_PROTOCOL_MINOR,
            release: X_SETUP_RELEASE,
            resource_id_base: X_SETUP_DEFAULT_RESOURCE_ID_BASE,
            resource_id_mask: X_SETUP_DEFAULT_RESOURCE_ID_MASK,
            max_request_units: X_SETUP_DEFAULT_MAX_REQUEST_UNITS,
            vendor: X_SETUP_VENDOR.to_vec(),
            roots: 0,
            formats: 0,
            root_size: Size {
                width: i32::from(X_SETUP_ROOT_WIDTH),
                height: i32::from(X_SETUP_ROOT_HEIGHT),
            },
        }
    }

    pub fn client_compatible() -> Self {
        Self {
            roots: 1,
            formats: X_SETUP_PIXMAP_FORMATS.len() as u8,
            ..Self::minimal()
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XSetupFailure {
    pub major_version: u16,
    pub minor_version: u16,
    pub reason: Vec<u8>,
}

impl XSetupFailure {
    pub fn new(reason: impl Into<Vec<u8>>) -> Self {
        Self {
            major_version: X_PROTOCOL_MAJOR,
            minor_version: X_PROTOCOL_MINOR,
            reason: reason.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XSetupParseError {
    Truncated {
        needed: usize,
        actual: usize,
    },
    InvalidByteOrder(u8),
    UnsupportedMajorVersion(u16),
    AuthFieldTooLarge {
        field: &'static str,
        len: usize,
        max: usize,
    },
    TrailingBytes(usize),
    ReplyFieldTooLarge {
        field: &'static str,
        len: usize,
        max: usize,
    },
}

impl core::fmt::Display for XSetupParseError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for XSetupParseError {}

pub fn parse_x11_setup_request(bytes: &[u8]) -> Result<XSetupRequest, XSetupParseError> {
    if bytes.len() < X_SETUP_CLIENT_PREFIX_LEN {
        return Err(XSetupParseError::Truncated {
            needed: X_SETUP_CLIENT_PREFIX_LEN,
            actual: bytes.len(),
        });
    }

    let byte_order = XByteOrder::parse(bytes[0])?;
    let major_version = byte_order.u16(&bytes[2..4]);
    let minor_version = byte_order.u16(&bytes[4..6]);
    if major_version != X_PROTOCOL_MAJOR {
        return Err(XSetupParseError::UnsupportedMajorVersion(major_version));
    }

    let auth_name_len = usize::from(byte_order.u16(&bytes[6..8]));
    let auth_data_len = usize::from(byte_order.u16(&bytes[8..10]));
    validate_auth_len("authorization_protocol_name", auth_name_len)?;
    validate_auth_len("authorization_data", auth_data_len)?;

    let auth_name_padded = padded_len(auth_name_len);
    let auth_data_padded = padded_len(auth_data_len);
    let needed = X_SETUP_CLIENT_PREFIX_LEN
        .checked_add(auth_name_padded)
        .and_then(|len| len.checked_add(auth_data_padded))
        .ok_or(XSetupParseError::AuthFieldTooLarge {
            field: "authorization_total",
            len: usize::MAX,
            max: X_SETUP_MAX_AUTH_FIELD_LEN * 2,
        })?;
    if bytes.len() < needed {
        return Err(XSetupParseError::Truncated {
            needed,
            actual: bytes.len(),
        });
    }
    if bytes.len() > needed {
        return Err(XSetupParseError::TrailingBytes(bytes.len() - needed));
    }

    let auth_name_start = X_SETUP_CLIENT_PREFIX_LEN;
    let auth_name_end = auth_name_start + auth_name_len;
    let auth_data_start = auth_name_start + auth_name_padded;
    let auth_data_end = auth_data_start + auth_data_len;

    Ok(XSetupRequest {
        byte_order,
        major_version,
        minor_version,
        authorization_protocol_name: bytes[auth_name_start..auth_name_end].to_vec(),
        authorization_data: bytes[auth_data_start..auth_data_end].to_vec(),
    })
}

pub fn x11_setup_request_total_len(prefix: &[u8]) -> Result<usize, XSetupParseError> {
    if prefix.len() < X_SETUP_CLIENT_PREFIX_LEN {
        return Err(XSetupParseError::Truncated {
            needed: X_SETUP_CLIENT_PREFIX_LEN,
            actual: prefix.len(),
        });
    }

    let byte_order = XByteOrder::parse(prefix[0])?;
    let auth_name_len = usize::from(byte_order.u16(&prefix[6..8]));
    let auth_data_len = usize::from(byte_order.u16(&prefix[8..10]));
    validate_auth_len("authorization_protocol_name", auth_name_len)?;
    validate_auth_len("authorization_data", auth_data_len)?;

    X_SETUP_CLIENT_PREFIX_LEN
        .checked_add(padded_len(auth_name_len))
        .and_then(|len| len.checked_add(padded_len(auth_data_len)))
        .ok_or(XSetupParseError::AuthFieldTooLarge {
            field: "authorization_total",
            len: usize::MAX,
            max: X_SETUP_MAX_AUTH_FIELD_LEN * 2,
        })
}

pub fn encode_x11_setup_success(
    byte_order: XByteOrder,
    setup: &XSetupSuccess,
) -> Result<Vec<u8>, XSetupParseError> {
    if setup.root_size.width <= 0
        || setup.root_size.height <= 0
        || setup.root_size.width > i32::from(u16::MAX)
        || setup.root_size.height > i32::from(u16::MAX)
    {
        return Err(XSetupParseError::ReplyFieldTooLarge {
            field: "root_size",
            len: usize::MAX,
            max: u16::MAX as usize,
        });
    }
    if setup.vendor.len() > u16::MAX as usize {
        return Err(XSetupParseError::ReplyFieldTooLarge {
            field: "vendor",
            len: setup.vendor.len(),
            max: u16::MAX as usize,
        });
    }

    if usize::from(setup.formats) > X_SETUP_PIXMAP_FORMATS.len() {
        return Err(XSetupParseError::ReplyFieldTooLarge {
            field: "setup_formats",
            len: usize::from(setup.formats),
            max: X_SETUP_PIXMAP_FORMATS.len(),
        });
    }
    let root_section_len = usize::from(setup.roots)
        .checked_mul(
            X_SETUP_ROOT_LEN
                + X_SETUP_PIXMAP_FORMATS.len() * X_SETUP_DEPTH_LEN
                + 2 * X_SETUP_VISUAL_LEN,
        )
        .ok_or(XSetupParseError::ReplyFieldTooLarge {
            field: "setup_roots",
            len: usize::MAX,
            max: u16::MAX as usize * 4,
        })?;
    let format_section_len = usize::from(setup.formats)
        .checked_mul(X_SETUP_PIXMAP_FORMAT_LEN)
        .ok_or(XSetupParseError::ReplyFieldTooLarge {
            field: "setup_formats",
            len: usize::MAX,
            max: u16::MAX as usize * 4,
        })?;
    let body_len = X_SETUP_SUCCESS_BODY_LEN
        + padded_len(setup.vendor.len())
        + format_section_len
        + root_section_len;
    let body_units =
        u16::try_from(body_len / 4).map_err(|_| XSetupParseError::ReplyFieldTooLarge {
            field: "setup_success_body",
            len: body_len,
            max: u16::MAX as usize * 4,
        })?;

    let mut out = Vec::with_capacity(X_SETUP_REPLY_PREFIX_LEN + body_len);
    out.push(X_SETUP_SUCCESS);
    out.push(0);
    byte_order.push_u16(&mut out, setup.major_version);
    byte_order.push_u16(&mut out, setup.minor_version);
    byte_order.push_u16(&mut out, body_units);

    byte_order.push_u32(&mut out, setup.release);
    byte_order.push_u32(&mut out, setup.resource_id_base);
    byte_order.push_u32(&mut out, setup.resource_id_mask);
    byte_order.push_u32(&mut out, 0);
    byte_order.push_u16(&mut out, setup.vendor.len() as u16);
    byte_order.push_u16(&mut out, setup.max_request_units);
    out.push(setup.roots);
    out.push(setup.formats);
    out.push(byte_order_code(byte_order));
    out.push(byte_order_code(byte_order));
    out.push(32);
    out.push(32);
    out.push(8);
    out.push(255);
    byte_order.push_u32(&mut out, 0);
    out.extend_from_slice(&setup.vendor);
    pad_to_four(&mut out);
    for format in X_SETUP_PIXMAP_FORMATS
        .iter()
        .take(usize::from(setup.formats))
    {
        encode_pixmap_format(&mut out, *format);
    }
    for _ in 0..setup.roots {
        encode_root(byte_order, &mut out, setup.root_size);
    }

    Ok(out)
}

pub fn encode_x11_setup_failure(
    byte_order: XByteOrder,
    failure: &XSetupFailure,
) -> Result<Vec<u8>, XSetupParseError> {
    if failure.reason.len() > u8::MAX as usize {
        return Err(XSetupParseError::ReplyFieldTooLarge {
            field: "failure_reason",
            len: failure.reason.len(),
            max: u8::MAX as usize,
        });
    }
    let reason_padded = padded_len(failure.reason.len());
    let reason_units =
        u16::try_from(reason_padded / 4).map_err(|_| XSetupParseError::ReplyFieldTooLarge {
            field: "failure_reason",
            len: failure.reason.len(),
            max: u16::MAX as usize * 4,
        })?;

    let mut out = Vec::with_capacity(X_SETUP_REPLY_PREFIX_LEN + reason_padded);
    out.push(X_SETUP_FAILURE);
    out.push(failure.reason.len() as u8);
    byte_order.push_u16(&mut out, failure.major_version);
    byte_order.push_u16(&mut out, failure.minor_version);
    byte_order.push_u16(&mut out, reason_units);
    out.extend_from_slice(&failure.reason);
    pad_to_four(&mut out);
    Ok(out)
}

pub(crate) const fn padded_len(len: usize) -> usize {
    (len + 3) & !3
}

fn validate_auth_len(field: &'static str, len: usize) -> Result<(), XSetupParseError> {
    if len > X_SETUP_MAX_AUTH_FIELD_LEN {
        return Err(XSetupParseError::AuthFieldTooLarge {
            field,
            len,
            max: X_SETUP_MAX_AUTH_FIELD_LEN,
        });
    }
    Ok(())
}

fn pad_to_four(out: &mut Vec<u8>) {
    out.resize(padded_len(out.len()), 0);
}

fn encode_pixmap_format(out: &mut Vec<u8>, format: XPixmapFormat) {
    out.push(format.depth);
    out.push(format.bits_per_pixel);
    out.push(format.scanline_pad);
    out.extend_from_slice(&[0; 5]);
}

fn encode_root(byte_order: XByteOrder, out: &mut Vec<u8>, root_size: Size) {
    byte_order.push_u32(out, X_SETUP_DEFAULT_ROOT);
    byte_order.push_u32(out, X_SETUP_DEFAULT_COLORMAP);
    byte_order.push_u32(out, 0x00ff_ffff);
    byte_order.push_u32(out, 0x0000_0000);
    byte_order.push_u32(out, 0);
    byte_order.push_u16(out, root_size.width as u16);
    byte_order.push_u16(out, root_size.height as u16);
    byte_order.push_u16(out, logical_pixels_to_millimeters(root_size.width));
    byte_order.push_u16(out, logical_pixels_to_millimeters(root_size.height));
    byte_order.push_u16(out, 1);
    byte_order.push_u16(out, 1);
    byte_order.push_u32(out, X_SETUP_DEFAULT_VISUAL);
    out.push(0);
    out.push(0);
    out.push(X_SETUP_ROOT_DEPTH);
    out.push(X_SETUP_PIXMAP_FORMATS.len() as u8);

    for format in X_SETUP_PIXMAP_FORMATS {
        let visual = match format.depth {
            24 => Some(X_SETUP_DEFAULT_VISUAL),
            32 => Some(X_SETUP_ARGB_VISUAL),
            _ => None,
        };
        out.push(format.depth);
        out.push(0);
        byte_order.push_u16(out, u16::from(visual.is_some()));
        byte_order.push_u32(out, 0);
        if let Some(visual) = visual {
            encode_true_color_visual(byte_order, out, visual);
        }
    }
}

fn encode_true_color_visual(byte_order: XByteOrder, out: &mut Vec<u8>, visual: u32) {
    byte_order.push_u32(out, visual);
    out.push(X_SETUP_TRUE_COLOR);
    out.push(8);
    byte_order.push_u16(out, 256);
    byte_order.push_u32(out, X_TRUE_COLOR_RED_MASK);
    byte_order.push_u32(out, X_TRUE_COLOR_GREEN_MASK);
    byte_order.push_u32(out, X_TRUE_COLOR_BLUE_MASK);
    byte_order.push_u32(out, 0);
}

fn logical_pixels_to_millimeters(pixels: i32) -> u16 {
    // X setup requires physical dimensions. Engine deliberately does not
    // expose connector/EDID identity, so the frontend reports a stable 96-DPI
    // logical size instead of inventing a physical monitor measurement.
    let millimeters = i64::from(pixels).saturating_mul(254).saturating_add(480) / 960;
    u16::try_from(millimeters).unwrap_or(u16::MAX).max(1)
}

const fn byte_order_code(byte_order: XByteOrder) -> u8 {
    match byte_order {
        XByteOrder::LittleEndian => 0,
        XByteOrder::BigEndian => 1,
    }
}
