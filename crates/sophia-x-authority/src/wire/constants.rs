const X_CREATE_WINDOW: u8 = 1;
const X_CHANGE_WINDOW_ATTRIBUTES: u8 = 2;
const X_GET_WINDOW_ATTRIBUTES: u8 = 3;
const X_DESTROY_WINDOW: u8 = 4;
const X_DESTROY_SUBWINDOWS: u8 = 5;
const X_REPARENT_WINDOW: u8 = 7;
const X_MAP_WINDOW: u8 = 8;
const X_MAP_SUBWINDOWS: u8 = 9;
const X_UNMAP_WINDOW: u8 = 10;
const X_CONFIGURE_WINDOW: u8 = 12;
const X_GET_GEOMETRY: u8 = 14;
const X_QUERY_TREE: u8 = 15;
const X_INTERN_ATOM: u8 = 16;
const X_GET_ATOM_NAME: u8 = 17;
const X_CHANGE_PROPERTY: u8 = 18;
const X_DELETE_PROPERTY: u8 = 19;
const X_GET_PROPERTY: u8 = 20;
const X_QUERY_POINTER: u8 = 38;
const X_LIST_PROPERTIES: u8 = 21;
const X_SET_SELECTION_OWNER: u8 = 22;
const X_GET_SELECTION_OWNER: u8 = 23;
const X_CONVERT_SELECTION: u8 = 24;
const X_SEND_EVENT: u8 = 25;
const X_GRAB_POINTER: u8 = 26;
const X_UNGRAB_POINTER: u8 = 27;
const X_GRAB_BUTTON: u8 = 28;
const X_UNGRAB_BUTTON: u8 = 29;
const X_GRAB_KEYBOARD: u8 = 31;
const X_UNGRAB_KEYBOARD: u8 = 32;
const X_GRAB_KEY: u8 = 33;
const X_UNGRAB_KEY: u8 = 34;
const X_ALLOW_EVENTS: u8 = 35;
const X_GRAB_SERVER: u8 = 36;
const X_UNGRAB_SERVER: u8 = 37;
const X_TRANSLATE_COORDINATES: u8 = 40;
const X_SET_INPUT_FOCUS: u8 = 42;
const X_GET_INPUT_FOCUS: u8 = 43;
const X_OPEN_FONT: u8 = 45;
const X_CLOSE_FONT: u8 = 46;
const X_SET_FONT_PATH: u8 = 51;
const X_GET_FONT_PATH: u8 = 52;
const X_QUERY_FONT: u8 = 47;
const X_QUERY_TEXT_EXTENTS: u8 = 48;
const X_LIST_FONTS: u8 = 49;
const X_LIST_FONTS_WITH_INFO: u8 = 50;
const X_CREATE_PIXMAP: u8 = 53;
const X_FREE_PIXMAP: u8 = 54;
const X_CREATE_GC: u8 = 55;
const X_CHANGE_GC: u8 = 56;
const X_COPY_GC: u8 = 57;
const X_SET_DASHES: u8 = 58;
const X_SET_CLIP_RECTANGLES: u8 = 59;
const X_FREE_GC: u8 = 60;
const X_CLEAR_AREA: u8 = 61;
const X_COPY_AREA: u8 = 62;
const X_POLY_LINE: u8 = 65;
const X_POLY_SEGMENT: u8 = 66;
const X_POLY_RECTANGLE: u8 = 67;
const X_FILL_POLY: u8 = 69;
const X_POLY_FILL_RECTANGLE: u8 = 70;
const X_COPY_PLANE: u8 = 63;
const X_POLY_POINT: u8 = 64;
const X_POLY_ARC: u8 = 68;
const X_POLY_FILL_ARC: u8 = 71;
const X_PUT_IMAGE: u8 = 72;
const X_GET_IMAGE: u8 = 73;
const X_POLY_TEXT8: u8 = 74;
const X_POLY_TEXT16: u8 = 75;
const X_IMAGE_TEXT8: u8 = 76;
const X_IMAGE_TEXT16: u8 = 77;
const X_CREATE_COLORMAP: u8 = 78;
const X_FREE_COLORMAP: u8 = 79;
const X_ALLOC_COLOR: u8 = 84;
const X_ALLOC_NAMED_COLOR: u8 = 85;
const X_LOOKUP_COLOR: u8 = 92;
const X_QUERY_COLORS: u8 = 91;
const X_CREATE_CURSOR: u8 = 93;
const X_CREATE_GLYPH_CURSOR: u8 = 94;
const X_FREE_CURSOR: u8 = 95;
const X_RECOLOR_CURSOR: u8 = 96;
const X_QUERY_EXTENSION: u8 = 98;
const X_LIST_EXTENSIONS: u8 = 99;
const X_GET_KEYBOARD_MAPPING: u8 = 101;
const X_GET_KEYBOARD_CONTROL: u8 = 103;
const X_BELL: u8 = 104;
const X_GET_POINTER_MAPPING: u8 = 117;
const X_QUERY_BEST_SIZE: u8 = 97;
const X_GET_MODIFIER_MAPPING: u8 = 119;
/// Does nothing. Its length field is deliberately unconstrained: clients pad
/// with it to align what follows, so the request is frequently longer than its
/// header and every extra byte is ignored.
const X_NO_OPERATION: u8 = 127;

pub const X_SOPHIA_PRESENT_EXTENSION_NAME: &str = "SOPHIA-PRESENT";
pub const X_SOPHIA_PRESENT_MAJOR_OPCODE: u8 = 130;
pub const X_SOPHIA_PRESENT_PIXMAP_MINOR_OPCODE: u8 = 0;
pub const X_MIT_SHM_EXTENSION_NAME: &str = "MIT-SHM";
pub const X_MIT_SHM_MAJOR_OPCODE: u8 = 131;
pub const X_MIT_SHM_FIRST_EVENT: u8 = 108;
pub const X_MIT_SHM_QUERY_VERSION_MINOR_OPCODE: u8 = 0;
pub const X_MIT_SHM_ATTACH_MINOR_OPCODE: u8 = 1;
pub const X_MIT_SHM_DETACH_MINOR_OPCODE: u8 = 2;
pub const X_MIT_SHM_PUT_IMAGE_MINOR_OPCODE: u8 = 3;
pub const X_MIT_SHM_GET_IMAGE_MINOR_OPCODE: u8 = 4;
pub const X_MIT_SHM_CREATE_PIXMAP_MINOR_OPCODE: u8 = 5;
/// MIT-SHM 1.2: the client passes a descriptor instead of a SysV id.
pub const X_MIT_SHM_ATTACH_FD_MINOR_OPCODE: u8 = 6;
/// MIT-SHM 1.2: the server allocates and hands the descriptor back.
pub const X_MIT_SHM_CREATE_SEGMENT_MINOR_OPCODE: u8 = 7;
pub const X_RANDR_EXTENSION_NAME: &str = "RANDR";
pub const X_RANDR_MAJOR_OPCODE: u8 = 132;
pub const X_RANDR_FIRST_EVENT: u8 = 64;
pub const X_RANDR_QUERY_VERSION_MINOR_OPCODE: u8 = 0;
pub const X_RANDR_SELECT_INPUT_MINOR_OPCODE: u8 = 4;
pub const X_RANDR_GET_SCREEN_SIZE_RANGE_MINOR_OPCODE: u8 = 6;
pub const X_RANDR_GET_SCREEN_RESOURCES_MINOR_OPCODE: u8 = 8;
pub const X_RANDR_GET_OUTPUT_INFO_MINOR_OPCODE: u8 = 9;
pub const X_RANDR_GET_OUTPUT_PROPERTY_MINOR_OPCODE: u8 = 15;
pub const X_RANDR_GET_CRTC_INFO_MINOR_OPCODE: u8 = 20;
pub const X_RANDR_GET_CRTC_GAMMA_SIZE_MINOR_OPCODE: u8 = 22;
pub const X_RANDR_GET_CRTC_GAMMA_MINOR_OPCODE: u8 = 23;
pub const X_RANDR_GET_SCREEN_RESOURCES_CURRENT_MINOR_OPCODE: u8 = 25;
pub const X_RANDR_GET_CRTC_TRANSFORM_MINOR_OPCODE: u8 = 27;
pub const X_RANDR_GET_PANNING_MINOR_OPCODE: u8 = 28;
pub const X_RANDR_GET_OUTPUT_PRIMARY_MINOR_OPCODE: u8 = 31;
pub const X_RANDR_GET_PROVIDERS_MINOR_OPCODE: u8 = 32;
pub const X_RANDR_GET_MONITORS_MINOR_OPCODE: u8 = 42;
pub const X_KEYBOARD_EXTENSION_NAME: &str = "XKEYBOARD";
pub const X_KEYBOARD_MAJOR_OPCODE: u8 = 133;
pub const X_KEYBOARD_FIRST_EVENT: u8 = 89;
pub const X_KEYBOARD_USE_EXTENSION_MINOR_OPCODE: u8 = 0;
pub const X_KEYBOARD_SELECT_EVENTS_MINOR_OPCODE: u8 = 1;
pub const X_KEYBOARD_GET_STATE_MINOR_OPCODE: u8 = 4;
pub const X_KEYBOARD_GET_CONTROLS_MINOR_OPCODE: u8 = 6;
pub const X_KEYBOARD_GET_MAP_MINOR_OPCODE: u8 = 8;
pub const X_KEYBOARD_GET_COMPAT_MAP_MINOR_OPCODE: u8 = 10;
pub const X_KEYBOARD_GET_INDICATOR_MAP_MINOR_OPCODE: u8 = 13;
pub const X_KEYBOARD_GET_NAMES_MINOR_OPCODE: u8 = 17;
pub const X_KEYBOARD_PER_CLIENT_FLAGS_MINOR_OPCODE: u8 = 21;
pub const X_KEYBOARD_GET_DEVICE_INFO_MINOR_OPCODE: u8 = 24;
pub const X_BIG_REQUESTS_EXTENSION_NAME: &str = "BIG-REQUESTS";
pub const X_BIG_REQUESTS_MAJOR_OPCODE: u8 = 134;
pub const X_BIG_REQUESTS_ENABLE_MINOR_OPCODE: u8 = 0;
pub const X_INPUT_EXTENSION_NAME: &str = "XInputExtension";
pub const X_INPUT_MAJOR_OPCODE: u8 = 135;
pub const X_INPUT_FIRST_EVENT: u8 = 90;
pub const X_INPUT_FIRST_ERROR: u8 = 160;
/// Fixed protocol devices; these never enumerate the seat's physical inventory.
pub const X_INPUT_MASTER_POINTER_ID: u16 = 2;
pub const X_INPUT_MASTER_KEYBOARD_ID: u16 = 3;
/// XI2 permits IDs above 127 to remain invisible to XI1 clients.
pub const X_INPUT_POINTER_SOURCE_ID: u16 = 128;
pub const X_INPUT_GET_EXTENSION_VERSION_MINOR_OPCODE: u8 = 1;
pub const X_INPUT_LIST_INPUT_DEVICES_MINOR_OPCODE: u8 = 2;
pub const X_INPUT_DEVICE_BELL_MINOR_OPCODE: u8 = 32;
/// XI1 `DeviceUse`: this device is the core pointer.
pub const X_INPUT_LEGACY_USE_POINTER: u8 = 0;
/// XI1 `DeviceUse`: this device is the core keyboard.
pub const X_INPUT_LEGACY_USE_KEYBOARD: u8 = 1;
/// XI1 `InputClass` discriminants. XI1 numbers its classes independently of XI2.
pub const X_INPUT_LEGACY_CLASS_KEY: u8 = 0;
pub const X_INPUT_LEGACY_CLASS_BUTTON: u8 = 1;
pub const X_INPUT_QUERY_POINTER_MINOR_OPCODE: u8 = 40;
pub const X_INPUT_CHANGE_CURSOR_MINOR_OPCODE: u8 = 42;
const X_INPUT_QUERY_POINTER_REQ_LEN: usize = 12;
const X_INPUT_CHANGE_CURSOR_REQ_LEN: usize = 16;
const X_INPUT_UNGRAB_DEVICE_REQ_LEN: usize = 12;
pub const X_INPUT_SELECT_EVENTS_MINOR_OPCODE: u8 = 46;
pub const X_INPUT_GET_CLIENT_POINTER_MINOR_OPCODE: u8 = 45;
pub const X_INPUT_QUERY_VERSION_MINOR_OPCODE: u8 = 47;
pub const X_INPUT_QUERY_DEVICE_MINOR_OPCODE: u8 = 48;
pub const X_INPUT_GET_FOCUS_MINOR_OPCODE: u8 = 50;
pub const X_INPUT_GRAB_DEVICE_MINOR_OPCODE: u8 = 51;
pub const X_INPUT_UNGRAB_DEVICE_MINOR_OPCODE: u8 = 52;
pub const X_INPUT_GET_PROPERTY_MINOR_OPCODE: u8 = 59;
pub const X_GENERIC_EVENT_EXTENSION_NAME: &str = "Generic Event Extension";
pub const X_GENERIC_EVENT_MAJOR_OPCODE: u8 = 136;
pub const X_GENERIC_EVENT_QUERY_VERSION_MINOR_OPCODE: u8 = 0;
pub const X_DRI3_EXTENSION_NAME: &str = "DRI3";
pub const X_DRI3_MAJOR_OPCODE: u8 = 137;
pub const X_DRI3_QUERY_VERSION_MINOR_OPCODE: u8 = 0;
pub const X_DRI3_OPEN_MINOR_OPCODE: u8 = 1;
pub const X_DRI3_PIXMAP_FROM_BUFFER_MINOR_OPCODE: u8 = 2;
pub const X_DRI3_BUFFER_FROM_PIXMAP_MINOR_OPCODE: u8 = 3;
pub const X_DRI3_FENCE_FROM_FD_MINOR_OPCODE: u8 = 4;
pub const X_DRI3_GET_SUPPORTED_MODIFIERS_MINOR_OPCODE: u8 = 6;
pub const X_DRI3_PIXMAP_FROM_BUFFERS_MINOR_OPCODE: u8 = 7;
pub const X_DRI3_BUFFERS_FROM_PIXMAP_MINOR_OPCODE: u8 = 8;
pub const X_DRI3_SET_DRM_DEVICE_IN_USE_MINOR_OPCODE: u8 = 9;
/// The highest DRI3 minor Sophia answers. Minor 5, `FDFromFence`, sits inside
/// this range without a constant above: it is defined by the protocol and not
/// implemented here, so it owes BadImplementation rather than BadRequest.
pub const X_DRI3_LAST_MINOR_OPCODE: u8 = X_DRI3_SET_DRM_DEVICE_IN_USE_MINOR_OPCODE;
pub const X_PRESENT_EXTENSION_NAME: &str = "Present";
pub const X_PRESENT_MAJOR_OPCODE: u8 = 138;
pub const X_PRESENT_FIRST_EVENT: u8 = 0;
pub const X_PRESENT_QUERY_VERSION_MINOR_OPCODE: u8 = 0;
pub const X_PRESENT_PIXMAP_MINOR_OPCODE: u8 = 1;
pub const X_PRESENT_NOTIFY_MSC_MINOR_OPCODE: u8 = 2;
pub const X_PRESENT_SELECT_INPUT_MINOR_OPCODE: u8 = 3;
pub const X_PRESENT_QUERY_CAPABILITIES_MINOR_OPCODE: u8 = 4;
/// The highest minor Present 1.2 defines, which is the version Sophia
/// advertises. Minor 5, `PresentPixmapSynced`, arrived in 1.4 and is not a
/// request at the advertised version.
pub const X_PRESENT_LAST_MINOR_OPCODE: u8 = X_PRESENT_QUERY_CAPABILITIES_MINOR_OPCODE;
pub const X_XFIXES_EXTENSION_NAME: &str = "XFIXES";
pub const X_XFIXES_MAJOR_OPCODE: u8 = 139;
pub const X_XFIXES_FIRST_EVENT: u8 = 66;
/// `XFixesSelectionNotify` is the extension's first event number; its three
/// causes are distinguished by the detail byte, not by separate event numbers.
pub const X_XFIXES_SELECTION_NOTIFY_SUBEVENT: u8 = 0;
/// Ownership was set, including being cleared to no owner.
pub const X_XFIXES_SET_SELECTION_OWNER_SUBTYPE: u8 = 0;
/// The owning window was destroyed.
pub const X_XFIXES_SELECTION_WINDOW_DESTROY_SUBTYPE: u8 = 1;
/// The owning client went away.
pub const X_XFIXES_SELECTION_CLIENT_CLOSE_SUBTYPE: u8 = 2;
pub const X_XFIXES_QUERY_VERSION_MINOR_OPCODE: u8 = 0;
pub const X_XFIXES_SELECT_SELECTION_INPUT_MINOR_OPCODE: u8 = 2;
pub const X_XFIXES_CREATE_REGION_MINOR_OPCODE: u8 = 5;
pub const X_XFIXES_DESTROY_REGION_MINOR_OPCODE: u8 = 10;
pub const X_XFIXES_SET_REGION_MINOR_OPCODE: u8 = 11;
// The region-algebra minors. These are what a client uses to build a shape
// out of pieces, and answering them is the difference between a region
// resource a client can create and one it can compute with.
/// The region constructors: a region built from something the server
/// already holds rather than from rectangles the client sends.
pub const X_XFIXES_CREATE_REGION_FROM_BITMAP_MINOR_OPCODE: u8 = 6;
pub const X_XFIXES_CREATE_REGION_FROM_WINDOW_MINOR_OPCODE: u8 = 7;
pub const X_XFIXES_CREATE_REGION_FROM_GC_MINOR_OPCODE: u8 = 8;
pub const X_XFIXES_CREATE_REGION_FROM_PICTURE_MINOR_OPCODE: u8 = 9;
pub const X_XFIXES_COPY_REGION_MINOR_OPCODE: u8 = 12;
pub const X_XFIXES_UNION_REGION_MINOR_OPCODE: u8 = 13;
pub const X_XFIXES_INTERSECT_REGION_MINOR_OPCODE: u8 = 14;
pub const X_XFIXES_SUBTRACT_REGION_MINOR_OPCODE: u8 = 15;
/// The region subtracted from a rectangle the client supplies, which is what
/// XFIXES means by inverting: a region has no complement without bounds.
pub const X_XFIXES_INVERT_REGION_MINOR_OPCODE: u8 = 16;
pub const X_XFIXES_TRANSLATE_REGION_MINOR_OPCODE: u8 = 17;
pub const X_XFIXES_REGION_EXTENTS_MINOR_OPCODE: u8 = 18;
pub const X_XFIXES_FETCH_REGION_MINOR_OPCODE: u8 = 19;
/// The region consumers: a region installed as some other object's clip or
/// shape. A region of zero clears it.
pub const X_XFIXES_SET_GC_CLIP_REGION_MINOR_OPCODE: u8 = 20;
pub const X_XFIXES_SET_WINDOW_SHAPE_REGION_MINOR_OPCODE: u8 = 21;
pub const X_XFIXES_SET_PICTURE_CLIP_REGION_MINOR_OPCODE: u8 = 22;
/// Grow a region outward on each side. Version 3.
pub const X_XFIXES_EXPAND_REGION_MINOR_OPCODE: u8 = 28;
const X_XFIXES_CREATE_REGION_FROM_DRAWABLE_REQ_LEN: usize = 12;
const X_XFIXES_CREATE_REGION_FROM_WINDOW_REQ_LEN: usize = 16;
const X_XFIXES_EXPAND_REGION_REQ_LEN: usize = 20;
/// The window regions XFIXES will build a region from. SHAPE's Input kind is
/// deliberately not one of them, which is what Xorg does.
pub const X_XFIXES_WINDOW_REGION_BOUNDING: u8 = 0;
pub const X_XFIXES_WINDOW_REGION_CLIP: u8 = 1;
/// The highest minor XFIXES 6.0 defines: `GetClientDisconnectMode`. A minor at or below
/// this that is not implemented is declined; one above it is not a request
/// this version has.
pub const X_XFIXES_LAST_MINOR_OPCODE: u8 = 34;
/// The version this server answers. Note that the minors behind it are not
/// all implemented: the ones that are not refuse by name rather than failing
/// to parse, and the residue is recorded in todo.md as debt to settle against
/// real client logs.
pub const X_XFIXES_MAJOR_VERSION: u32 = 6;
pub const X_XFIXES_MINOR_VERSION: u32 = 0;
const X_XFIXES_COMBINE_REGION_REQ_LEN: usize = 16;
const X_XFIXES_COPY_REGION_REQ_LEN: usize = 12;
const X_XFIXES_INVERT_REGION_REQ_LEN: usize = 20;
const X_XFIXES_TRANSLATE_REGION_REQ_LEN: usize = 12;
const X_XFIXES_REGION_QUERY_REQ_LEN: usize = 8;
pub const X_GLX_EXTENSION_NAME: &str = "GLX";
pub const X_GLX_MAJOR_OPCODE: u8 = 140;
pub const X_GLX_FIRST_EVENT: u8 = 72;
/// `GLX_EXT_texture_from_pixmap` configuration attributes.
pub const X_GLX_BIND_TO_TEXTURE_RGB_ATTRIBUTE: u32 = 0x20D0;
pub const X_GLX_BIND_TO_TEXTURE_RGBA_ATTRIBUTE: u32 = 0x20D1;
pub const X_GLX_BIND_TO_TEXTURE_TARGETS_ATTRIBUTE: u32 = 0x20D3;
pub const X_GLX_Y_INVERTED_ATTRIBUTE: u32 = 0x20D4;
/// `GLX_EXT_texture_from_pixmap` target bits and names.
pub const X_GLX_TEXTURE_1D_BIT_VALUE: u32 = 0x1;
pub const X_GLX_TEXTURE_2D_BIT_VALUE: u32 = 0x2;
pub const X_GLX_TEXTURE_RECTANGLE_BIT_VALUE: u32 = 0x4;
/// Every direct target the extension defines.
pub const X_GLX_TEXTURE_TARGETS_ALL: u32 =
    X_GLX_TEXTURE_1D_BIT_VALUE | X_GLX_TEXTURE_2D_BIT_VALUE | X_GLX_TEXTURE_RECTANGLE_BIT_VALUE;
pub const X_GLX_TEXTURE_TARGET_ATTRIBUTE: u32 = 0x20D6;
pub const X_GLX_TEXTURE_1D_VALUE: u32 = 0x20DB;
pub const X_GLX_TEXTURE_2D_VALUE: u32 = 0x20DC;
pub const X_GLX_TEXTURE_RECTANGLE_VALUE: u32 = 0x20DD;

/// The target bit a named target selects, if it names one at all.
pub const fn x_glx_texture_target_bit(target: u32) -> Option<u32> {
    match target {
        X_GLX_TEXTURE_1D_VALUE => Some(X_GLX_TEXTURE_1D_BIT_VALUE),
        X_GLX_TEXTURE_2D_VALUE => Some(X_GLX_TEXTURE_2D_BIT_VALUE),
        X_GLX_TEXTURE_RECTANGLE_VALUE => Some(X_GLX_TEXTURE_RECTANGLE_BIT_VALUE),
        _ => None,
    }
}

/// The target name a bit answers to, so a query returns what was asked for.
pub const fn x_glx_texture_target_name(target_bit: u32) -> u32 {
    match target_bit {
        X_GLX_TEXTURE_1D_BIT_VALUE => X_GLX_TEXTURE_1D_VALUE,
        X_GLX_TEXTURE_RECTANGLE_BIT_VALUE => X_GLX_TEXTURE_RECTANGLE_VALUE,
        _ => X_GLX_TEXTURE_2D_VALUE,
    }
}

/// Whether an extent can be bound to a target.
///
/// A one-dimensional texture has one row. Modern direct GL supports
/// non-power-of-two extents for both two-dimensional and rectangle textures.
pub const fn x_glx_texture_target_admits(target_bit: u32, width: i32, height: i32) -> bool {
    if width <= 0 || height <= 0 {
        return false;
    }
    match target_bit {
        X_GLX_TEXTURE_1D_BIT_VALUE => height == 1,
        X_GLX_TEXTURE_2D_BIT_VALUE | X_GLX_TEXTURE_RECTANGLE_BIT_VALUE => true,
        _ => false,
    }
}

/// `GLX_TEXTURE_FORMAT_EXT` and its values.
pub const X_GLX_TEXTURE_FORMAT_ATTRIBUTE: u32 = 0x20D5;
pub const X_GLX_TEXTURE_FORMAT_NONE_VALUE: u32 = 0x20D8;
pub const X_GLX_TEXTURE_FORMAT_RGB_VALUE: u32 = 0x20D9;
pub const X_GLX_TEXTURE_FORMAT_RGBA_VALUE: u32 = 0x20DA;
/// `GLX_MIPMAP_TEXTURE_EXT`, and the configuration capability it requires.
///
/// A mipmapped binding needs `GLX_BIND_TO_MIPMAP_TEXTURE_EXT` on the
/// configuration, which the row itself answers.
pub const X_GLX_MIPMAP_TEXTURE_ATTRIBUTE: u32 = 0x20D7;
pub const X_GLX_BIND_TO_MIPMAP_TEXTURE_ATTRIBUTE: u32 = 0x20D2;

/// Selects the first supported target in the extension's default preference order.
pub const fn x_glx_default_texture_target(supported: u32, width: i32, height: i32) -> Option<u32> {
    let candidates = [
        X_GLX_TEXTURE_2D_BIT_VALUE,
        X_GLX_TEXTURE_RECTANGLE_BIT_VALUE,
        X_GLX_TEXTURE_1D_BIT_VALUE,
    ];
    let mut index = 0;
    while index < candidates.len() {
        let bit = candidates[index];
        if supported & bit != 0 && x_glx_texture_target_admits(bit, width, height) {
            return Some(bit);
        }
        index += 1;
    }
    None
}
/// GLX's error range follows the disjoint XInput and RENDER ranges.
pub const X_GLX_FIRST_ERROR: u8 = 170;
/// How many codes the GLX protocol defines, `GLXBadContext` through
/// `GLXBadProfileARB`. The whole span is reserved even though Sophia emits a
/// few of them, because a client computes every offset from the same base.
pub const X_GLX_ERROR_COUNT: u8 = 14;
/// `GLXBadDrawable`: the drawable is not one this configuration can render to.
pub const X_GLX_BAD_DRAWABLE_ERROR_OFFSET: u8 = 2;
/// `GLXBadPixmap`: the pixmap is not a GLX drawable, or cannot become one.
pub const X_GLX_BAD_PIXMAP_ERROR_OFFSET: u8 = 3;
/// `GLXBadFBConfig`: no configuration answers to that identifier.
pub const X_GLX_BAD_FB_CONFIG_ERROR_OFFSET: u8 = 9;

// The GLX request minors, all of them, in protocol order.
//
// The ones Sophia does not implement are named too. A table that lists only what
// is implemented cannot say what was refused, and it reads as though the rest of
// the protocol does not exist -- which for an extension advertising GLX 1.4 is a
// claim about the wrong thing. Each unimplemented minor carries why, so the
// distinction between "not offered" and "not yet written" survives in the table
// rather than in someone's memory.

/// Indirect GL rendering: the client streams GL commands to the server for
/// execution. Sophia runs no GL of its own -- every context here belongs to the
/// client's own driver -- so this is not a gap to be filled but a mode Sophia
/// does not offer.
pub const X_GLX_RENDER_MINOR_OPCODE: u8 = 1;
/// Indirect GL rendering for command buffers over the single-request limit. Not
/// offered, for the same reason as `Render`.
pub const X_GLX_RENDER_LARGE_MINOR_OPCODE: u8 = 2;
pub const X_GLX_CREATE_CONTEXT_MINOR_OPCODE: u8 = 3;
pub const X_GLX_DESTROY_CONTEXT_MINOR_OPCODE: u8 = 4;
pub const X_GLX_MAKE_CURRENT_MINOR_OPCODE: u8 = 5;
pub const X_GLX_IS_DIRECT_MINOR_OPCODE: u8 = 6;
pub const X_GLX_QUERY_VERSION_MINOR_OPCODE: u8 = 7;
/// Blocks until the indirect GL stream drains. Meaningless without a server-side
/// GL stream to drain.
pub const X_GLX_WAIT_GL_MINOR_OPCODE: u8 = 8;
/// Blocks until prior X requests complete, for ordering against indirect GL.
/// Same reason.
pub const X_GLX_WAIT_X_MINOR_OPCODE: u8 = 9;
/// Copies GL state between two server-side contexts. Sophia holds no GL state to
/// copy.
pub const X_GLX_COPY_CONTEXT_MINOR_OPCODE: u8 = 10;
/// The indirect present path. Direct clients reach the screen through DRI3 and
/// Present instead, which is the path Sophia implements.
pub const X_GLX_SWAP_BUFFERS_MINOR_OPCODE: u8 = 11;
/// Builds GL display lists from an X font. Indirect, and deprecated besides.
pub const X_GLX_USE_X_FONT_MINOR_OPCODE: u8 = 12;
/// GLX 1.2 GLX pixmaps. `GLX_DRAWABLE_TYPE` deliberately excludes
/// `GLX_PIXMAP_BIT`, so Sophia does not claim these drawables.
pub const X_GLX_CREATE_GLX_PIXMAP_MINOR_OPCODE: u8 = 13;
pub const X_GLX_GET_VISUAL_CONFIGS_MINOR_OPCODE: u8 = 14;
/// The destructor for GLX 1.2 GLX pixmaps, which are not offered.
pub const X_GLX_DESTROY_GLX_PIXMAP_MINOR_OPCODE: u8 = 15;
/// The vendor escape hatch, and the transport for extensions Sophia does not
/// advertise. Refusing it is what the advertisement already implies.
pub const X_GLX_VENDOR_PRIVATE_MINOR_OPCODE: u8 = 16;
/// `VendorPrivate` for escapes that answer. Not advertised, so not offered.
pub const X_GLX_VENDOR_PRIVATE_WITH_REPLY_MINOR_OPCODE: u8 = 17;
pub const X_GLX_QUERY_EXTENSIONS_STRING_MINOR_OPCODE: u8 = 18;
pub const X_GLX_QUERY_SERVER_STRING_MINOR_OPCODE: u8 = 19;
pub const X_GLX_CLIENT_INFO_MINOR_OPCODE: u8 = 20;
pub const X_GLX_GET_FB_CONFIGS_MINOR_OPCODE: u8 = 21;
/// GLX 1.3 GLX pixmaps, the FBConfig-based successor to minor 13. Excluded from
/// `GLX_DRAWABLE_TYPE` for the same reason.
pub const X_GLX_CREATE_PIXMAP_MINOR_OPCODE: u8 = 22;
/// The destructor for GLX 1.3 GLX pixmaps, which are not offered.
pub const X_GLX_DESTROY_PIXMAP_MINOR_OPCODE: u8 = 23;
pub const X_GLX_CREATE_NEW_CONTEXT_MINOR_OPCODE: u8 = 24;
pub const X_GLX_QUERY_CONTEXT_MINOR_OPCODE: u8 = 25;
pub const X_GLX_MAKE_CONTEXT_CURRENT_MINOR_OPCODE: u8 = 26;
pub const X_GLX_CREATE_PBUFFER_MINOR_OPCODE: u8 = 27;
pub const X_GLX_DESTROY_PBUFFER_MINOR_OPCODE: u8 = 28;
pub const X_GLX_GET_DRAWABLE_ATTRIBUTES_MINOR_OPCODE: u8 = 29;
pub const X_GLX_CHANGE_DRAWABLE_ATTRIBUTES_MINOR_OPCODE: u8 = 30;
pub const X_GLX_CREATE_WINDOW_MINOR_OPCODE: u8 = 31;
pub const X_GLX_DELETE_WINDOW_MINOR_OPCODE: u8 = 32;
pub const X_GLX_SET_CLIENT_INFO_ARB_MINOR_OPCODE: u8 = 33;
pub const X_GLX_CREATE_CONTEXT_ATTRIBS_ARB_MINOR_OPCODE: u8 = 34;
pub const X_GLX_SET_CLIENT_INFO_2_ARB_MINOR_OPCODE: u8 = 35;
/// The highest GLX minor the protocol defines. A minor above this is not a
/// request Sophia declined to implement; it is not a GLX request at all.
pub const X_GLX_LAST_MINOR_OPCODE: u8 = X_GLX_SET_CLIENT_INFO_2_ARB_MINOR_OPCODE;

// The GLX attribute tokens Sophia names, by the value that travels on the wire.
//
// These were thirty bare hex literals in the `GetFBConfigs` reply builder, where
// the only way to know that `0x186a1` was `GLX_SAMPLES` was to look it up. A
// reply built from unnamed numbers cannot be reviewed against the specification
// it claims to satisfy.

/// `GLX_BUFFER_SIZE`: total colour bits per pixel.
pub const X_GLX_BUFFER_SIZE_ATTRIBUTE: u32 = 0x2;
/// `GLX_LEVEL`: overlay/underlay plane, zero for the main plane.
pub const X_GLX_LEVEL_ATTRIBUTE: u32 = 0x3;
pub const X_GLX_DOUBLEBUFFER_ATTRIBUTE: u32 = 0x5;
pub const X_GLX_STEREO_ATTRIBUTE: u32 = 0x6;
pub const X_GLX_AUX_BUFFERS_ATTRIBUTE: u32 = 0x7;
pub const X_GLX_RED_SIZE_ATTRIBUTE: u32 = 0x8;
pub const X_GLX_GREEN_SIZE_ATTRIBUTE: u32 = 0x9;
pub const X_GLX_BLUE_SIZE_ATTRIBUTE: u32 = 0xA;
pub const X_GLX_ALPHA_SIZE_ATTRIBUTE: u32 = 0xB;
pub const X_GLX_DEPTH_SIZE_ATTRIBUTE: u32 = 0xC;
pub const X_GLX_STENCIL_SIZE_ATTRIBUTE: u32 = 0xD;
pub const X_GLX_ACCUM_RED_SIZE_ATTRIBUTE: u32 = 0xE;
pub const X_GLX_ACCUM_GREEN_SIZE_ATTRIBUTE: u32 = 0xF;
pub const X_GLX_ACCUM_BLUE_SIZE_ATTRIBUTE: u32 = 0x10;
pub const X_GLX_ACCUM_ALPHA_SIZE_ATTRIBUTE: u32 = 0x11;
pub const X_GLX_TRANSPARENT_TYPE_ATTRIBUTE: u32 = 0x20;
pub const X_GLX_X_VISUAL_TYPE_ATTRIBUTE: u32 = 0x22;
/// `GLX_CONFIG_CAVEAT`: whether choosing this configuration costs something.
pub const X_GLX_CONFIG_CAVEAT_ATTRIBUTE: u32 = 0x23;
pub const X_GLX_VISUAL_ID_ATTRIBUTE: u32 = 0x800B;
/// `GLX_DRAWABLE_TYPE`: which drawable kinds the configuration supports.
pub const X_GLX_DRAWABLE_TYPE_ATTRIBUTE: u32 = 0x8010;
/// `GLX_RENDER_TYPE`: which colour models the configuration renders.
pub const X_GLX_RENDER_TYPE_ATTRIBUTE: u32 = 0x8011;
/// `GLX_X_RENDERABLE`: whether X can draw to drawables of this configuration.
pub const X_GLX_X_RENDERABLE_ATTRIBUTE: u32 = 0x8012;
pub const X_GLX_FBCONFIG_ID_ATTRIBUTE: u32 = 0x8013;
pub const X_GLX_MAX_PBUFFER_WIDTH_ATTRIBUTE: u32 = 0x8016;
pub const X_GLX_MAX_PBUFFER_HEIGHT_ATTRIBUTE: u32 = 0x8017;
pub const X_GLX_MAX_PBUFFER_PIXELS_ATTRIBUTE: u32 = 0x8018;
/// `GLX_LARGEST_PBUFFER`: clamp to the maximum rather than refusing.
pub const X_GLX_LARGEST_PBUFFER_ATTRIBUTE: u32 = 0x801C;
/// `GLX_PBUFFER_WIDTH` and `GLX_PBUFFER_HEIGHT`. Height is the lower number.
pub const X_GLX_PBUFFER_HEIGHT_ATTRIBUTE: u32 = 0x8040;
pub const X_GLX_PBUFFER_WIDTH_ATTRIBUTE: u32 = 0x8041;
/// `GLX_FRAMEBUFFER_SRGB_CAPABLE_EXT`, from `GLX_EXT_framebuffer_sRGB`.
pub const X_GLX_FRAMEBUFFER_SRGB_CAPABLE_ATTRIBUTE: u32 = 0x20B2;
/// `GLX_SAMPLE_BUFFERS`, added by GLX 1.4. Answered as zero: Sophia offers no
/// multisample configuration, and says so rather than omitting the attribute.
pub const X_GLX_SAMPLE_BUFFERS_ATTRIBUTE: u32 = 0x186A0;
/// `GLX_SAMPLES`, added by GLX 1.4. Zero, for the same reason.
pub const X_GLX_SAMPLES_ATTRIBUTE: u32 = 0x186A1;

// The GLX attribute values Sophia answers with.

/// `GLX_NONE`: the answer for both `GLX_TRANSPARENT_TYPE` and
/// `GLX_CONFIG_CAVEAT`.
pub const X_GLX_NONE_VALUE: u32 = 0x8000;
/// `GLX_TRUE_COLOR`, the only X visual type Sophia offers.
pub const X_GLX_TRUE_COLOR_VALUE: u32 = 0x8002;
/// `GLX_RGBA_BIT`: the only render type Sophia offers. Colour-index rendering
/// went with indirect GL.
pub const X_GLX_RGBA_BIT_VALUE: u32 = 0x1;
/// Extra resource identifiers, for a client that has used up the range it was
/// given at connection setup.
///
/// A long-lived client that creates and destroys many resources -- a browser
/// left open for days -- eventually exhausts its range, and without this it
/// has no way to ask for more. The failure mode is the client dying rather
/// than degrading, which is why this is worth having before anything needs it.
pub const X_XC_MISC_EXTENSION_NAME: &str = "XC-MISC";
pub const X_XC_MISC_MAJOR_OPCODE: u8 = 143;
pub const X_XC_MISC_GET_VERSION_MINOR_OPCODE: u8 = 0;
pub const X_XC_MISC_GET_XID_RANGE_MINOR_OPCODE: u8 = 1;
pub const X_XC_MISC_GET_XID_LIST_MINOR_OPCODE: u8 = 2;
pub const X_XC_MISC_MAJOR_VERSION: u16 = 1;
pub const X_XC_MISC_MINOR_VERSION: u16 = 1;
const X_XC_MISC_GET_VERSION_REQ_LEN: usize = 8;
const X_XC_MISC_GET_XID_RANGE_REQ_LEN: usize = 4;
const X_XC_MISC_GET_XID_LIST_REQ_LEN: usize = 8;
/// The most identifiers one `GetXIDList` will return.
///
/// The request carries a `CARD32`, so a client can ask for four billion. The
/// reply is a list in memory, so the ask is bounded before it is honoured.
pub const X_XC_MISC_MAX_XID_LIST: u32 = 4096;

/// The legacy mode-line extension, which Mesa still uses for one thing.
///
/// `glXGetMscRateOML` is implemented in Mesa by asking this extension for the
/// modeline and dividing the clock by the total pixels in a frame. RandR
/// superseded the extension for everything else two decades ago, so only the
/// two requests that answer take part here.
pub const X_XF86_VIDMODE_EXTENSION_NAME: &str = "XFree86-VidModeExtension";
pub const X_XF86_VIDMODE_MAJOR_OPCODE: u8 = 142;
pub const X_XF86_VIDMODE_QUERY_VERSION_MINOR_OPCODE: u8 = 0;
pub const X_XF86_VIDMODE_GET_MODE_LINE_MINOR_OPCODE: u8 = 1;
/// Sent by `libXxf86vm` once it has seen a major version of 2 or more, so
/// refusing it would break the exchange immediately after `QueryVersion`
/// succeeded.
pub const X_XF86_VIDMODE_SET_CLIENT_VERSION_MINOR_OPCODE: u8 = 14;
/// Answering 2 is what selects the modern reply shape in `libXxf86vm`;
/// answering 0 or 1 selects a shorter, differently laid out one.
/// `GetPermissions`, the highest minor version 2.2 defines. Sophia implements
/// only `QueryVersion`, `GetModeLine` and `SetClientVersion`; the rest of this
/// range is defined-but-unimplemented and owes BadImplementation.
pub const X_XF86_VIDMODE_LAST_MINOR_OPCODE: u8 = 20;
pub const X_XF86_VIDMODE_MAJOR_VERSION: u16 = 2;
pub const X_XF86_VIDMODE_MINOR_VERSION: u16 = 2;
const X_XF86_VIDMODE_QUERY_VERSION_REQ_LEN: usize = 4;
const X_XF86_VIDMODE_GET_MODE_LINE_REQ_LEN: usize = 8;
const X_XF86_VIDMODE_SET_CLIENT_VERSION_REQ_LEN: usize = 8;

pub const X_SYNC_EXTENSION_NAME: &str = "SYNC";
pub const X_SYNC_MAJOR_OPCODE: u8 = 141;
pub const X_SYNC_FIRST_EVENT: u8 = 68;
pub const X_SYNC_INITIALIZE_MINOR_OPCODE: u8 = 0;
pub const X_SYNC_LIST_SYSTEM_COUNTERS_MINOR_OPCODE: u8 = 1;
pub const X_SYNC_CREATE_COUNTER_MINOR_OPCODE: u8 = 2;
pub const X_SYNC_SET_COUNTER_MINOR_OPCODE: u8 = 3;
pub const X_SYNC_CHANGE_COUNTER_MINOR_OPCODE: u8 = 4;
pub const X_SYNC_QUERY_COUNTER_MINOR_OPCODE: u8 = 5;
pub const X_SYNC_DESTROY_COUNTER_MINOR_OPCODE: u8 = 6;
pub const X_SYNC_DESTROY_FENCE_MINOR_OPCODE: u8 = 17;

const X_CREATE_WINDOW_REQ_LEN: usize = 32;
const X_CHANGE_WINDOW_ATTRIBUTES_REQ_LEN: usize = 12;
const X_GET_WINDOW_ATTRIBUTES_REQ_LEN: usize = 8;
const X_DESTROY_WINDOW_REQ_LEN: usize = 8;
const X_DESTROY_SUBWINDOWS_REQ_LEN: usize = 8;
const X_REPARENT_WINDOW_REQ_LEN: usize = 16;
const X_MAP_WINDOW_REQ_LEN: usize = 8;
const X_MAP_SUBWINDOWS_REQ_LEN: usize = 8;
const X_UNMAP_WINDOW_REQ_LEN: usize = 8;
const X_CONFIGURE_WINDOW_REQ_LEN: usize = 12;
const X_GET_GEOMETRY_REQ_LEN: usize = 8;
const X_QUERY_TREE_REQ_LEN: usize = 8;
const X_INTERN_ATOM_REQ_LEN: usize = 8;
const X_GET_ATOM_NAME_REQ_LEN: usize = 8;
const X_CHANGE_PROPERTY_REQ_LEN: usize = 24;
const X_DELETE_PROPERTY_REQ_LEN: usize = 12;
const X_GET_PROPERTY_REQ_LEN: usize = 24;
const X_QUERY_POINTER_REQ_LEN: usize = 8;
const X_LIST_PROPERTIES_REQ_LEN: usize = 8;
const X_SET_SELECTION_OWNER_REQ_LEN: usize = 16;
const X_GET_SELECTION_OWNER_REQ_LEN: usize = 8;
const X_CONVERT_SELECTION_REQ_LEN: usize = 24;
const X_SEND_EVENT_REQ_LEN: usize = 44;
const X_GRAB_BUTTON_REQ_LEN: usize = 24;
const X_UNGRAB_BUTTON_REQ_LEN: usize = 12;
const X_GRAB_POINTER_REQ_LEN: usize = 24;
const X_UNGRAB_POINTER_REQ_LEN: usize = 8;
const X_GRAB_KEYBOARD_REQ_LEN: usize = 16;
const X_UNGRAB_KEYBOARD_REQ_LEN: usize = 8;
const X_GRAB_KEY_REQ_LEN: usize = 16;
const X_UNGRAB_KEY_REQ_LEN: usize = 12;
const X_ALLOW_EVENTS_REQ_LEN: usize = 8;
const X_GRAB_SERVER_REQ_LEN: usize = 4;
const X_UNGRAB_SERVER_REQ_LEN: usize = 4;
const X_TRANSLATE_COORDINATES_REQ_LEN: usize = 16;
const X_SET_INPUT_FOCUS_REQ_LEN: usize = 12;
const X_GET_INPUT_FOCUS_REQ_LEN: usize = 4;
const X_GET_IMAGE_REQ_LEN: usize = 20;
const X_OPEN_FONT_REQ_LEN: usize = 12;
const X_CLOSE_FONT_REQ_LEN: usize = 8;
const X_QUERY_FONT_REQ_LEN: usize = 8;
const X_QUERY_TEXT_EXTENTS_REQ_LEN: usize = 8;
const X_COPY_GC_REQ_LEN: usize = 16;
const X_SET_DASHES_REQ_LEN: usize = 12;
const X_SET_FONT_PATH_REQ_LEN: usize = 8;
const X_GET_FONT_PATH_REQ_LEN: usize = 4;
const X_LIST_FONTS_REQ_LEN: usize = 8;
const X_LIST_FONTS_WITH_INFO_REQ_LEN: usize = 8;
const X_CREATE_PIXMAP_REQ_LEN: usize = 16;
const X_FREE_PIXMAP_REQ_LEN: usize = 8;
const X_CREATE_GC_REQ_LEN: usize = 16;
const X_CHANGE_GC_REQ_LEN: usize = 12;
const X_SET_CLIP_RECTANGLES_REQ_LEN: usize = 12;
const X_FREE_GC_REQ_LEN: usize = 8;
const X_CLEAR_AREA_REQ_LEN: usize = 16;
const X_COPY_AREA_REQ_LEN: usize = 28;
const X_COPY_PLANE_REQ_LEN: usize = 32;
const X_POLY_LINE_REQ_LEN: usize = 12;
const X_POLY_SEGMENT_REQ_LEN: usize = 12;
const X_POLY_RECTANGLE_REQ_LEN: usize = 12;
const X_FILL_POLY_REQ_LEN: usize = 16;
const X_POLY_FILL_RECTANGLE_REQ_LEN: usize = 12;
const X_POLY_FILL_ARC_REQ_LEN: usize = 12;
const X_POLY_POINT_REQ_LEN: usize = 12;
const X_POLY_ARC_REQ_LEN: usize = 12;
const X_PUT_IMAGE_REQ_LEN: usize = 24;
const X_POLY_TEXT8_REQ_LEN: usize = 16;
const X_POLY_TEXT16_REQ_LEN: usize = 16;
const X_IMAGE_TEXT8_REQ_LEN: usize = 16;
const X_IMAGE_TEXT16_REQ_LEN: usize = 16;
const X_CREATE_COLORMAP_REQ_LEN: usize = 16;
const X_FREE_COLORMAP_REQ_LEN: usize = 8;
const X_ALLOC_COLOR_REQ_LEN: usize = 16;
const X_ALLOC_NAMED_COLOR_REQ_LEN: usize = 12;
const X_QUERY_COLORS_REQ_LEN: usize = 8;
const X_CREATE_CURSOR_REQ_LEN: usize = 32;
const X_CREATE_GLYPH_CURSOR_REQ_LEN: usize = 32;
const X_FREE_CURSOR_REQ_LEN: usize = 8;
const X_RECOLOR_CURSOR_REQ_LEN: usize = 20;
const X_QUERY_EXTENSION_REQ_LEN: usize = 8;
const X_LIST_EXTENSIONS_REQ_LEN: usize = 4;
const X_GET_KEYBOARD_MAPPING_REQ_LEN: usize = 8;
const X_GET_POINTER_MAPPING_REQ_LEN: usize = 4;
const X_QUERY_BEST_SIZE_REQ_LEN: usize = 12;
const X_GET_MODIFIER_MAPPING_REQ_LEN: usize = 4;
const X_SOPHIA_PRESENT_PIXMAP_REQ_LEN: usize = 32;
const X_MIT_SHM_QUERY_VERSION_REQ_LEN: usize = 4;
const X_MIT_SHM_ATTACH_REQ_LEN: usize = 16;
const X_MIT_SHM_DETACH_REQ_LEN: usize = 8;
const X_MIT_SHM_PUT_IMAGE_REQ_LEN: usize = 40;
const X_MIT_SHM_GET_IMAGE_REQ_LEN: usize = 32;
const X_MIT_SHM_CREATE_PIXMAP_REQ_LEN: usize = 28;
/// The descriptor arrives out of band, so it occupies no request bytes.
const X_MIT_SHM_ATTACH_FD_REQ_LEN: usize = 12;
const X_MIT_SHM_CREATE_SEGMENT_REQ_LEN: usize = 16;
const X_RANDR_QUERY_VERSION_REQ_LEN: usize = 12;
const X_RANDR_SELECT_INPUT_REQ_LEN: usize = 12;
const X_RANDR_GET_SCREEN_SIZE_RANGE_REQ_LEN: usize = 8;
const X_RANDR_GET_SCREEN_RESOURCES_REQ_LEN: usize = 8;
const X_RANDR_GET_OUTPUT_INFO_REQ_LEN: usize = 12;
const X_RANDR_GET_OUTPUT_PROPERTY_REQ_LEN: usize = 28;
const X_RANDR_GET_CRTC_INFO_REQ_LEN: usize = 12;
const X_RANDR_GET_CRTC_GAMMA_SIZE_REQ_LEN: usize = 8;
const X_RANDR_GET_CRTC_GAMMA_REQ_LEN: usize = 8;
const X_RANDR_GET_CRTC_TRANSFORM_REQ_LEN: usize = 8;
const X_RANDR_GET_PANNING_REQ_LEN: usize = 8;
const X_RANDR_GET_OUTPUT_PRIMARY_REQ_LEN: usize = 8;
const X_RANDR_GET_MONITORS_REQ_LEN: usize = 12;
const X_KEYBOARD_USE_EXTENSION_REQ_LEN: usize = 8;
const X_KEYBOARD_SELECT_EVENTS_REQ_LEN: usize = 16;
const X_KEYBOARD_GET_MAP_REQ_LEN: usize = 28;
const X_KEYBOARD_GET_CONTROLS_REQ_LEN: usize = 8;
const X_KEYBOARD_PER_CLIENT_FLAGS_REQ_LEN: usize = 28;
const X_BIG_REQUESTS_ENABLE_REQ_LEN: usize = 4;
const X_INPUT_LIST_INPUT_DEVICES_REQ_LEN: usize = 4;
const X_INPUT_QUERY_VERSION_REQ_LEN: usize = 8;
const X_INPUT_GET_CLIENT_POINTER_REQ_LEN: usize = 8;
const X_INPUT_QUERY_DEVICE_REQ_LEN: usize = 8;
const X_INPUT_SELECT_EVENTS_REQ_LEN: usize = 12;
const X_INPUT_GET_FOCUS_REQ_LEN: usize = 8;
const X_INPUT_GRAB_DEVICE_REQ_LEN: usize = 24;
const X_INPUT_GET_PROPERTY_REQ_LEN: usize = 24;
const X_GENERIC_EVENT_QUERY_VERSION_REQ_LEN: usize = 8;

pub const X_PUT_IMAGE_MAX_DATA_BYTES: usize = 256 * 1024;
pub const X_QUERY_COLORS_MAX_PIXELS: usize = 256;
pub const X_POLY_TEXT8_MAX_BYTES: usize = 64 * 1024;
pub const X_POLY_TEXT16_MAX_BYTES: usize = 64 * 1024;
pub const X_IMAGE_TEXT8_MAX_BYTES: usize = 64 * 1024;
pub const X_IMAGE_TEXT16_MAX_BYTES: usize = 64 * 1024;
/// A `QueryTextExtents` string, in characters.
///
/// The request carries no count; its length is recovered from the request
/// length, so this is the bound on what that arithmetic may produce.
pub const X_QUERY_TEXT_EXTENTS_MAX_CHARS: usize = 32 * 1024;

/// The longest dash pattern a client may set.
///
/// A pattern is walked once per segment, so its length is work per line. Real
/// patterns are two or four entries; this bounds a pathological one.
pub const X_SET_DASHES_MAX_LEN: usize = 255;
pub const X_ALLOC_NAMED_COLOR_MAX_NAME_BYTES: usize = 256;

/// The compositing and antialiased-text extension every modern toolkit asks
/// for first.
///
/// Both Quickshell and Brave asked for this by name and were refused; the
/// absent-extension log is what put it here. A client refused RENDER does not
/// fail -- it falls back to core drawing and looks wrong or feels slow, which
/// is the kind of degradation that never shows up in an error trace.
pub const X_RENDER_EXTENSION_NAME: &str = "RENDER";
pub const X_RENDER_MAJOR_OPCODE: u8 = 144;
/// The base for RENDER's five errors: PictFormat, Picture, PictOp, GlyphSet
/// and Glyph, in that order. XInput holds 160 and its protocol defines five
/// errors of its own, so 160..=164 stay reserved for it.
pub const X_RENDER_FIRST_ERROR: u8 = 165;
pub const X_RENDER_PICT_FORMAT_ERROR_OFFSET: u8 = 0;
pub const X_RENDER_PICTURE_ERROR_OFFSET: u8 = 1;
pub const X_RENDER_PICT_OP_ERROR_OFFSET: u8 = 2;
pub const X_RENDER_GLYPH_SET_ERROR_OFFSET: u8 = 3;
pub const X_RENDER_GLYPH_ERROR_OFFSET: u8 = 4;
/// The advertised version is the promise, and it tracks what is implemented.
///
/// 0.4 is the whole base protocol; 0.5 adds ARGB cursors; 0.6 transforms and
/// filters; 0.10 solid fills and gradients; 0.11 the PDF blend operators.
/// MIT-SHM taught what advertising past the implementation costs: a version
/// reply that over-promises sends a client down a path that ends in the
/// client's error handler rather than in its fallback. This constant moves
/// only when the requests behind the next version answer.
pub const X_RENDER_MAJOR_VERSION: u32 = 0;
pub const X_RENDER_MINOR_VERSION: u32 = 10;

// The RENDER request minors, all of them, in protocol order, the GLX way:
// the ones Sophia does not implement are named too, each with why, so the
// difference between "declined" and "not yet written" survives here rather
// than in someone's memory. Refusals are two-tier. A minor defined within the
// advertised version but not implemented answers BadImplementation, which is
// also what Xorg answers for the five it never wrote. A minor beyond the
// advertised version answers BadRequest, because a genuine server of that
// version had no dispatch entry for it at all.
pub const X_RENDER_QUERY_VERSION_MINOR_OPCODE: u8 = 0;
pub const X_RENDER_QUERY_PICT_FORMATS_MINOR_OPCODE: u8 = 1;
/// Indexed-visual palettes for PictTypeIndexed formats. Sophia's visuals are
/// fixed TrueColor, so no indexed format exists to query. Version 0.7.
pub const X_RENDER_QUERY_PICT_INDEX_VALUES_MINOR_OPCODE: u8 = 2;
/// Never implemented by any server, Xorg included; the protocol reserved the
/// name and nothing was ever behind it.
pub const X_RENDER_QUERY_DITHERS_MINOR_OPCODE: u8 = 3;
pub const X_RENDER_CREATE_PICTURE_MINOR_OPCODE: u8 = 4;
pub const X_RENDER_CHANGE_PICTURE_MINOR_OPCODE: u8 = 5;
pub const X_RENDER_SET_PICTURE_CLIP_RECTANGLES_MINOR_OPCODE: u8 = 6;
pub const X_RENDER_FREE_PICTURE_MINOR_OPCODE: u8 = 7;
pub const X_RENDER_COMPOSITE_MINOR_OPCODE: u8 = 8;
/// Never implemented by Xorg; clients scale through transforms instead.
pub const X_RENDER_SCALE_MINOR_OPCODE: u8 = 9;
/// The polygon rasterizer family, declined for now rather than forever: no
/// measured client sends these -- Qt and Chromium composite through GL, Xft
/// needs only glyphs -- and the refusal log will say if one appears. A cairo
/// client drawing over XRender would send these; none has been observed.
pub const X_RENDER_TRAPEZOIDS_MINOR_OPCODE: u8 = 10;
/// Declined with the trapezoid family, for the same reason.
pub const X_RENDER_TRIANGLES_MINOR_OPCODE: u8 = 11;
/// Declined with the trapezoid family, for the same reason.
pub const X_RENDER_TRI_STRIP_MINOR_OPCODE: u8 = 12;
/// Declined with the trapezoid family, for the same reason.
pub const X_RENDER_TRI_FAN_MINOR_OPCODE: u8 = 13;
/// Never implemented by any server; reserved in the protocol and abandoned.
pub const X_RENDER_COLOR_TRAPEZOIDS_MINOR_OPCODE: u8 = 14;
/// Never implemented by any server; reserved in the protocol and abandoned.
pub const X_RENDER_COLOR_TRIANGLES_MINOR_OPCODE: u8 = 15;
// Minor 16 was reserved for a Transform request that never entered the
// protocol; it is not a request at any version and answers BadRequest.
pub const X_RENDER_CREATE_GLYPH_SET_MINOR_OPCODE: u8 = 17;
pub const X_RENDER_REFERENCE_GLYPH_SET_MINOR_OPCODE: u8 = 18;
pub const X_RENDER_FREE_GLYPH_SET_MINOR_OPCODE: u8 = 19;
pub const X_RENDER_ADD_GLYPHS_MINOR_OPCODE: u8 = 20;
/// Never implemented by any server; glyphs arrive through AddGlyphs.
pub const X_RENDER_ADD_GLYPHS_FROM_PICTURE_MINOR_OPCODE: u8 = 21;
pub const X_RENDER_FREE_GLYPHS_MINOR_OPCODE: u8 = 22;
pub const X_RENDER_COMPOSITE_GLYPHS_8_MINOR_OPCODE: u8 = 23;
pub const X_RENDER_COMPOSITE_GLYPHS_16_MINOR_OPCODE: u8 = 24;
pub const X_RENDER_COMPOSITE_GLYPHS_32_MINOR_OPCODE: u8 = 25;
pub const X_RENDER_FILL_RECTANGLES_MINOR_OPCODE: u8 = 26;
/// Client-supplied ARGB cursors, the libXcursor path. Version 0.5.
pub const X_RENDER_CREATE_CURSOR_MINOR_OPCODE: u8 = 27;
/// Picture-space transforms, so a client can scale or rotate what it
/// composites from. Version 0.6.
pub const X_RENDER_SET_PICTURE_TRANSFORM_MINOR_OPCODE: u8 = 28;
/// Version 0.6.
pub const X_RENDER_QUERY_FILTERS_MINOR_OPCODE: u8 = 29;
/// Version 0.6. GTK sends this at startup without consulting the version
/// first, so refusing it ended both GTK3 and GTK4 clients before they drew.
pub const X_RENDER_SET_PICTURE_FILTER_MINOR_OPCODE: u8 = 30;
/// Version 0.8, above what is advertised.
pub const X_RENDER_CREATE_ANIM_CURSOR_MINOR_OPCODE: u8 = 31;
/// Version 0.9, above what is advertised.
pub const X_RENDER_ADD_TRAPS_MINOR_OPCODE: u8 = 32;
/// A source of one colour. Version 0.10.
pub const X_RENDER_CREATE_SOLID_FILL_MINOR_OPCODE: u8 = 33;
/// Version 0.10. Cairo paints widget backgrounds with these and sends
/// them without asking what version the server offers.
pub const X_RENDER_CREATE_LINEAR_GRADIENT_MINOR_OPCODE: u8 = 34;
/// Version 0.10.
pub const X_RENDER_CREATE_RADIAL_GRADIENT_MINOR_OPCODE: u8 = 35;
/// Version 0.10.
pub const X_RENDER_CREATE_CONICAL_GRADIENT_MINOR_OPCODE: u8 = 36;
pub const X_RENDER_LAST_MINOR_OPCODE: u8 = X_RENDER_CREATE_CONICAL_GRADIENT_MINOR_OPCODE;

// The four picture formats Sophia offers, one per representable pixel layout.
// Their identifiers live in low server-owned XID space beside the setup-owned
// root window at 0x20, below every client's resource range.
/// Premultiplied 32-bit ARGB, the depth-32 visual's format.
pub const X_RENDER_FORMAT_ARGB32: u32 = 0x26;
/// 24-bit RGB with no alpha component, the default visual's format.
pub const X_RENDER_FORMAT_RGB24: u32 = 0x27;
/// 8-bit alpha, the mask format antialiased glyph coverage arrives in.
pub const X_RENDER_FORMAT_A8: u32 = 0x28;
/// 1-bit alpha, the mask format for sharp edges.
pub const X_RENDER_FORMAT_A1: u32 = 0x29;
/// PictTypeDirect; Sophia offers no indexed formats.
pub const X_RENDER_PICT_TYPE_DIRECT: u8 = 1;

const X_RENDER_QUERY_VERSION_REQ_LEN: usize = 12;
const X_RENDER_QUERY_PICT_FORMATS_REQ_LEN: usize = 4;
/// The fixed part every trapezoid and triangle request begins with.
const X_RENDER_CREATE_SOLID_FILL_REQ_LEN: usize = 16;
const X_RENDER_PRIMITIVE_PREFIX_LEN: usize = 24;
const X_RENDER_SET_PICTURE_TRANSFORM_REQ_LEN: usize = 44;
const X_RENDER_QUERY_FILTERS_REQ_LEN: usize = 8;
const X_RENDER_SET_PICTURE_FILTER_REQ_LEN: usize = 12;

/// The filters this server offers, and the aliases onto them.
///
/// The protocol's other filter is `convolution`, and it is deliberately
/// absent: a client that finds it missing disables its own kernel work
/// cleanly, and one that finds it advertised and ignored draws something
/// nobody asked for.
pub const X_RENDER_FILTER_NEAREST: &str = "nearest";
pub const X_RENDER_FILTER_BILINEAR: &str = "bilinear";
pub const X_RENDER_FILTER_FAST: &str = "fast";
pub const X_RENDER_FILTER_GOOD: &str = "good";
pub const X_RENDER_FILTER_BEST: &str = "best";

/// Non-rectangular window regions.
///
/// The last extension Quickshell asked for and was refused. A Qt panel sets
/// an input shape so clicks fall through the parts of it that are not the
/// panel, which is why storing shapes without honouring them would be worse
/// than not offering the extension at all.
pub const X_SHAPE_EXTENSION_NAME: &str = "SHAPE";
pub const X_SHAPE_MAJOR_OPCODE: u8 = 145;
/// SHAPE defines exactly one event, `ShapeNotify`, and no errors.
pub const X_SHAPE_FIRST_EVENT: u8 = 70;
pub const X_SHAPE_QUERY_VERSION_MINOR_OPCODE: u8 = 0;
pub const X_SHAPE_RECTANGLES_MINOR_OPCODE: u8 = 1;
pub const X_SHAPE_MASK_MINOR_OPCODE: u8 = 2;
pub const X_SHAPE_COMBINE_MINOR_OPCODE: u8 = 3;
pub const X_SHAPE_OFFSET_MINOR_OPCODE: u8 = 4;
pub const X_SHAPE_QUERY_EXTENTS_MINOR_OPCODE: u8 = 5;
pub const X_SHAPE_SELECT_INPUT_MINOR_OPCODE: u8 = 6;
pub const X_SHAPE_INPUT_SELECTED_MINOR_OPCODE: u8 = 7;
pub const X_SHAPE_GET_RECTANGLES_MINOR_OPCODE: u8 = 8;
pub const X_SHAPE_LAST_MINOR_OPCODE: u8 = X_SHAPE_GET_RECTANGLES_MINOR_OPCODE;
pub const X_SHAPE_MAJOR_VERSION: u16 = 1;
pub const X_SHAPE_MINOR_VERSION: u16 = 1;

/// The three shapes a window carries.
///
/// Bounding is where the window exists at all, Clip is where its own
/// contents are drawn inside that, and Input is where it answers the
/// pointer.
pub const X_SHAPE_KIND_BOUNDING: u8 = 0;
pub const X_SHAPE_KIND_CLIP: u8 = 1;
pub const X_SHAPE_KIND_INPUT: u8 = 2;

pub const X_SHAPE_OP_SET: u8 = 0;
pub const X_SHAPE_OP_UNION: u8 = 1;
pub const X_SHAPE_OP_INTERSECT: u8 = 2;
/// The destination with the source taken out of it.
pub const X_SHAPE_OP_SUBTRACT: u8 = 3;
/// The source with the destination taken out of it -- the mirror of
/// Subtract, not a complement. Worth naming because at least one other
/// implementation aliases this to Set.
pub const X_SHAPE_OP_INVERT: u8 = 4;

/// The orderings a client may claim for the rectangles it sends. All four
/// are accepted and none is trusted: the list is canonicalised on arrival,
/// so a client that mislabels its ordering gets the right answer anyway.
pub const X_SHAPE_ORDERING_UNSORTED: u8 = 0;
pub const X_SHAPE_ORDERING_YX_BANDED: u8 = 3;

const X_SHAPE_QUERY_VERSION_REQ_LEN: usize = 4;
const X_SHAPE_RECTANGLES_REQ_LEN: usize = 16;
const X_SHAPE_MASK_REQ_LEN: usize = 20;
const X_SHAPE_COMBINE_REQ_LEN: usize = 20;
const X_SHAPE_OFFSET_REQ_LEN: usize = 16;
const X_SHAPE_QUERY_EXTENTS_REQ_LEN: usize = 8;
const X_SHAPE_SELECT_INPUT_REQ_LEN: usize = 12;
const X_SHAPE_INPUT_SELECTED_REQ_LEN: usize = 8;
const X_SHAPE_GET_RECTANGLES_REQ_LEN: usize = 12;
