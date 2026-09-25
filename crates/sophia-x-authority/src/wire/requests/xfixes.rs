/// Decoded Xfixes requests; payloads retain their protocol representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XFixesRequest {
    XfixesQueryVersion {
        major_version: u32,
        minor_version: u32,
    },
    XfixesSelectSelectionInput {
        window: XResourceId,
        selection: XAtom,
        event_mask: u32,
    },
    XfixesCreateRegion {
        region: XResourceId,
        rectangles: Vec<Rect>,
    },
    /// `CopyRegion`, `UnionRegion`, `IntersectRegion` and `SubtractRegion`:
    /// one shape, differing only in how the two sources combine. Copy names
    /// its single source twice.
    XfixesCombineRegion {
        minor_opcode: u8,
        source: XResourceId,
        other: XResourceId,
        destination: XResourceId,
    },
    /// `InvertRegion`: the source subtracted from the bounds the client
    /// supplies, because a region has no complement without them.
    XfixesInvertRegion {
        source: XResourceId,
        bounds: Rect,
        destination: XResourceId,
    },
    XfixesTranslateRegion {
        region: XResourceId,
        dx: i32,
        dy: i32,
    },
    XfixesRegionExtents {
        source: XResourceId,
        destination: XResourceId,
    },
    XfixesFetchRegion {
        region: XResourceId,
    },
    /// `CreateRegionFromBitmap`, `FromGC`, `FromPicture` and `FromWindow`:
    /// a region built from something the server already holds. Only the
    /// window form carries a kind.
    XfixesCreateRegionFrom {
        minor_opcode: u8,
        region: XResourceId,
        source: XResourceId,
        kind: u8,
    },
    XfixesExpandRegion {
        source: XResourceId,
        destination: XResourceId,
        left: u16,
        right: u16,
        top: u16,
        bottom: u16,
    },
    /// An XFIXES minor this server does not implement, decoded so the refusal
    /// can name it.
    XfixesUnimplemented {
        minor_opcode: u8,
    },
    XfixesDestroyRegion {
        region: XResourceId,
    },
    XfixesSetRegion {
        region: XResourceId,
        rectangles: Vec<Rect>,
    },
}
