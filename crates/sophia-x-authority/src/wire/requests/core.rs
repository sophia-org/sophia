/// Decoded Core requests; payloads retain their protocol representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XCoreRequest {
    CreateWindow {
        packet: XAuthorityRequestPacket,
        parent: XResourceId,
        depth: u8,
        visual: u32,
        colormap: Option<XResourceId>,
        background_pixmap: Option<crate::XWindowBackground>,
        background_pixel: Option<u32>,
        /// Raw: zero is CopyFromParent. Sophia draws no borders, so these
        /// are validated and kept nowhere (t216).
        border_pixmap: Option<u32>,
        border_pixel: Option<u32>,
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
        /// Raw: zero is CopyFromParent. Sophia draws no borders, so these
        /// are validated and kept nowhere (t216).
        border_pixmap: Option<u32>,
        border_pixel: Option<u32>,
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
        /// Children the socket layer withholds from this map because another
        /// client manages the parent: each becomes a MapRequest to that
        /// client instead. Empty at decode.
        withheld: Vec<XResourceId>,
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
    FreeColors {
        colormap: XResourceId,
        plane_mask: u32,
        pixels: Vec<u32>,
    },
    /// A new colormap on the source's visual, taking this client's allocations.
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
