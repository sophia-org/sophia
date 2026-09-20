//! Which colour a fill takes, pixel by pixel.

use super::{XAuthorityCpuBufferSnapshot, XGraphicsContextValues, raster_ops::XFillPattern};

/// Choose the pattern a graphics context's fill style asks for.
///
/// A style naming a pixmap the store does not hold falls back to solid rather
/// than painting nothing, which is what the protocol's own "unspecified"
/// leaves room for and what keeps a mis-specified fill visible instead of
/// silently blank.
pub(super) fn fill_pattern<'a>(
    gc: &XGraphicsContextValues,
    pixels: Option<&'a XAuthorityCpuBufferSnapshot>,
) -> XFillPattern<'a> {
    let origin = (
        i32::from(gc.tile_stipple_x_origin),
        i32::from(gc.tile_stipple_y_origin),
    );
    match (gc.fill_style, pixels) {
        (crate::X_FILL_TILED, Some(pixels)) => XFillPattern::Tile { pixels, origin },
        (crate::X_FILL_STIPPLED, Some(pixels)) => XFillPattern::Stipple {
            pixels,
            origin,
            foreground: gc.foreground,
            background: None,
        },
        (crate::X_FILL_OPAQUE_STIPPLED, Some(pixels)) => XFillPattern::Stipple {
            pixels,
            origin,
            foreground: gc.foreground,
            background: Some(gc.background),
        },
        _ => XFillPattern::Solid(gc.foreground),
    }
}
