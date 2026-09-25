/// Decoded Render requests; payloads retain their protocol representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XRenderRequest {
    /// `RenderQueryVersion`. The client states its own version and receives
    /// the lower of the two.
    RenderQueryVersion {
        major: u32,
        minor: u32,
    },
    /// `RenderQueryPictFormats`: the pixel layouts pictures may take, and
    /// which visual each one belongs to.
    RenderQueryPictFormats,
    RenderCreatePicture {
        picture: XResourceId,
        drawable: XResourceId,
        format: u32,
        values: XRenderPictureValueSet,
    },
    RenderChangePicture {
        picture: XResourceId,
        values: XRenderPictureValueSet,
    },
    RenderSetPictureClipRectangles {
        picture: XResourceId,
        clip_x_origin: i16,
        clip_y_origin: i16,
        rectangles: Vec<Rect>,
    },
    RenderFreePicture {
        picture: XResourceId,
    },
    /// `RenderFillRectangles`: one premultiplied color through one operator.
    RenderFillRectangles {
        op: u8,
        picture: XResourceId,
        color: [u16; 4],
        rectangles: Vec<Rect>,
    },
    /// `RenderComposite`: source, optional mask and destination pictures.
    RenderComposite {
        op: u8,
        source: XResourceId,
        mask: Option<XResourceId>,
        destination: XResourceId,
        source_x: i16,
        source_y: i16,
        mask_x: i16,
        mask_y: i16,
        destination_x: i16,
        destination_y: i16,
        width: u16,
        height: u16,
    },
    RenderCreateGlyphSet {
        glyphset: XResourceId,
        format: u32,
    },
    /// A second identifier for an existing set, which the protocol defines as
    /// sharing rather than copying.
    RenderReferenceGlyphSet {
        glyphset: XResourceId,
        existing: XResourceId,
    },
    RenderFreeGlyphSet {
        glyphset: XResourceId,
    },
    RenderAddGlyphs {
        glyphset: XResourceId,
        ids: Vec<u32>,
        glyphs: Vec<XRenderGlyphInfo>,
        data: Vec<u8>,
    },
    RenderFreeGlyphs {
        glyphset: XResourceId,
        ids: Vec<u32>,
    },
    /// The 8-, 16- and 32-bit glyph identifier widths share one variant; the
    /// width mattered only to the decoder.
    RenderCompositeGlyphs {
        op: u8,
        source: XResourceId,
        destination: XResourceId,
        mask_format: u32,
        glyphset: XResourceId,
        source_x: i16,
        source_y: i16,
        elements: Vec<XRenderGlyphElement>,
        minor_opcode: u8,
    },
    /// `RenderCreateCursor`: a cursor image taken from a picture.
    RenderCreateCursor {
        cursor: XResourceId,
        source: XResourceId,
        hotspot_x: u16,
        hotspot_y: u16,
    },
    /// `RenderSetPictureTransform`: nine 16.16 fixed-point entries, row
    /// major, mapping a destination-relative coordinate to the source pixel.
    RenderSetPictureTransform {
        picture: XResourceId,
        matrix: [i32; 9],
    },
    RenderQueryFilters {
        drawable: XResourceId,
    },
    RenderSetPictureFilter {
        picture: XResourceId,
        name: Vec<u8>,
        has_params: bool,
    },
    /// `RenderTrapezoids`: a coverage mask built from trapezoids, which is
    /// how GTK draws the shadow under a window decoration.
    RenderTrapezoids {
        op: u8,
        source: XResourceId,
        destination: XResourceId,
        mask_format: u32,
        source_x: i16,
        source_y: i16,
        trapezoids: Vec<crate::XRenderTrapezoid>,
    },
    /// `RenderTriangles`, `RenderTriStrip` and `RenderTriFan`, expanded at
    /// decode into the triangles they all describe.
    RenderTriangles {
        op: u8,
        source: XResourceId,
        destination: XResourceId,
        mask_format: u32,
        source_x: i16,
        source_y: i16,
        triangles: Vec<crate::XRenderTriangle>,
        minor_opcode: u8,
    },
    /// `RenderCreateSolidFill`: a source of one colour, already
    /// premultiplied on the wire.
    RenderCreateSolidFill {
        picture: XResourceId,
        color: [u16; 4],
    },
    /// The linear, radial and conical gradients, which differ only in how a
    /// point becomes a position along the ramp.
    RenderCreateGradient {
        picture: XResourceId,
        geometry: crate::XRenderGradientGeometry,
        stops: Vec<crate::XRenderGradientStop>,
        minor_opcode: u8,
    },
    /// A RENDER minor Sophia does not implement, decoded so the refusal can
    /// name it.
    RenderUnimplemented {
        minor_opcode: u8,
    },
}
