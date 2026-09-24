// The decoded core and extension request, one variant per request the
// authority serves. Included into `wire.rs`; split out to keep that file
// within the layout ledger's bound (t026).

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
        /// The border width asked for. Sophia draws no border; the value is a
        /// fact clients read back (GetGeometry, CreateNotify, ConfigureNotify).
        border_width: u16,
        /// Class CopyFromParent: InputOnly under an InputOnly parent.
        copy_class_from_parent: bool,
        /// The bit-gravity and win-gravity attributes, when the request set
        /// them (t199).
        bit_gravity: Option<u8>,
        win_gravity: Option<u8>,
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
        bit_gravity: Option<u8>,
        win_gravity: Option<u8>,
        /// Raw, because zero is CopyFromParent: the parent's colormap.
        colormap: Option<u32>,
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
        border_width: Option<u16>,
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
        /// A value the protocol refuses once the colormap is known: a
        /// contiguity flag other than True or False, or zero colours to
        /// allocate. Decoded here, answered after the colormap's own check.
        invalid_value: Option<u32>,
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
        hotspot_x: u16,
        hotspot_y: u16,
    },
    CreateGlyphCursor {
        cursor: XResourceId,
        source_font: XResourceId,
        mask_font: Option<XResourceId>,
        source_char: u16,
        mask_char: u16,
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
    SetPointerMapping {
        mapping: Vec<u8>,
    },
    ChangeKeyboardMapping {
        first_keycode: u8,
        keysyms_per_keycode: u8,
        keysyms: Vec<u32>,
    },
    SetModifierMapping {
        keycodes_per_modifier: u8,
        keycodes: Vec<u8>,
    },
    QueryKeymap,
    /// Acted on by the socket layer, which owns the leases; the dispatcher
    /// validates the window. `own_window` is the decoder's finding that the
    /// window lies in the requester's own range, which the protocol refuses.
    ChangeSaveSet {
        window: XResourceId,
        mode: XSaveSetMode,
        own_window: bool,
    },
    SetCloseDownMode {
        mode: XCloseDownMode,
    },
    /// `None` is AllTemporary.
    KillClient {
        resource: Option<XResourceId>,
    },
    UnmapSubwindows {
        window: XResourceId,
    },
    CirculateWindow {
        window: XResourceId,
        /// 0 RaiseLowest, 1 LowerHighest.
        direction: u8,
    },
    ChangeActivePointerGrab {
        cursor: XResourceId,
        time: u32,
        event_mask: u16,
    },
    RotateProperties {
        window: XResourceId,
        delta: i16,
        properties: Vec<u32>,
    },
    ChangeKeyboardControl(crate::XKeyboardControlChange),
    ChangePointerControl {
        acceleration_numerator: i16,
        acceleration_denominator: i16,
        threshold: i16,
        do_acceleration: bool,
        do_threshold: bool,
    },
    GetPointerControl,
    SetScreenSaver {
        timeout: i16,
        interval: i16,
        prefer_blanking: u8,
        allow_exposures: u8,
    },
    GetScreenSaver,
    GetMotionEvents {
        window: XResourceId,
        start: u32,
        stop: u32,
    },
    ListHosts,
    /// Decoded for its framing and answered BadAccess: admission is by
    /// namespace and peer credentials, and no client may change a list
    /// that decides nothing.
    ChangeHosts,
    SetAccessControl,
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
