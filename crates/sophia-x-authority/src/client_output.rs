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
const X_KEYMAP_NOTIFY: u8 = 11;
const X_FOCUS_OUT: u8 = 10;
const X_EXPOSE: u8 = 12;
const X_GRAPHICS_EXPOSE: u8 = 13;
const X_NO_EXPOSE: u8 = 14;
const X_VISIBILITY_NOTIFY: u8 = 15;
const X_GRAVITY_NOTIFY: u8 = 24;
const X_COLORMAP_NOTIFY: u8 = 32;
const X_DESTROY_NOTIFY: u8 = 17;
const X_UNMAP_NOTIFY: u8 = 18;
const X_MAP_NOTIFY: u8 = 19;
const X_MAP_REQUEST: u8 = 20;
const X_CONFIGURE_REQUEST: u8 = 23;
const X_RESIZE_REQUEST: u8 = 25;
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
        /// Where the pointer is: a key event carries the pointer's root and
        /// event-window coordinates.
        root_x: i16,
        root_y: i16,
        event_x: i16,
        event_y: i16,
        state: u16,
    },
    Focus {
        sequence: u16,
        focused: bool,
        detail: u8,
        event: XResourceId,
        mode: u8,
    },
    /// The keys down, reported after every EnterNotify and FocusIn to the
    /// clients that selected KeymapState on the window entered or focused.
    /// It carries no sequence number: bytes 1 to 31 are the bitmap's bytes
    /// 1 to 31, QueryKeymap's without its first byte.
    KeymapNotify { keys: [u8; 31] },
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
    /// A window's colormap attribute changed (`new`), or the colormap it
    /// names was installed or uninstalled. `colormap` is raw: zero is None,
    /// which a freed colormap leaves behind.
    ColormapNotify {
        sequence: u16,
        window: XResourceId,
        colormap: u32,
        new: bool,
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
    /// A child moved by its win-gravity when its parent was resized (t199):
    /// to the child (StructureNotify) and its parent (SubstructureNotify).
    GravityNotify {
        sequence: u16,
        event: XResourceId,
        window: XResourceId,
        x: i16,
        y: i16,
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
    /// A ConfigureWindow on a child of a window another client manages
    /// (SubstructureRedirect selected on the parent): the request as made,
    /// unset fields carrying the current values, and nothing applied.
    ConfigureRequest {
        sequence: u16,
        stack_mode: u8,
        parent: XResourceId,
        window: XResourceId,
        sibling: XResourceId,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        border_width: u16,
        value_mask: u16,
    },
    /// A size change on a window another client selected ResizeRedirect
    /// on: the size asked for, and the size not applied.
    ResizeRequest {
        sequence: u16,
        window: XResourceId,
        width: u16,
        height: u16,
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
        /// Where the SendEvent aimed it, resolved from PointerWindow or
        /// InputFocus, and how (t182): with no mask the destination's owner
        /// is owed it, with one every client selecting those events there,
        /// climbing the ancestors when `propagate` is set and nobody on the
        /// window selected. None of this reaches the wire; the record does.
        destination: XResourceId,
        event_mask: u32,
        propagate: bool,
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

include!("client_output/reply.rs");
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
