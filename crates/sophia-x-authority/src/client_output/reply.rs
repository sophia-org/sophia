// Every reply the authority encodes, one variant per request that answers
// with one. Included into `client_output.rs`; split out to keep that file
// within the layout ledger's bound (t026).

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
