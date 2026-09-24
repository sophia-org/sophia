use crate::{
    X_ATOM_NONE, X_RENDER_FILTER_BEST, X_RENDER_FILTER_BILINEAR, X_RENDER_FILTER_FAST,
    X_RENDER_FILTER_GOOD, X_RENDER_FILTER_NEAREST, X_RENDER_FIRST_ERROR, X_RENDER_FORMAT_A1,
    X_RENDER_FORMAT_A8, X_RENDER_FORMAT_ARGB32, X_RENDER_FORMAT_RGB24, X_RENDER_GLYPH_ERROR_OFFSET,
    X_RENDER_GLYPH_SET_ERROR_OFFSET, X_RENDER_PICT_FORMAT_ERROR_OFFSET,
    X_RENDER_PICT_OP_ERROR_OFFSET, X_RENDER_PICT_TYPE_DIRECT, X_RENDER_PICTURE_ERROR_OFFSET,
    X_SETUP_ARGB_VISUAL, X_SETUP_DEFAULT_VISUAL, XAuthorityRuntimeError, XByteOrder, XColorRgb16,
    XResourceId, XTimestamp, XWireParseError, padded_len,
};
use sophia_protocol::Rect;

include!("client_output/replies/core_early.rs");
include!("client_output/replies/core_late.rs");
include!("client_output/replies/glx_sync.rs");
include!("client_output/replies/randr.rs");
include!("client_output/replies/render_extensions.rs");
include!("client_output/replies/x_render.rs");
include!("client_output/replies/xi.rs");
include!("client_output/replies/xkb.rs");
include!("client_output/errors.rs");
include!("client_output/events.rs");
include!("client_output/helpers.rs");
include!("client_output/reply_records.rs");

pub const X_CLIENT_OUTPUT_RECORD_LEN: usize = 32;

pub(crate) const X_KEY_PRESS: u8 = 2;
pub(crate) const X_KEY_RELEASE: u8 = 3;
pub(crate) const X_BUTTON_PRESS: u8 = 4;
pub(crate) const X_BUTTON_RELEASE: u8 = 5;
pub(crate) const X_MOTION_NOTIFY: u8 = 6;
const X_FOCUS_IN: u8 = 9;
const X_FOCUS_OUT: u8 = 10;
const X_EXPOSE: u8 = 12;
const X_GRAPHICS_EXPOSE: u8 = 13;
const X_NO_EXPOSE: u8 = 14;
const X_VISIBILITY_NOTIFY: u8 = 15;
const X_DESTROY_NOTIFY: u8 = 17;
const X_UNMAP_NOTIFY: u8 = 18;
const X_MAP_NOTIFY: u8 = 19;
const X_MAP_REQUEST: u8 = 20;
const X_MAPPING_NOTIFY: u8 = 34;
const X_REPARENT_NOTIFY: u8 = 21;
const X_CONFIGURE_NOTIFY: u8 = 22;
const X_CIRCULATE_NOTIFY: u8 = 26;
const X_CIRCULATE_REQUEST: u8 = 27;
const X_PROPERTY_NOTIFY: u8 = 28;
const X_SELECTION_NOTIFY: u8 = 31;

const PROPERTY_NEW_VALUE: u8 = 0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XClientEvent {
    Key {
        sequence: u16,
        pressed: bool,
        keycode: u8,
        time: XTimestamp,
        root: XResourceId,
        event: XResourceId,
        state: u16,
    },
    Focus {
        sequence: u16,
        focused: bool,
        detail: u8,
        event: XResourceId,
        mode: u8,
    },
    XkbStateNotify {
        sequence: u16,
        time: XTimestamp,
        modifiers: u8,
        changed: u16,
        keycode: u8,
        event_type: u8,
    },
    PointerMotion {
        sequence: u16,
        time: XTimestamp,
        root: XResourceId,
        event: XResourceId,
        root_x: i16,
        root_y: i16,
        event_x: i16,
        event_y: i16,
        state: u16,
    },
    PointerButton {
        sequence: u16,
        pressed: bool,
        button: u8,
        time: XTimestamp,
        root: XResourceId,
        event: XResourceId,
        root_x: i16,
        root_y: i16,
        event_x: i16,
        event_y: i16,
        state: u16,
    },
    PointerCrossing {
        sequence: u16,
        entered: bool,
        detail: u8,
        time: XTimestamp,
        root: XResourceId,
        event: XResourceId,
        root_x: i16,
        root_y: i16,
        event_x: i16,
        event_y: i16,
        state: u16,
        mode: u8,
        focus: bool,
    },
    Expose {
        sequence: u16,
        window: XResourceId,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        count: u16,
    },
    /// One destination rectangle a CopyArea or CopyPlane could not fill
    /// because its source lay outside the source drawable. `count` is the
    /// number of rectangles still to come for the same request.
    GraphicsExpose {
        sequence: u16,
        drawable: XResourceId,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        minor_opcode: u16,
        count: u16,
        major_opcode: u8,
    },
    NoExpose {
        sequence: u16,
        drawable: XResourceId,
        minor_opcode: u16,
        major_opcode: u8,
    },
    VisibilityNotify {
        sequence: u16,
        window: XResourceId,
        state: u8,
    },
    CreateNotify {
        sequence: u16,
        parent: XResourceId,
        window: XResourceId,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        border_width: u16,
        override_redirect: bool,
    },
    MapNotify {
        sequence: u16,
        event: XResourceId,
        window: XResourceId,
        override_redirect: bool,
    },
    /// A map was redirected to whoever is managing the parent.
    ///
    /// A client selecting `SubstructureRedirect` on a window is asking to
    /// decide what happens to its children, so a map of one is turned into
    /// this request and the window is not mapped. It is how a window manager
    /// gets to place a window before it appears, and a server that maps
    /// anyway leaves the manager describing a layout that already happened.
    MapRequest {
        sequence: u16,
        parent: XResourceId,
        window: XResourceId,
    },
    /// A window was destroyed.
    ///
    /// `event` is the window the record is addressed to and `window` is the one
    /// destroyed. They differ for a `SubstructureNotify` selector on the
    /// parent, which is why both are carried rather than one being derived.
    DestroyNotify {
        sequence: u16,
        event: XResourceId,
        window: XResourceId,
    },
    UnmapNotify {
        sequence: u16,
        event: XResourceId,
        window: XResourceId,
        from_configure: bool,
    },
    /// A window moved to the top or bottom of its siblings by CirculateWindow:
    /// to the window (StructureNotify) and its parent (SubstructureNotify).
    CirculateNotify {
        sequence: u16,
        event: XResourceId,
        window: XResourceId,
        /// 0 Top, 1 Bottom.
        place: u8,
    },
    /// A circulate a client selecting SubstructureRedirect on the parent has
    /// asked to decide, as MapRequest is to a map.
    CirculateRequest {
        sequence: u16,
        parent: XResourceId,
        window: XResourceId,
        place: u8,
    },
    /// A window given a new parent: by a departing client's save-set
    /// (t166), which reparents what it saved to the nearest survivor.
    ReparentNotify {
        sequence: u16,
        event: XResourceId,
        window: XResourceId,
        parent: XResourceId,
        x: i16,
        y: i16,
        override_redirect: bool,
    },
    /// The keyboard, modifier or pointer mapping changed (request 0, 1, 2),
    /// told to every client, as the protocol does not let it be unselected.
    MappingNotify {
        sequence: u16,
        request: u8,
        first_keycode: u8,
        count: u8,
    },
    ConfigureNotify {
        sequence: u16,
        synthetic: bool,
        event: XResourceId,
        window: XResourceId,
        above_sibling: Option<XResourceId>,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        border_width: u16,
        override_redirect: bool,
    },
    PropertyNotify {
        sequence: u16,
        window: XResourceId,
        atom: u32,
        time: XTimestamp,
        new_value: bool,
    },
    SelectionClear {
        sequence: u16,
        time: XTimestamp,
        owner: XResourceId,
        selection: u32,
    },
    SelectionRequest {
        sequence: u16,
        time: XTimestamp,
        owner: XResourceId,
        requestor: XResourceId,
        selection: u32,
        target: u32,
        property: u32,
    },
    SelectionNotify {
        sequence: u16,
        synthetic: bool,
        time: XTimestamp,
        requestor: XResourceId,
        selection: u32,
        target: u32,
        property: u32,
    },
    ClientMessage {
        sequence: u16,
        bytes: [u8; X_CLIENT_OUTPUT_RECORD_LEN],
    },
    /// `XFixesSelectionNotify`: a selection's ownership changed.
    ///
    /// `window` is the window the recipient named when it subscribed, not the
    /// selection's owner: a watcher asks about a selection through a window of
    /// its own, and the event comes back addressed to that window.
    ///
    /// `subtype` says what changed the ownership -- it was set (0), the owner
    /// window was destroyed (1), or the owning client went away (2) -- and a
    /// subscriber receives only the subtypes its mask selected, bit `1 <<
    /// subtype`.
    XfixesSelectionNotify {
        sequence: u16,
        subtype: u8,
        window: XResourceId,
        owner: XResourceId,
        selection: u32,
        time: XTimestamp,
        selection_time: XTimestamp,
    },
    /// `ShapeNotify`: one of a window's shapes changed.
    ///
    /// `shaped` reports whether the kind is set at all, not whether the
    /// region has area -- a client that sets an empty shape has shaped its
    /// window, and the extents are then zero.
    ShapeNotify {
        sequence: u16,
        kind: u8,
        window: XResourceId,
        extents: Rect,
        shaped: bool,
    },
    ShmCompletion {
        sequence: u16,
        drawable: XResourceId,
        segment: XResourceId,
        offset: u32,
    },
    PresentConfigureNotify {
        sequence: u16,
        event_id: XResourceId,
        window: XResourceId,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        pixmap_width: u16,
        pixmap_height: u16,
        pixmap_flags: u32,
    },
    PresentCompleteNotify {
        sequence: u16,
        event_id: XResourceId,
        window: XResourceId,
        serial: u32,
        ust: u64,
        msc: u64,
        /// 0 = a presented pixmap completed; 1 = an MSC notification.
        kind: u8,
        mode: u8,
    },
    PresentIdleNotify {
        sequence: u16,
        event_id: XResourceId,
        window: XResourceId,
        serial: u32,
        pixmap: XResourceId,
        idle_fence: Option<XResourceId>,
    },
    RandrScreenChange {
        sequence: u16,
        timestamp: u32,
        config_timestamp: u32,
        root: XResourceId,
        request_window: XResourceId,
        width: u16,
        height: u16,
        mm_width: u16,
        mm_height: u16,
    },
    RandrCrtcChange {
        sequence: u16,
        timestamp: u32,
        window: XResourceId,
        crtc: u32,
        mode: u32,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
    },
    RandrOutputChange {
        sequence: u16,
        timestamp: u32,
        window: XResourceId,
        output: u32,
        crtc: u32,
        mode: u32,
    },
    RandrResourceChange {
        sequence: u16,
        timestamp: u32,
        window: XResourceId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XClientReply {
    GrabStatus {
        sequence: u16,
        status: u8,
    },
    InternAtom {
        sequence: u16,
        atom: u32,
    },
    GetAtomName {
        sequence: u16,
        name: String,
    },
    GetGeometry {
        sequence: u16,
        depth: u8,
        root: XResourceId,
        geometry: Rect,
        border_width: u16,
    },
    GetImage {
        sequence: u16,
        depth: u8,
        visual: u32,
        data: Vec<u8>,
    },
    QueryTree {
        sequence: u16,
        root: XResourceId,
        parent: XResourceId,
        children: Vec<XResourceId>,
    },
    GetWindowAttributes {
        sequence: u16,
        visual: u32,
        colormap: XResourceId,
        map_state: u8,
        override_redirect: bool,
    },
    QueryExtension {
        sequence: u16,
        present: bool,
        major_opcode: u8,
        first_event: u8,
        first_error: u8,
    },
    ListExtensions {
        sequence: u16,
        names: Vec<String>,
    },
    ListFonts {
        sequence: u16,
        names: Vec<String>,
    },
    ListFontsWithInfo {
        sequence: u16,
        /// Each name with the metrics of the face it resolves to.
        names: Vec<(String, Box<crate::XFontMetrics>)>,
    },
    QueryBestSize {
        sequence: u16,
        width: u16,
        height: u16,
    },
    ShmQueryVersion {
        sequence: u16,
        major_version: u16,
        minor_version: u16,
        shared_pixmaps: bool,
        pixmap_format: u8,
    },
    ShmGetImage {
        sequence: u16,
        depth: u8,
        visual: u32,
        size: u32,
    },
    Dri3QueryVersion {
        sequence: u16,
        major_version: u32,
        minor_version: u32,
    },
    Dri3Open {
        sequence: u16,
    },
    XCMiscGetVersion {
        sequence: u16,
        major_version: u16,
        minor_version: u16,
    },
    /// A block of identifiers a client may use, or `count: 0` meaning none are
    /// available -- which the protocol defines and clients handle, unlike an
    /// invented range that would collide with another client's resources.
    XCMiscGetXIDRange {
        sequence: u16,
        start_id: u32,
        count: u32,
    },
    XCMiscGetXIDList {
        sequence: u16,
        ids: Vec<u32>,
    },
    RenderQueryVersion {
        sequence: u16,
        major_version: u32,
        minor_version: u32,
    },
    /// The filters this server offers and the aliases onto them.
    ///
    /// Carries only the sequence: which filters exist is a property of the
    /// server, so the encoder owns the table.
    RenderQueryFilters {
        sequence: u16,
    },
    /// The four picture formats and the visual each belongs to.
    ///
    /// Carries only the sequence: the formats are the pixel layouts this
    /// server can represent, which is a property of the server rather than of
    /// any request, so the encoder owns the table.
    RenderQueryPictFormats {
        sequence: u16,
    },
    XF86VidModeQueryVersion {
        sequence: u16,
        major_version: u16,
        minor_version: u16,
    },
    /// The modeline of the screen's primary output.
    ///
    /// Carries the timing rather than a summary of it, because the client
    /// computing a refresh rate from this wants `clock / (htotal * vtotal)`
    /// exactly -- that is the whole reason the request exists.
    XF86VidModeGetModeLine {
        sequence: u16,
        timing: sophia_protocol::OutputModeTiming,
    },
    /// `CreateSegment`: the body says nothing, and the descriptor beside it
    /// says everything. The socket layer supplies that descriptor.
    ShmCreateSegment {
        sequence: u16,
    },
    Dri3GetSupportedModifiers {
        sequence: u16,
        window_modifiers: Vec<u64>,
        screen_modifiers: Vec<u64>,
    },
    /// `BufferFromPixmap`: the single-plane recovery of an imported pixmap.
    ///
    /// A separate record from `Dri3BuffersFromPixmap` because the wire replies
    /// are separate shapes, not one shape with a flag -- this one carries a
    /// total byte length and a single u16 stride where the other carries
    /// per-plane lists and a modifier.
    Dri3BufferFromPixmap {
        sequence: u16,
        size_bytes: u32,
        width: u16,
        height: u16,
        stride: u16,
        depth: u8,
        bits_per_pixel: u8,
    },
    /// `BuffersFromPixmap`: the modifier-aware, per-plane recovery.
    ///
    /// `strides` and `offsets` are the same length, and that length is the
    /// `nfd` the reply header promises. The descriptors themselves travel out
    /// of band rather than in this record.
    Dri3BuffersFromPixmap {
        sequence: u16,
        width: u16,
        height: u16,
        modifier: u64,
        depth: u8,
        bits_per_pixel: u8,
        strides: Vec<u32>,
        offsets: Vec<u32>,
    },
    /// `FetchRegion`: the region's extents, then its rectangles in the
    /// canonical YX-banded order the store already keeps them in.
    ShapeQueryVersion {
        sequence: u16,
        major_version: u16,
        minor_version: u16,
    },
    /// XTEST's version, which is a constant rather than a negotiation.
    ///
    /// The fields sit where the protocol puts them and not where the request
    /// puts them: the major occupies the reply's detail byte and the minor
    /// starts at byte eight. Carrying them as named fields keeps that
    /// asymmetry in the encoder rather than in every caller.
    XTestGetVersion {
        sequence: u16,
        major_version: u8,
        minor_version: u16,
    },
    /// `XTestCompareCursor`. One bit, and it rides the detail byte.
    XTestCompareCursor {
        sequence: u16,
        same: bool,
    },
    ShapeQueryExtents {
        sequence: u16,
        bounding_shaped: bool,
        clip_shaped: bool,
        bounding_extents: Rect,
        clip_extents: Rect,
    },
    ShapeInputSelected {
        sequence: u16,
        enabled: bool,
    },
    /// The rectangles of one kind, in the canonical order the store keeps
    /// them in -- so the ordering this reply claims is one it can honour.
    ShapeGetRectangles {
        sequence: u16,
        ordering: u8,
        rects: Vec<Rect>,
    },
    XfixesFetchRegion {
        sequence: u16,
        extents: Rect,
        rects: Vec<Rect>,
    },
    XfixesQueryVersion {
        sequence: u16,
        major_version: u32,
        minor_version: u32,
    },
    PresentQueryVersion {
        sequence: u16,
        major_version: u32,
        minor_version: u32,
    },
    PresentQueryCapabilities {
        sequence: u16,
        capabilities: u32,
    },
    RandrQueryVersion {
        sequence: u16,
        major_version: u32,
        minor_version: u32,
    },
    RandrGetScreenSizeRange {
        sequence: u16,
        min_width: u16,
        min_height: u16,
        max_width: u16,
        max_height: u16,
    },
    RandrGetScreenResources {
        sequence: u16,
        timestamp: u32,
        crtcs: Vec<u32>,
        outputs: Vec<u32>,
        modes: Vec<XRandrModeInfo>,
    },
    RandrGetOutputInfo {
        sequence: u16,
        timestamp: u32,
        crtc: u32,
        mm_width: u32,
        mm_height: u32,
        crtcs: Vec<u32>,
        modes: Vec<u32>,
        name: Vec<u8>,
    },
    RandrGetOutputProperty {
        sequence: u16,
        property_type: u32,
        bytes_after: u32,
        format: u8,
        data: Vec<u8>,
    },
    RandrGetCrtcInfo {
        sequence: u16,
        timestamp: u32,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        mode: u32,
        outputs: Vec<u32>,
    },
    RandrGetCrtcGammaSize {
        sequence: u16,
        size: u16,
    },
    RandrGetCrtcGamma {
        sequence: u16,
    },
    RandrGetCrtcTransform {
        sequence: u16,
    },
    RandrGetPanning {
        sequence: u16,
        timestamp: u32,
    },
    RandrGetOutputPrimary {
        sequence: u16,
        output: u32,
    },
    RandrGetProviders {
        sequence: u16,
        timestamp: u32,
    },
    RandrGetMonitors {
        sequence: u16,
        timestamp: u32,
        monitors: Vec<XRandrMonitorInfo>,
    },
    XkbUseExtension {
        sequence: u16,
        supported: bool,
        server_major: u16,
        server_minor: u16,
    },
    GlxQueryVersion {
        sequence: u16,
        major_version: u32,
        minor_version: u32,
    },
    GlxString {
        sequence: u16,
        value: String,
    },
    GlxVisualConfigs {
        sequence: u16,
        configs: Vec<[u32; 18]>,
    },
    GlxFbConfigs {
        sequence: u16,
        configs: Vec<Vec<(u32, u32)>>,
    },
    GlxIsDirect {
        sequence: u16,
        direct: bool,
    },
    GlxMakeCurrent {
        sequence: u16,
        context_tag: u32,
    },
    GlxDrawableAttributes {
        sequence: u16,
        attributes: Vec<(u32, u32)>,
    },
    SyncInitialize {
        sequence: u16,
        major_version: u8,
        minor_version: u8,
    },
    SyncListSystemCounters {
        sequence: u16,
    },
    SyncQueryCounter {
        sequence: u16,
        value: i64,
    },
    XkbGetMap {
        sequence: u16,
        present: u16,
        keysyms: Vec<[u32; 2]>,
        modifier_map: Vec<(u8, u8)>,
    },
    XkbGetCompatMap {
        sequence: u16,
        device_id: u8,
    },
    XkbGetIndicatorMap {
        sequence: u16,
        device_id: u8,
    },
    XkbGetState {
        sequence: u16,
        modifiers: u8,
    },
    XkbGetControls {
        sequence: u16,
    },
    XkbGetNames {
        sequence: u16,
        which: u32,
        min_keycode: u8,
        max_keycode: u8,
        component_atoms: Vec<u32>,
        type_atoms: Vec<u32>,
        /// Two per key type, in type order, and never atom None.
        level_atoms: Vec<u32>,
        key_names: Vec<[u8; 4]>,
    },
    XkbGetDeviceInfo {
        sequence: u16,
        device_id: u8,
        supported: u16,
        unsupported: u16,
    },
    XkbPerClientFlags {
        sequence: u16,
        supported: u32,
        value: u32,
    },
    XiQueryVersion {
        sequence: u16,
        major_version: u16,
        minor_version: u16,
    },
    GeQueryVersion {
        sequence: u16,
        major_version: u16,
        minor_version: u16,
    },
    XiGetClientPointer {
        sequence: u16,
        device_id: u16,
    },
    XiGetExtensionVersion {
        sequence: u16,
        server_major: u16,
        server_minor: u16,
    },
    XiQueryDevice {
        sequence: u16,
        devices: Vec<XXiDeviceInfo>,
    },
    XiListInputDevices {
        sequence: u16,
        devices: Vec<XXiLegacyDeviceInfo>,
    },
    XiQueryPointer {
        sequence: u16,
        root: XResourceId,
        child: XResourceId,
        root_x: i16,
        root_y: i16,
        win_x: i16,
        win_y: i16,
        buttons: u32,
        modifiers: u16,
    },
    XiGetFocus {
        sequence: u16,
        focus: XResourceId,
    },
    XiGetProperty {
        sequence: u16,
    },
    BigRequestsEnable {
        sequence: u16,
        maximum_request_length: u32,
    },
    GetInputFocus {
        sequence: u16,
        focus: XResourceId,
        revert_to: u8,
    },
    QueryPointer {
        sequence: u16,
        root: XResourceId,
        child: XResourceId,
        root_x: i16,
        root_y: i16,
        win_x: i16,
        win_y: i16,
        mask: u16,
    },
    GetModifierMapping {
        sequence: u16,
        keycodes_per_modifier: u8,
        keycodes: Vec<u8>,
    },
    GetPointerMapping {
        sequence: u16,
        mapping: Vec<u8>,
    },
    GetKeyboardMapping {
        sequence: u16,
        keysyms_per_keycode: u8,
        keysyms: Vec<u32>,
    },
    GetKeyboardControl {
        sequence: u16,
        keyboard: crate::XKeyboardControl,
    },
    /// SetPointerMapping's and SetModifierMapping's status: 0 Success,
    /// 1 Busy, 2 Failed.
    MappingStatus {
        sequence: u16,
        status: u8,
    },
    QueryKeymap {
        sequence: u16,
        keys: [u8; 32],
    },
    GetPointerControl {
        sequence: u16,
        pointer: crate::XPointerControl,
    },
    GetScreenSaver {
        sequence: u16,
        screen_saver: crate::XScreenSaverControl,
    },
    /// This authority keeps no motion history: the reply carries no events,
    /// which the protocol allows.
    GetMotionEvents {
        sequence: u16,
    },
    /// An empty list with access control enabled: admission is by namespace
    /// and peer credentials, and there is no list to report.
    ListHosts {
        sequence: u16,
    },
    TranslateCoordinates {
        sequence: u16,
        same_screen: bool,
        child: Option<XResourceId>,
        dst_x: i16,
        dst_y: i16,
    },
    QueryFont {
        sequence: u16,
        metrics: Box<crate::XFontMetrics>,
    },
    QueryTextExtents {
        sequence: u16,
        extents: crate::XTextExtents,
    },
    GetFontPath {
        sequence: u16,
        directories: Vec<String>,
    },
    GetProperty {
        sequence: u16,
        property_type: u32,
        format: u8,
        bytes_after: u32,
        item_count: u32,
        bytes: Vec<u8>,
    },
    GetSelectionOwner {
        sequence: u16,
        owner: Option<XResourceId>,
    },
    AllocNamedColor {
        sequence: u16,
        pixel: u32,
        exact: XColorRgb16,
        screen: XColorRgb16,
    },
    LookupColor {
        sequence: u16,
        exact: XColorRgb16,
        screen: XColorRgb16,
    },
    AllocColor {
        sequence: u16,
        pixel: u32,
        red: u16,
        green: u16,
        blue: u16,
    },
    ListProperties {
        sequence: u16,
        atoms: Vec<u32>,
    },
    ListInstalledColormaps {
        sequence: u16,
        colormaps: Vec<u32>,
    },
    QueryColors {
        sequence: u16,
        colors: Vec<XColorRgb16>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XClientOutput {
    Error(XClientError),
    Event(XClientEvent),
    Reply(XClientReply),
}

pub fn encode_x_client_output(byte_order: XByteOrder, output: XClientOutput) -> Vec<u8> {
    match output {
        XClientOutput::Error(error) => encode_x_client_error(byte_order, error).to_vec(),
        XClientOutput::Event(event) => encode_x_client_event(byte_order, event).to_vec(),
        XClientOutput::Reply(reply) => encode_x_client_reply(byte_order, reply),
    }
}

pub fn encode_x_client_reply(byte_order: XByteOrder, reply: XClientReply) -> Vec<u8> {
    let reply = match encode_core_early_reply(byte_order, reply) {
        Ok(bytes) => return bytes,
        Err(reply) => reply,
    };
    let reply = match encode_render_extension_reply(byte_order, reply) {
        Ok(bytes) => return bytes,
        Err(reply) => reply,
    };
    let reply = match encode_x_render_reply(byte_order, reply) {
        Ok(bytes) => return bytes,
        Err(reply) => reply,
    };
    let reply = match encode_randr_reply(byte_order, reply) {
        Ok(bytes) => return bytes,
        Err(reply) => reply,
    };
    let reply = match encode_xkb_reply(byte_order, reply) {
        Ok(bytes) => return bytes,
        Err(reply) => reply,
    };
    let reply = match encode_glx_sync_reply(byte_order, reply) {
        Ok(bytes) => return bytes,
        Err(reply) => reply,
    };
    let reply = match encode_x_input_reply(byte_order, reply) {
        Ok(bytes) => return bytes,
        Err(reply) => reply,
    };
    match encode_core_late_reply(byte_order, reply) {
        Ok(bytes) => bytes,
        Err(_) => unreachable!("reply escaped its family encoder"),
    }
}
