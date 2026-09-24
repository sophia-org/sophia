use sophia_protocol::{
    NamespaceId, PortalTransferId, Rect, Region, SurfaceConstraints, SurfaceId, TransactionId,
};

use crate::{
    XAtom, XAuthorityRequestKind, XAuthorityRequestPacket, XByteOrder, XClientEvent,
    XGraphicsContextValues, XPoint, XPropertyChange, XPropertyMode, XPropertyRead, XResourceId,
    XSelectionChangeKind, padded_len,
};

include!("wire/constants.rs");
include!("wire/constants_xtest.rs");
include!("wire/core/color_cursor.rs");
include!("wire/core/drawing.rs");
include!("wire/core/discovery.rs");
include!("wire/core/input.rs");
include!("wire/core/properties.rs");
include!("wire/core/resources.rs");
include!("wire/core/windows.rs");
include!("wire/extensions/big_requests.rs");
include!("wire/extensions/dri3.rs");
include!("wire/extensions/glx.rs");
include!("wire/extensions/present.rs");
include!("wire/extensions/query_version.rs");
include!("wire/extensions/randr.rs");
include!("wire/extensions/shm.rs");
include!("wire/extensions/sophia_present.rs");
include!("wire/extensions/sync.rs");
include!("wire/extensions/xfixes.rs");
include!("wire/extensions/xf86_vidmode.rs");
include!("wire/extensions/xc_misc.rs");
include!("wire/extensions/render.rs");
include!("wire/extensions/shape.rs");
include!("wire/extensions/xtest.rs");
include!("wire/extensions/xi.rs");
include!("wire/extensions/xkb.rs");
include!("wire/validation.rs");

/// The XID range granted to one X11 client during connection setup.
///
/// Server-owned resources such as the root window are intentionally outside
/// this range. It therefore applies only when a request creates a new client
/// resource, not when it references an existing drawable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XWireClientResourceRange {
    pub base: u32,
    pub mask: u32,
}

impl XWireClientResourceRange {
    pub const fn owns_new_resource(self, resource_id: u32) -> bool {
        resource_id != 0 && (resource_id & !self.mask) == self.base
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XWireClientContext {
    pub byte_order: XByteOrder,
    pub namespace: NamespaceId,
    pub transaction: TransactionId,
    /// `None` preserves deterministic decoder fixtures that are not attached
    /// to a live X11 setup. Socket clients must always provide their range.
    pub resource_id_range: Option<XWireClientResourceRange>,
}

impl XWireClientContext {
    fn validate_new_resource_id(self, resource_id: u32) -> Result<(), XWireParseError> {
        if self
            .resource_id_range
            .is_some_and(|range| !range.owns_new_resource(resource_id))
        {
            return Err(XWireParseError::ResourceIdOutsideClientRange { resource_id });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XGlxContextConfig {
    Visual(u32),
    FbConfig(u32),
}

/// One item of a `PolyText8` or `PolyText16` request.
///
/// Characters are held as the protocol's CHAR2B value whichever request
/// carried them: an 8-bit request's byte is the low half of a character whose
/// high half is zero, which is exactly how the server reads it. Holding one
/// type keeps a single drawing path for both requests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XPolyTextItem {
    Text { delta: i8, chars: Vec<u16> },
    Font { font: XResourceId },
}

/// The colormap requests a TrueColor-only server declines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XColormapRequestKind {
    /// Read-write allocation: there are no allocable cells on a TrueColor
    /// visual, so the answer is `BadAlloc`.
    AllocCells,
    AllocPlanes,
    /// Storing into a read-only colormap is `BadAccess`.
    StoreColors,
    StoreNamedColor,
    /// Accepted and ignored: a TrueColor colormap is always installed.
    Install,
    Uninstall,
    /// Accepted: nothing was allocated, so nothing needs freeing.
    FreeColors,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XWireRequest {
    Authority(XAuthorityRequestPacket),
    CreateWindow {
        packet: XAuthorityRequestPacket,
        parent: XResourceId,
        depth: u8,
        visual: u32,
        colormap: Option<XResourceId>,
        background_pixmap: Option<crate::XWindowBackground>,
        background_pixel: Option<u32>,
        override_redirect: bool,
        event_mask: Option<u32>,
        do_not_propagate_mask: Option<u32>,
        /// Raw, for the same reason the change request carries it raw.
        cursor: Option<u32>,
        /// The window was created InputOnly: it can be the target of input
        /// and geometry requests but never of a drawing request.
        input_only: bool,
    },
    ChangeWindowAttributes {
        window: XResourceId,
        /// None means the request did not mention the attribute at all.
        background_pixmap: Option<crate::XWindowBackground>,
        background_pixel: Option<u32>,
        override_redirect: Option<bool>,
        event_mask: Option<u32>,
        do_not_propagate_mask: Option<u32>,
        /// Raw, because zero is None in the attribute: the window stops
        /// having a cursor of its own rather than naming resource zero.
        cursor: Option<u32>,
    },
    GetWindowAttributes {
        window: XResourceId,
    },
    DestroyWindow {
        window: XResourceId,
    },
    ReparentWindow {
        window: XResourceId,
        parent: XResourceId,
        x: i16,
        y: i16,
    },
    /// Destroy every child of this window. The window itself survives.
    DestroySubwindows {
        window: XResourceId,
    },
    MapSubwindows {
        window: XResourceId,
    },
    UnmapWindow {
        window: XResourceId,
    },
    ConfigureWindow {
        window: XResourceId,
        value_mask: u16,
        x: Option<i16>,
        y: Option<i16>,
        width: Option<u16>,
        height: Option<u16>,
        sibling: Option<XResourceId>,
        stack_mode: Option<u8>,
    },
    GetGeometry {
        drawable: XResourceId,
    },
    QueryTree {
        window: XResourceId,
    },
    InternAtom {
        only_if_exists: bool,
        name: String,
    },
    GetAtomName {
        atom: XAtom,
    },
    ChangeProperty(XPropertyChange),
    GetProperty(XPropertyRead),
    ListProperties {
        window: XResourceId,
    },
    GetSelectionOwner {
        selection: XAtom,
    },
    SendSelectionNotify {
        destination: XResourceId,
        event_mask: u32,
        event: XClientEvent,
    },
    GrabPointer {
        window: XResourceId,
        event_mask: u16,
        owner_events: bool,
        pointer_mode: u8,
        keyboard_mode: u8,
        time: u32,
    },
    UngrabPointer {
        time: u32,
    },
    GrabButton {
        window: XResourceId,
        event_mask: u16,
        button: u8,
        modifiers: u16,
        owner_events: bool,
        pointer_mode: u8,
        keyboard_mode: u8,
    },
    UngrabButton {
        window: XResourceId,
        button: u8,
        modifiers: u16,
    },
    GrabKeyboard {
        window: XResourceId,
        owner_events: bool,
        pointer_mode: u8,
        keyboard_mode: u8,
        time: u32,
    },
    UngrabKeyboard {
        time: u32,
    },
    GrabKey {
        window: XResourceId,
        key: u8,
        modifiers: u16,
        owner_events: bool,
        pointer_mode: u8,
        keyboard_mode: u8,
    },
    UngrabKey {
        window: XResourceId,
        key: u8,
        modifiers: u16,
    },
    AllowEvents {
        mode: u8,
        time: u32,
    },
    GrabServer,
    UngrabServer,
    NoOperation,
    CreateGraphicsContext {
        gc: XResourceId,
        drawable: XResourceId,
        values: XGraphicsContextValues,
    },
    ChangeGraphicsContext {
        gc: XResourceId,
        value_mask: u32,
        values: XGraphicsContextValues,
    },
    SetClipRectangles {
        gc: XResourceId,
        clip_x_origin: i16,
        clip_y_origin: i16,
        rectangles: Vec<Rect>,
    },
    FreeGraphicsContext {
        gc: XResourceId,
    },
    ClearArea {
        exposures: bool,
        window: XResourceId,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
    },
    PolyFillRectangle {
        drawable: XResourceId,
        gc: XResourceId,
        rectangles: Vec<Rect>,
    },
    PutImage {
        format: u8,
        drawable: XResourceId,
        gc: XResourceId,
        width: u16,
        height: u16,
        dst_x: i16,
        dst_y: i16,
        left_pad: u8,
        depth: u8,
        data: Vec<u8>,
    },
    GetImage {
        format: u8,
        drawable: XResourceId,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        plane_mask: u32,
    },
    PolyText8 {
        drawable: XResourceId,
        gc: XResourceId,
        x: i16,
        y: i16,
        items: Vec<XPolyTextItem>,
    },
    PolyText16 {
        drawable: XResourceId,
        gc: XResourceId,
        x: i16,
        y: i16,
        items: Vec<XPolyTextItem>,
    },
    ImageText8 {
        drawable: XResourceId,
        gc: XResourceId,
        x: i16,
        y: i16,
        text: Vec<u8>,
    },
    ImageText16 {
        drawable: XResourceId,
        gc: XResourceId,
        x: i16,
        y: i16,
        chars: Vec<u16>,
    },
    QueryTextExtents {
        fontable: XResourceId,
        chars: Vec<u16>,
    },
    SetFontPath,
    GetFontPath,
    /// A colormap request this TrueColor server answers without serving.
    ///
    /// Decoded so the refusal is the protocol's own error rather than
    /// `BadRequest`, which some clients treat as fatal.
    ColormapRequest {
        kind: XColormapRequestKind,
        colormap: XResourceId,
    },
    /// A new colormap on the source's visual: a static visual has no
    /// allocations to move, so the copy is the creation.
    CopyColormapAndFree {
        colormap: XResourceId,
        source: XResourceId,
    },
    ListInstalledColormaps {
        window: XResourceId,
    },
    CopyGraphicsContext {
        source: XResourceId,
        destination: XResourceId,
        value_mask: u32,
    },
    SetDashes {
        gc: XResourceId,
        dash_offset: u16,
        dashes: Vec<u8>,
    },
    CreateColormap {
        alloc: u8,
        colormap: XResourceId,
        window: XResourceId,
        visual: u32,
    },
    FreeColormap {
        colormap: XResourceId,
    },
    AllocColor {
        colormap: XResourceId,
        red: u16,
        green: u16,
        blue: u16,
    },
    AllocNamedColor {
        colormap: XResourceId,
        name: String,
    },
    LookupColor {
        colormap: XResourceId,
        name: String,
    },
    GetInputFocus,
    SetInputFocus {
        focus: XResourceId,
        revert_to: u8,
        time: u32,
    },
    OpenFont {
        font: XResourceId,
        name: String,
    },
    CloseFont {
        font: XResourceId,
    },
    QueryFont {
        font: XResourceId,
    },
    ListFonts {
        max_names: u16,
        pattern: String,
    },
    ListFontsWithInfo {
        max_names: u16,
        pattern: String,
    },
    CreatePixmap {
        depth: u8,
        pixmap: XResourceId,
        drawable: XResourceId,
        width: u16,
        height: u16,
    },
    FreePixmap {
        pixmap: XResourceId,
    },
    QueryExtension {
        name: String,
    },
    DeleteProperty {
        window: XResourceId,
        property: u32,
    },
    QueryPointer {
        window: XResourceId,
    },
    ListExtensions,
    QueryBestSize {
        class: u8,
        drawable: XResourceId,
        width: u16,
        height: u16,
    },
    CopyArea {
        source: XResourceId,
        destination: XResourceId,
        gc: XResourceId,
        src_x: i16,
        src_y: i16,
        dst_x: i16,
        dst_y: i16,
        width: u16,
        height: u16,
    },
    CopyPlane {
        source: XResourceId,
        destination: XResourceId,
        gc: XResourceId,
        src_x: i16,
        src_y: i16,
        dst_x: i16,
        dst_y: i16,
        width: u16,
        height: u16,
        /// Exactly one bit, selecting the source plane to copy.
        bit_plane: u32,
    },
    PolySegment {
        drawable: XResourceId,
        gc: XResourceId,
        /// Each segment's two endpoints, kept so the segments can be drawn
        /// rather than only reported as dirty.
        segments: Vec<(XPoint, XPoint)>,
    },
    PolyLine {
        drawable: XResourceId,
        gc: XResourceId,
        points: Vec<XPoint>,
    },
    PolyRectangle {
        drawable: XResourceId,
        gc: XResourceId,
        rectangles: Vec<Rect>,
    },
    FillPoly {
        drawable: XResourceId,
        gc: XResourceId,
        shape: u8,
        coordinate_mode: u8,
        points: Vec<XPoint>,
    },
    PolyFillArc {
        drawable: XResourceId,
        gc: XResourceId,
        arcs: Vec<crate::XArc>,
    },
    PolyArc {
        drawable: XResourceId,
        gc: XResourceId,
        arcs: Vec<crate::XArc>,
    },
    PolyPoint {
        drawable: XResourceId,
        gc: XResourceId,
        coordinate_mode: u8,
        points: Vec<XPoint>,
    },
    ShmQueryVersion,
    ShmGetImage {
        drawable: XResourceId,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        plane_mask: u32,
        format: u8,
        segment: XResourceId,
        offset: u32,
    },
    Dri3QueryVersion {
        major_version: u32,
        minor_version: u32,
    },
    Dri3Open {
        drawable: XResourceId,
        provider: u32,
    },
    Dri3PixmapFromBuffer {
        pixmap: XResourceId,
        drawable: XResourceId,
        size_bytes: u32,
        width: u16,
        height: u16,
        stride: u16,
        depth: u8,
        bits_per_pixel: u8,
    },
    Dri3PixmapFromBuffers {
        pixmap: XResourceId,
        window: XResourceId,
        num_buffers: u8,
        width: u16,
        height: u16,
        strides: [u32; sophia_protocol::DMA_BUF_MAX_PLANES],
        offsets: [u32; sophia_protocol::DMA_BUF_MAX_PLANES],
        depth: u8,
        bits_per_pixel: u8,
        modifier: u64,
    },
    Dri3FenceFromFd {
        drawable: XResourceId,
        fence: XResourceId,
        initially_triggered: bool,
    },
    Dri3SetDrmDeviceInUse {
        window: XResourceId,
        major: u32,
        minor: u32,
    },
    Dri3GetSupportedModifiers {
        window: XResourceId,
        depth: u8,
        bits_per_pixel: u8,
    },
    Dri3BufferFromPixmap {
        pixmap: XResourceId,
    },
    Dri3BuffersFromPixmap {
        pixmap: XResourceId,
    },
    /// A DRI3 request Sophia decodes but does not implement.
    ///
    /// Kept as a request rather than a parse failure so the answer is a normal
    /// client-visible X11 error naming its own minor opcode, which is what the
    /// compatibility matrix requires of anything unsupported.
    Dri3Unimplemented {
        minor_opcode: u8,
    },
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
    ShmAttach {
        segment: XResourceId,
        shmid: u32,
        read_only: bool,
    },
    /// MIT-SHM 1.2 `AttachFd`. The descriptor is delivered by the socket
    /// layer rather than carried here, which is why this looks lighter than
    /// `ShmAttach` while doing more.
    ShmAttachFd {
        segment: XResourceId,
        read_only: bool,
    },
    /// MIT-SHM 1.2 `CreateSegment`. The server allocates, and the reply hands
    /// the client a descriptor for the memory.
    ShmCreateSegment {
        segment: XResourceId,
        size: u32,
        read_only: bool,
    },
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
    /// A SHAPE minor no version of the extension defines.
    /// A GLX minor Sophia does not answer. Decoded rather than refused at the
    /// parser, so the client gets a normal error against a sequence number it
    /// can attribute, which a parse failure would deny it.
    GlxUnimplemented {
        minor_opcode: u8,
    },
    ShapeUnimplemented {
        minor_opcode: u8,
    },
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
    ShmDetach {
        segment: XResourceId,
    },
    ShmPutImage {
        drawable: XResourceId,
        gc: XResourceId,
        total_width: u16,
        total_height: u16,
        src_x: u16,
        src_y: u16,
        src_width: u16,
        src_height: u16,
        dst_x: i16,
        dst_y: i16,
        depth: u8,
        format: u8,
        send_event: bool,
        segment: XResourceId,
        offset: u32,
    },
    ShmCreatePixmap {
        pixmap: XResourceId,
        drawable: XResourceId,
        width: u16,
        height: u16,
        depth: u8,
        segment: XResourceId,
        offset: u32,
    },
    RandrQueryVersion {
        major_version: u32,
        minor_version: u32,
    },
    RandrSelectInput {
        window: XResourceId,
        enable: u16,
    },
    RandrGetScreenSizeRange {
        window: XResourceId,
    },
    RandrGetScreenResources {
        window: XResourceId,
        current: bool,
    },
    RandrGetOutputInfo {
        output: u32,
        config_timestamp: u32,
    },
    RandrGetOutputProperty {
        output: u32,
        property: XAtom,
        property_type: XAtom,
        long_offset: u32,
        long_length: u32,
        delete: bool,
        pending: bool,
    },
    RandrGetCrtcInfo {
        crtc: u32,
        config_timestamp: u32,
    },
    RandrGetCrtcGammaSize {
        crtc: u32,
    },
    RandrGetCrtcGamma {
        crtc: u32,
    },
    RandrGetCrtcTransform {
        crtc: u32,
    },
    RandrGetPanning {
        crtc: u32,
    },
    RandrGetOutputPrimary {
        window: XResourceId,
    },
    RandrGetProviders {
        window: XResourceId,
    },
    RandrGetMonitors {
        window: XResourceId,
        get_active: bool,
    },
    XkbUseExtension {
        wanted_major: u16,
        wanted_minor: u16,
    },
    GlxQueryVersion {
        major_version: u32,
        minor_version: u32,
    },
    GlxGetVisualConfigs {
        screen: u32,
    },
    GlxGetFbConfigs {
        screen: u32,
    },
    GlxClientInfo,
    GlxCreateContext {
        context: XResourceId,
        config: XGlxContextConfig,
        screen: u32,
        share: Option<XResourceId>,
        direct: bool,
    },
    GlxDestroyContext {
        context: XResourceId,
    },
    GlxMakeCurrent {
        drawable: Option<XResourceId>,
        context: Option<XResourceId>,
        old_context_tag: u32,
    },
    GlxIsDirect {
        context: XResourceId,
    },
    GlxCreateWindow {
        screen: u32,
        fbconfig: u32,
        window: XResourceId,
        glx_window: XResourceId,
    },
    GlxCreatePbuffer {
        screen: u32,
        fbconfig: u32,
        pbuffer: XResourceId,
        width: u32,
        height: u32,
        /// `GLX_LARGEST_PBUFFER`: take the largest available rather than fail.
        largest: bool,
    },
    GlxDestroyPbuffer {
        pbuffer: XResourceId,
    },
    /// GLX 1.3 `CreatePixmap`: a GLX drawable over an existing X pixmap.
    GlxCreatePixmap {
        screen: u32,
        fbconfig: u32,
        pixmap: XResourceId,
        glx_pixmap: XResourceId,
        /// `GLX_TEXTURE_TARGET_EXT`, where the client named one. Absent means
        /// the server chooses, which the extension allows.
        target: Option<u32>,
        /// `GLX_TEXTURE_FORMAT_EXT`, where named.
        format: Option<u32>,
        /// `GLX_MIPMAP_TEXTURE_EXT`, where named.
        mipmap: Option<bool>,
    },
    /// GLX 1.2 `CreateGLXPixmap`, which names a visual where its successor
    /// names a configuration.
    GlxCreateGlxPixmap {
        screen: u32,
        visual: u32,
        pixmap: XResourceId,
        glx_pixmap: XResourceId,
    },
    /// Both destructors. They differ only in the name a refusal carries.
    GlxDestroyPixmap {
        minor_opcode: u8,
        glx_pixmap: XResourceId,
    },
    GlxQueryContext {
        context: XResourceId,
    },
    GlxChangeDrawableAttributes {
        drawable: XResourceId,
    },
    GlxMakeContextCurrent {
        drawable: XResourceId,
        read_drawable: XResourceId,
        context: Option<XResourceId>,
    },
    GlxDeleteWindow {
        glx_window: XResourceId,
    },
    GlxGetDrawableAttributes {
        drawable: XResourceId,
    },
    SyncInitialize {
        desired_major: u8,
        desired_minor: u8,
    },
    SyncListSystemCounters,
    SyncCreateCounter {
        counter: XResourceId,
        initial_value: i64,
    },
    SyncSetCounter {
        counter: XResourceId,
        value: i64,
    },
    SyncChangeCounter {
        counter: XResourceId,
        delta: i64,
    },
    SyncQueryCounter {
        counter: XResourceId,
    },
    SyncDestroyCounter {
        counter: XResourceId,
    },
    SyncDestroyFence {
        fence: XResourceId,
    },
    GlxQueryExtensionsString,
    GlxQueryServerString {
        name: u32,
    },
    XkbGetMap {
        full: u16,
        partial: u16,
    },
    XkbGetCompatMap {
        device_spec: u16,
    },
    XkbGetIndicatorMap {
        device_spec: u16,
    },
    XkbGetState,
    /// Asked to latch or lock modifiers and the keyboard group.
    ///
    /// Carried whole rather than reduced, because what this instance can
    /// honour is decided where the keyboard state lives, not here.
    XkbLatchLockState {
        affect_mod_locks: u8,
        mod_locks: u8,
        lock_group: bool,
        group_lock: u8,
        affect_mod_latches: u8,
        mod_latches: u8,
        latch_group: bool,
        group_latch: u16,
    },
    XkbGetControls,
    XkbGetNames {
        which: u32,
    },
    XkbGetDeviceInfo {
        device_spec: u16,
        wanted: u16,
    },
    XkbSelectEvents {
        affect_which: u16,
        clear: u16,
        select_all: u16,
        state_details: Option<(u16, u16)>,
    },
    XkbPerClientFlags {
        change: u32,
        value: u32,
    },
    XiQueryVersion {
        major_version: u16,
        minor_version: u16,
    },
    XiQueryPointer {
        window: XResourceId,
        device_id: u16,
    },
    XiGetClientPointer,
    XiDeviceBell,
    XiGrabDevice {
        window: XResourceId,
        time: u32,
        cursor: Option<XResourceId>,
        device_id: u16,
        pointer_mode: u8,
        keyboard_mode: u8,
        owner_events: bool,
        event_mask: Vec<u32>,
    },
    XiUngrabDevice {
        device_id: u16,
        time: u32,
    },
    XiChangeCursor {
        window: XResourceId,
        cursor: Option<XResourceId>,
    },
    XiGetExtensionVersion,
    XiListInputDevices,
    XiQueryDevice {
        device_id: u16,
    },
    XiSelectEvents {
        window: XResourceId,
        masks: Vec<(u16, Vec<u32>)>,
    },
    XiGetFocus {
        device_id: u16,
    },
    XiGetProperty,
    GeQueryVersion {
        major_version: u16,
        minor_version: u16,
    },
    BigRequestsEnable,
    QueryColors {
        colormap: XResourceId,
        pixels: Vec<u32>,
    },
    CreateCursor {
        cursor: XResourceId,
        source: XResourceId,
        mask: Option<XResourceId>,
    },
    CreateGlyphCursor {
        cursor: XResourceId,
        source_font: XResourceId,
        mask_font: Option<XResourceId>,
    },
    FreeCursor {
        cursor: XResourceId,
    },
    RecolorCursor {
        cursor: XResourceId,
    },
    GetModifierMapping,
    GetPointerMapping,
    GetKeyboardMapping {
        first_keycode: u8,
        count: u8,
    },
    GetKeyboardControl,
    Bell,
    /// Mode travels in the header's data byte rather than a body, so the
    /// request is one word long and `mode` is the only thing it carries.
    ForceScreenSaver {
        mode: u8,
    },
    /// Move the pointer, optionally only when it is already inside a
    /// rectangle of the source window. Either window may be `None`, and the
    /// two cases mean different things: no source window is an
    /// unconditional warp, and no destination window makes the destination
    /// an offset from where the pointer already is.
    WarpPointer {
        source: XResourceId,
        destination: XResourceId,
        src_x: i16,
        src_y: i16,
        src_width: u16,
        src_height: u16,
        dst_x: i16,
        dst_y: i16,
    },
    TranslateCoordinates {
        source: XResourceId,
        destination: XResourceId,
        src_x: i16,
        src_y: i16,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XWireParseError {
    Truncated {
        needed: usize,
        actual: usize,
    },
    InvalidLength {
        opcode: u8,
        expected_at_least: usize,
        actual: usize,
    },
    TrailingBytes(usize),
    /// A BIG-REQUESTS frame longer than the maximum the connection accepts,
    /// or shorter than its own header; its body was dropped unread.
    BeyondMaximumLength {
        opcode: u8,
        units: u32,
    },
    UnknownOpcode(u8),
    InvalidPropertyMode(u8),
    InvalidPropertyFormat(u8),
    InvalidEventType(u8),
    InvalidValue(u32),
    PropertyValueTooLarge {
        len: usize,
        max: usize,
    },
    ResourceIdOutsideClientRange {
        resource_id: u32,
    },
}

impl core::fmt::Display for XWireParseError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for XWireParseError {}

/// Read the colormap every one of these requests names, at the same offset.
///
/// `StoreNamedColor` puts its colormap there too, after the one-byte mode.
/// Each colormap request framed as the protocol frames it, so a request one
/// unit long or short is the length error before it is anything else.
fn decode_colormap_request(
    context: XWireClientContext,
    bytes: &[u8],
    opcode: u8,
    kind: XColormapRequestKind,
) -> Result<XWireRequest, XWireParseError> {
    match kind {
        XColormapRequestKind::Install | XColormapRequestKind::Uninstall => {
            require_exact_len(opcode, X_INSTALL_COLORMAP_REQ_LEN, bytes.len())?;
        }
        XColormapRequestKind::AllocCells => {
            require_exact_len(opcode, X_ALLOC_COLOR_CELLS_REQ_LEN, bytes.len())?;
        }
        XColormapRequestKind::AllocPlanes => {
            require_exact_len(opcode, X_ALLOC_COLOR_PLANES_REQ_LEN, bytes.len())?;
        }
        // The plane mask and then pixels, four bytes each.
        XColormapRequestKind::FreeColors => {
            require_len(opcode, X_FREE_COLORS_REQ_LEN, bytes.len())?;
            require_item_multiple(opcode, X_FREE_COLORS_REQ_LEN, 4, bytes.len())?;
        }
        // Colour items of twelve bytes.
        XColormapRequestKind::StoreColors => {
            require_len(opcode, X_STORE_COLORS_REQ_LEN, bytes.len())?;
            require_item_multiple(
                opcode,
                X_STORE_COLORS_REQ_LEN,
                X_STORE_COLORS_ITEM_LEN,
                bytes.len(),
            )?;
        }
        // A name whose length is at 12..14, padded to four.
        XColormapRequestKind::StoreNamedColor => {
            require_len(opcode, X_STORE_NAMED_COLOR_REQ_LEN, bytes.len())?;
            let name_len = usize::from(context.byte_order.u16(&bytes[12..14]));
            require_exact_len(
                opcode,
                X_STORE_NAMED_COLOR_REQ_LEN + ((name_len + 3) & !3),
                bytes.len(),
            )?;
        }
    }
    Ok(XWireRequest::ColormapRequest {
        kind,
        colormap: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
    })
}

pub fn decode_x11_core_request(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    if bytes.len() < 4 {
        return Err(XWireParseError::Truncated {
            needed: 4,
            actual: bytes.len(),
        });
    }

    let opcode = bytes[0];
    let declared_len = usize::from(context.byte_order.u16(&bytes[2..4])) * 4;
    if declared_len < 4 {
        return Err(XWireParseError::InvalidLength {
            opcode,
            expected_at_least: 4,
            actual: declared_len,
        });
    }
    if bytes.len() < declared_len {
        return Err(XWireParseError::Truncated {
            needed: declared_len,
            actual: bytes.len(),
        });
    }
    if bytes.len() > declared_len {
        return Err(XWireParseError::TrailingBytes(bytes.len() - declared_len));
    }

    match opcode {
        X_CREATE_WINDOW => decode_create_window(context, bytes),
        X_CHANGE_WINDOW_ATTRIBUTES => decode_change_window_attributes(context, bytes),
        X_GET_WINDOW_ATTRIBUTES => decode_get_window_attributes(context, bytes),
        X_DESTROY_WINDOW => decode_destroy_window(context, bytes),
        X_DESTROY_SUBWINDOWS => decode_destroy_subwindows(context, bytes),
        X_REPARENT_WINDOW => decode_reparent_window(context, bytes),
        X_MAP_WINDOW => decode_map_window(context, bytes),
        X_MAP_SUBWINDOWS => decode_map_subwindows(context, bytes),
        X_UNMAP_WINDOW => decode_unmap_window(context, bytes),
        X_CONFIGURE_WINDOW => decode_configure_window(context, bytes),
        X_GET_GEOMETRY => decode_get_geometry(context, bytes),
        X_QUERY_TREE => decode_query_tree(context, bytes),
        X_INTERN_ATOM => decode_intern_atom(context, bytes),
        X_GET_ATOM_NAME => decode_get_atom_name(context, bytes),
        X_CHANGE_PROPERTY => decode_change_property(context, bytes),
        X_DELETE_PROPERTY => {
            require_exact_len(X_DELETE_PROPERTY, X_DELETE_PROPERTY_REQ_LEN, bytes.len())?;
            Ok(XWireRequest::DeleteProperty {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                property: context.byte_order.u32(&bytes[8..12]),
            })
        }
        X_GET_PROPERTY => decode_get_property(context, bytes),
        X_LIST_PROPERTIES => decode_list_properties(context, bytes),
        X_SET_SELECTION_OWNER => decode_set_selection_owner(context, bytes),
        X_GET_SELECTION_OWNER => decode_get_selection_owner(context, bytes),
        X_CONVERT_SELECTION => decode_convert_selection(context, bytes),
        X_SEND_EVENT => decode_send_event(context, bytes),
        X_GRAB_POINTER => decode_grab_pointer(context, bytes),
        X_UNGRAB_POINTER => decode_ungrab_pointer(context, bytes),
        X_GRAB_BUTTON => decode_grab_button(context, bytes),
        X_UNGRAB_BUTTON => decode_ungrab_button(context, bytes),
        X_GRAB_KEYBOARD => decode_grab_keyboard(context, bytes),
        X_UNGRAB_KEYBOARD => decode_ungrab_keyboard(context, bytes),
        X_GRAB_KEY => decode_grab_key(context, bytes),
        X_UNGRAB_KEY => decode_ungrab_key(context, bytes),
        X_ALLOW_EVENTS => decode_allow_events(context, bytes),
        X_GRAB_SERVER => decode_grab_server(bytes),
        X_UNGRAB_SERVER => decode_ungrab_server(bytes),
        X_TRANSLATE_COORDINATES => decode_translate_coordinates(context, bytes),
        X_WARP_POINTER => decode_warp_pointer(context, bytes),
        X_QUERY_POINTER => {
            require_exact_len(X_QUERY_POINTER, X_QUERY_POINTER_REQ_LEN, bytes.len())?;
            Ok(XWireRequest::QueryPointer {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
            })
        }
        X_SET_INPUT_FOCUS => decode_set_input_focus(context, bytes),
        X_GET_INPUT_FOCUS => decode_get_input_focus(bytes),
        X_GET_KEYBOARD_CONTROL => {
            require_exact_len(X_GET_KEYBOARD_CONTROL, 4, bytes.len())?;
            Ok(XWireRequest::GetKeyboardControl)
        }
        X_BELL => {
            require_exact_len(X_BELL, 4, bytes.len())?;
            Ok(XWireRequest::Bell)
        }
        X_FORCE_SCREEN_SAVER => {
            require_exact_len(X_FORCE_SCREEN_SAVER, 4, bytes.len())?;
            // An out-of-range mode is a Value error the client must see with
            // its sequence, not a decode failure, so it is carried through.
            Ok(XWireRequest::ForceScreenSaver { mode: bytes[1] })
        }
        X_OPEN_FONT => decode_open_font(context, bytes),
        X_CLOSE_FONT => decode_close_font(context, bytes),
        X_QUERY_FONT => decode_query_font(context, bytes),
        X_QUERY_TEXT_EXTENTS => decode_query_text_extents(context, bytes),
        // Decoded so the refusal is a proper protocol error rather than an
        // unknown opcode. A client that dies on BadRequest -- xterm installs
        // an error handler that exits -- must be able to ask and be told no.
        X_COPY_GC => {
            require_exact_len(X_COPY_GC, X_COPY_GC_REQ_LEN, bytes.len())?;
            let value_mask = context.byte_order.u32(&bytes[12..16]);
            if value_mask & !0x007f_ffff != 0 {
                return Err(XWireParseError::InvalidValue(value_mask));
            }
            Ok(XWireRequest::CopyGraphicsContext {
                source: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                destination: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
                value_mask,
            })
        }
        X_SET_DASHES => {
            require_len(X_SET_DASHES, X_SET_DASHES_REQ_LEN, bytes.len())?;
            let dash_len = usize::from(context.byte_order.u16(&bytes[10..12]));
            if dash_len > X_SET_DASHES_MAX_LEN {
                return Err(XWireParseError::PropertyValueTooLarge {
                    len: dash_len,
                    max: X_SET_DASHES_MAX_LEN,
                });
            }
            let expected = X_SET_DASHES_REQ_LEN + padded_len(dash_len);
            if bytes.len() != expected {
                return Err(XWireParseError::InvalidLength {
                    opcode: X_SET_DASHES,
                    expected_at_least: expected,
                    actual: bytes.len(),
                });
            }
            Ok(XWireRequest::SetDashes {
                gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                dash_offset: context.byte_order.u16(&bytes[8..10]),
                dashes: bytes[X_SET_DASHES_REQ_LEN..X_SET_DASHES_REQ_LEN + dash_len].to_vec(),
            })
        }
        X_SET_FONT_PATH => {
            require_len(X_SET_FONT_PATH, X_SET_FONT_PATH_REQ_LEN, bytes.len())?;
            Ok(XWireRequest::SetFontPath)
        }
        X_GET_FONT_PATH => {
            require_exact_len(X_GET_FONT_PATH, X_GET_FONT_PATH_REQ_LEN, bytes.len())?;
            Ok(XWireRequest::GetFontPath)
        }
        X_LIST_FONTS => decode_list_fonts(context, bytes),
        X_LIST_FONTS_WITH_INFO => decode_list_fonts_with_info(context, bytes),
        X_CREATE_PIXMAP => decode_create_pixmap(context, bytes),
        X_FREE_PIXMAP => decode_free_pixmap(context, bytes),
        X_CREATE_GC => decode_create_gc(context, bytes),
        X_SET_CLIP_RECTANGLES => decode_set_clip_rectangles(context, bytes),
        X_CHANGE_GC => decode_change_gc(context, bytes),
        X_FREE_GC => decode_free_gc(context, bytes),
        X_CLEAR_AREA => decode_clear_area(context, bytes),
        X_COPY_AREA => decode_copy_area(context, bytes),
        X_POLY_LINE => decode_poly_line(context, bytes),
        X_POLY_SEGMENT => decode_poly_segment(context, bytes),
        X_POLY_RECTANGLE => decode_poly_rectangle(context, bytes),
        X_FILL_POLY => decode_fill_poly(context, bytes),
        X_POLY_FILL_RECTANGLE => decode_poly_fill_rectangle(context, bytes),
        X_POLY_FILL_ARC => decode_poly_fill_arc(context, bytes),
        X_POLY_ARC => decode_poly_arc(context, bytes),
        X_POLY_POINT => decode_poly_point(context, bytes),
        X_COPY_PLANE => {
            require_exact_len(X_COPY_PLANE, X_COPY_PLANE_REQ_LEN, bytes.len())?;
            let bit_plane = context.byte_order.u32(&bytes[28..32]);
            // Exactly one plane, as the protocol requires. Zero or several is
            // a bad value rather than a copy of nothing.
            if bit_plane.count_ones() != 1 {
                return Err(XWireParseError::InvalidValue(bit_plane));
            }
            Ok(XWireRequest::CopyPlane {
                source: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                destination: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
                gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[12..16])), 1),
                src_x: context.byte_order.i16(&bytes[16..18]),
                src_y: context.byte_order.i16(&bytes[18..20]),
                dst_x: context.byte_order.i16(&bytes[20..22]),
                dst_y: context.byte_order.i16(&bytes[22..24]),
                width: context.byte_order.u16(&bytes[24..26]),
                height: context.byte_order.u16(&bytes[26..28]),
                bit_plane,
            })
        }
        X_PUT_IMAGE => decode_put_image(context, bytes),
        X_GET_IMAGE => decode_get_image(context, bytes),
        X_POLY_TEXT8 => decode_poly_text8(context, bytes),
        X_POLY_TEXT16 => decode_poly_text16(context, bytes),
        X_IMAGE_TEXT8 => decode_image_text8(context, bytes),
        X_IMAGE_TEXT16 => decode_image_text16(context, bytes),
        X_CREATE_COLORMAP => decode_create_colormap(context, bytes),
        X_FREE_COLORMAP => {
            require_exact_len(X_FREE_COLORMAP, X_FREE_COLORMAP_REQ_LEN, bytes.len())?;
            Ok(XWireRequest::FreeColormap {
                colormap: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
            })
        }
        X_COPY_COLORMAP_AND_FREE => decode_copy_colormap_and_free(context, bytes),
        X_LIST_INSTALLED_COLORMAPS => {
            require_exact_len(
                X_LIST_INSTALLED_COLORMAPS,
                X_LIST_INSTALLED_COLORMAPS_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::ListInstalledColormaps {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
            })
        }
        X_INSTALL_COLORMAP => decode_colormap_request(
            context,
            bytes,
            X_INSTALL_COLORMAP,
            XColormapRequestKind::Install,
        ),
        X_UNINSTALL_COLORMAP => decode_colormap_request(
            context,
            bytes,
            X_UNINSTALL_COLORMAP,
            XColormapRequestKind::Uninstall,
        ),
        X_ALLOC_COLOR_CELLS => decode_colormap_request(
            context,
            bytes,
            X_ALLOC_COLOR_CELLS,
            XColormapRequestKind::AllocCells,
        ),
        X_ALLOC_COLOR_PLANES => decode_colormap_request(
            context,
            bytes,
            X_ALLOC_COLOR_PLANES,
            XColormapRequestKind::AllocPlanes,
        ),
        X_FREE_COLORS => decode_colormap_request(
            context,
            bytes,
            X_FREE_COLORS,
            XColormapRequestKind::FreeColors,
        ),
        X_STORE_COLORS => decode_colormap_request(
            context,
            bytes,
            X_STORE_COLORS,
            XColormapRequestKind::StoreColors,
        ),
        X_STORE_NAMED_COLOR => decode_colormap_request(
            context,
            bytes,
            X_STORE_NAMED_COLOR,
            XColormapRequestKind::StoreNamedColor,
        ),
        X_ALLOC_COLOR => decode_alloc_color(context, bytes),
        X_ALLOC_NAMED_COLOR => decode_named_color(context, bytes),
        X_LOOKUP_COLOR => decode_named_color(context, bytes),
        X_QUERY_COLORS => decode_query_colors(context, bytes),
        X_CREATE_CURSOR => decode_create_cursor(context, bytes),
        X_CREATE_GLYPH_CURSOR => decode_create_glyph_cursor(context, bytes),
        X_FREE_CURSOR => decode_free_cursor(context, bytes),
        X_RECOLOR_CURSOR => decode_recolor_cursor(context, bytes),
        X_QUERY_BEST_SIZE => decode_query_best_size(context, bytes),
        X_QUERY_EXTENSION => decode_query_extension(context, bytes),
        X_LIST_EXTENSIONS => decode_list_extensions(bytes),
        X_GET_KEYBOARD_MAPPING => decode_get_keyboard_mapping(bytes),
        X_GET_POINTER_MAPPING => decode_get_pointer_mapping(bytes),
        X_GET_MODIFIER_MAPPING => decode_get_modifier_mapping(bytes),
        X_SOPHIA_PRESENT_MAJOR_OPCODE => decode_sophia_present(context, bytes),
        X_MIT_SHM_MAJOR_OPCODE => decode_mit_shm(context, bytes),
        X_RANDR_MAJOR_OPCODE => decode_randr(context, bytes),
        X_KEYBOARD_MAJOR_OPCODE => decode_x_keyboard(context, bytes),
        X_BIG_REQUESTS_MAJOR_OPCODE => decode_big_requests(bytes),
        X_INPUT_MAJOR_OPCODE => decode_x_input(context, bytes),
        X_GENERIC_EVENT_MAJOR_OPCODE => {
            // The minor is checked before the length, as every other
            // extension does. Asking the length first answers BadLength for a
            // request this extension does not have at all, which tells the
            // client its own well-formed request was the wrong size.
            if bytes[1] != X_GENERIC_EVENT_QUERY_VERSION_MINOR_OPCODE {
                return Err(XWireParseError::UnknownOpcode(bytes[1]));
            }
            require_exact_len(
                X_GENERIC_EVENT_MAJOR_OPCODE,
                X_GENERIC_EVENT_QUERY_VERSION_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::GeQueryVersion {
                major_version: context.byte_order.u16(&bytes[4..6]),
                minor_version: context.byte_order.u16(&bytes[6..8]),
            })
        }
        // Deliberately no length check beyond the preamble's: NoOperation may
        // carry any amount of padding, and all of it is ignored.
        X_NO_OPERATION => Ok(XWireRequest::NoOperation),
        X_DRI3_MAJOR_OPCODE => decode_dri3(context, bytes),
        X_PRESENT_MAJOR_OPCODE => decode_present(context, bytes),
        X_XFIXES_MAJOR_OPCODE => decode_xfixes(context, bytes),
        X_XF86_VIDMODE_MAJOR_OPCODE => decode_xf86_vidmode(context, bytes),
        X_XC_MISC_MAJOR_OPCODE => decode_xc_misc(context, bytes),
        X_RENDER_MAJOR_OPCODE => decode_render(context, bytes),
        X_SHAPE_MAJOR_OPCODE => decode_shape(context, bytes),
        X_TEST_MAJOR_OPCODE => decode_xtest(context, bytes),
        X_GLX_MAJOR_OPCODE => decode_glx(context, bytes),
        X_SYNC_MAJOR_OPCODE => decode_sync(context, bytes),
        other => Err(XWireParseError::UnknownOpcode(other)),
    }
}
