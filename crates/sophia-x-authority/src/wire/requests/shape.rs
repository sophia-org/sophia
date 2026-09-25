/// Decoded Shape requests; payloads retain their protocol representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XShapeRequest {
    ShapeQueryVersion,
    /// `ShapeRectangles`: a rectangle list combined into one of the window's
    /// three shapes.
    ShapeRectangles {
        op: u8,
        kind: u8,
        ordering: u8,
        destination: XResourceId,
        x_offset: i16,
        y_offset: i16,
        rectangles: Vec<Rect>,
    },
    /// `ShapeMask`: the same, sourced from a depth-1 pixmap. A `None` source
    /// with Set returns the kind to its default.
    ShapeMask {
        op: u8,
        kind: u8,
        destination: XResourceId,
        x_offset: i16,
        y_offset: i16,
        source: Option<XResourceId>,
    },
    /// `ShapeCombine`: sourced from another window's shape.
    ShapeCombine {
        op: u8,
        kind: u8,
        source_kind: u8,
        destination: XResourceId,
        x_offset: i16,
        y_offset: i16,
        source: XResourceId,
    },
    ShapeOffset {
        kind: u8,
        destination: XResourceId,
        x_offset: i16,
        y_offset: i16,
    },
    ShapeQueryExtents {
        window: XResourceId,
    },
    ShapeSelectInput {
        window: XResourceId,
        enable: bool,
    },
    ShapeInputSelected {
        window: XResourceId,
    },
    ShapeGetRectangles {
        window: XResourceId,
        kind: u8,
    },
    ShapeUnimplemented {
        minor_opcode: u8,
    },
}
