// This bitmap is derived from X.Org's public-domain misc 6x13 ISO-8859-1 font.
// Upstream license: "Public domain font. Share and enjoy."

pub const X_FIXED_6X13_WIDTH: i32 = 6;
pub const X_FIXED_6X13_ASCENT: i32 = 11;
pub const X_FIXED_6X13_DESCENT: i32 = 2;
pub const X_FIXED_6X13_HEIGHT: i32 = X_FIXED_6X13_ASCENT + X_FIXED_6X13_DESCENT;
pub const X_FIXED_6X13_CANONICAL_NAME: &str =
    "-misc-fixed-medium-r-semicondensed--13-120-75-75-c-60-iso8859-1";
/// The same face under the Unicode registry.
///
/// An XLFD's last two fields are its charset registry and encoding, not a
/// different typeface: `iso8859-1` and `iso10646-1` name one 6x13 bitmap
/// indexed two ways. xterm asks for this spelling whenever it is in UTF-8
/// mode, which on a UTF-8 locale is by default, so refusing it refuses the
/// terminal rather than the encoding.
///
/// The built-in face is indexed by one byte and says so, so a client told to
/// expect a single-byte font draws with the 8-bit requests. A configured font
/// path supplies the real two-byte face under this name, and then the same
/// spelling means what it says.
pub const X_FIXED_6X13_UNICODE_NAME: &str =
    "-misc-fixed-medium-r-semicondensed--13-120-75-75-c-60-iso10646-1";

pub mod builtin;
pub mod catalog;
pub mod directory;
mod fixed_6x13;
pub mod metrics;
pub mod pcf;
pub mod xlfd;

use fixed_6x13::X_FIXED_6X13_GLYPHS;

pub use catalog::{XFontCatalog, XFontOpenError, XFontOpenSource};
pub use metrics::{XCharInfo, XFontMetrics, XTextExtents};
pub use pcf::{XGlyph, XLoadedFont};

/// A face a resource holds open.
///
/// Shared, because a graphics context retains the face it was given and must
/// keep drawing with it after the client closes the font identifier. Cheap to
/// clone, so the drawing paths pass it freely.
pub type XFontHandle = std::sync::Arc<pcf::XLoadedFont>;

/// The face every resource starts with, and the one a bare session serves.
///
/// Built once for the process and shared. It is immutable, and every graphics
/// context and every text draw that has not been given another face reaches
/// for it, so constructing it per use would rebuild two hundred and fifty six
/// glyphs on a path that runs per request.
pub fn builtin_font_handle() -> XFontHandle {
    static BUILTIN: std::sync::OnceLock<XFontHandle> = std::sync::OnceLock::new();
    std::sync::Arc::clone(BUILTIN.get_or_init(|| std::sync::Arc::new(builtin::fixed_6x13())))
}

/// Metrics of the built-in face, for the paths that still assume one cell size.
pub fn x_fixed_glyph_rows(byte: u8) -> [u8; 13] {
    X_FIXED_6X13_GLYPHS[usize::from(byte)]
}

/// How many faces `ListFontsWithInfo` will measure for one request.
///
/// Every entry costs a load, so this is smaller than a plain listing's bound.
/// A font menu asks for far fewer; a client asking for more gets the first of
/// them rather than an error.
pub const X_LIST_FONTS_WITH_INFO_MAX_NAMES: usize = 256;

/// Why `OpenFont` did not produce a font resource.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XFontOpenFailure {
    /// No path element publishes the name: a client error, `BadName`.
    Unresolved,
    /// The name resolved but the resource could not be created.
    Resource(crate::XAuthorityRuntimeError),
}
