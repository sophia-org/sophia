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

/// Colormap allocation, mutation and installation requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XColormapRequestKind {
    /// Read-write allocation: there are no allocable cells on a TrueColor
    /// visual, so the answer is `BadAlloc`.
    AllocCells,
    AllocPlanes,
    /// Storing into a read-only colormap is `BadAccess`.
    StoreColors,
    StoreNamedColor,
    /// Replace the namespace screen's installed map, or restore its default.
    Install,
    Uninstall,
}

/// What becomes of a client's resources when its connection ends.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum XCloseDownMode {
    #[default]
    Destroy,
    RetainPermanent,
    RetainTemporary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XSaveSetMode {
    Insert,
    Delete,
}

include!("wire/request.rs");
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
    // Xorg checks AllocColorCells' count before its contiguity flag, and
    // AllocColorPlanes' flag before its count.
    let contiguous = (bytes[1] > 1).then_some(u32::from(bytes[1]));
    let no_colors = || (context.byte_order.u16(&bytes[8..10]) == 0).then_some(0);
    let invalid_value = match kind {
        XColormapRequestKind::AllocCells => no_colors().or(contiguous),
        XColormapRequestKind::AllocPlanes => contiguous.or_else(no_colors),
        _ => None,
    };
    Ok(XWireRequest::Core(crate::XCoreRequest::ColormapRequest {
        kind,
        colormap: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
        invalid_value,
    }))
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
            Ok(XWireRequest::Core(crate::XCoreRequest::DeleteProperty {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                property: context.byte_order.u32(&bytes[8..12]),
            }))
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
            Ok(XWireRequest::Core(crate::XCoreRequest::QueryPointer {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
            }))
        }
        X_SET_INPUT_FOCUS => decode_set_input_focus(context, bytes),
        X_GET_INPUT_FOCUS => decode_get_input_focus(bytes),
        X_UNMAP_SUBWINDOWS => {
            require_exact_len(X_UNMAP_SUBWINDOWS, X_UNMAP_SUBWINDOWS_REQ_LEN, bytes.len())?;
            Ok(XWireRequest::Core(crate::XCoreRequest::UnmapSubwindows {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
            }))
        }
        X_CIRCULATE_WINDOW => {
            require_exact_len(X_CIRCULATE_WINDOW, X_CIRCULATE_WINDOW_REQ_LEN, bytes.len())?;
            if bytes[1] > 1 {
                return Err(XWireParseError::InvalidValue(u32::from(bytes[1])));
            }
            Ok(XWireRequest::Core(crate::XCoreRequest::CirculateWindow {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                direction: bytes[1],
            }))
        }
        X_CHANGE_ACTIVE_POINTER_GRAB => decode_change_active_pointer_grab(context, bytes),
        X_ROTATE_PROPERTIES => decode_rotate_properties(context, bytes),
        X_CHANGE_SAVE_SET => {
            require_exact_len(X_CHANGE_SAVE_SET, X_CHANGE_SAVE_SET_REQ_LEN, bytes.len())?;
            let mode = match bytes[1] {
                0 => XSaveSetMode::Insert,
                1 => XSaveSetMode::Delete,
                other => return Err(XWireParseError::InvalidValue(u32::from(other))),
            };
            let raw = context.byte_order.u32(&bytes[4..8]);
            Ok(XWireRequest::Core(crate::XCoreRequest::ChangeSaveSet {
                window: XResourceId::new(u64::from(raw), 1),
                mode,
                own_window: context
                    .resource_id_range
                    .is_some_and(|range| range.owns_new_resource(raw)),
            }))
        }
        X_SET_CLOSE_DOWN_MODE => {
            require_exact_len(
                X_SET_CLOSE_DOWN_MODE,
                X_SET_CLOSE_DOWN_MODE_REQ_LEN,
                bytes.len(),
            )?;
            let mode = match bytes[1] {
                0 => XCloseDownMode::Destroy,
                1 => XCloseDownMode::RetainPermanent,
                2 => XCloseDownMode::RetainTemporary,
                other => return Err(XWireParseError::InvalidValue(u32::from(other))),
            };
            Ok(XWireRequest::Core(crate::XCoreRequest::SetCloseDownMode {
                mode,
            }))
        }
        X_KILL_CLIENT => {
            require_exact_len(X_KILL_CLIENT, X_KILL_CLIENT_REQ_LEN, bytes.len())?;
            let raw = context.byte_order.u32(&bytes[4..8]);
            Ok(XWireRequest::Core(crate::XCoreRequest::KillClient {
                resource: (raw != 0).then(|| XResourceId::new(u64::from(raw), 1)),
            }))
        }
        X_SET_POINTER_MAPPING => {
            let count = usize::from(bytes[1]);
            require_exact_len(
                X_SET_POINTER_MAPPING,
                X_SET_POINTER_MAPPING_REQ_LEN + ((count + 3) & !3),
                bytes.len(),
            )?;
            Ok(XWireRequest::Core(crate::XCoreRequest::SetPointerMapping {
                mapping: bytes[4..4 + count].to_vec(),
            }))
        }
        X_CHANGE_KEYBOARD_MAPPING => {
            require_len(
                X_CHANGE_KEYBOARD_MAPPING,
                X_CHANGE_KEYBOARD_MAPPING_REQ_LEN,
                bytes.len(),
            )?;
            let count = usize::from(bytes[1]);
            let per_keycode = bytes[5];
            if per_keycode == 0 {
                return Err(XWireParseError::InvalidValue(0));
            }
            require_exact_len(
                X_CHANGE_KEYBOARD_MAPPING,
                X_CHANGE_KEYBOARD_MAPPING_REQ_LEN + count * usize::from(per_keycode) * 4,
                bytes.len(),
            )?;
            let keysyms = bytes[8..]
                .chunks_exact(4)
                .map(|word| context.byte_order.u32(word))
                .collect();
            Ok(XWireRequest::Core(
                crate::XCoreRequest::ChangeKeyboardMapping {
                    first_keycode: bytes[4],
                    keysyms_per_keycode: per_keycode,
                    keysyms,
                },
            ))
        }
        X_SET_MODIFIER_MAPPING => {
            let per_modifier = usize::from(bytes[1]);
            require_exact_len(
                X_SET_MODIFIER_MAPPING,
                X_SET_MODIFIER_MAPPING_REQ_LEN + 8 * per_modifier,
                bytes.len(),
            )?;
            Ok(XWireRequest::Core(
                crate::XCoreRequest::SetModifierMapping {
                    keycodes_per_modifier: bytes[1],
                    keycodes: bytes[4..].to_vec(),
                },
            ))
        }
        X_QUERY_KEYMAP => {
            require_exact_len(X_QUERY_KEYMAP, X_QUERY_KEYMAP_REQ_LEN, bytes.len())?;
            Ok(XWireRequest::Core(crate::XCoreRequest::QueryKeymap))
        }
        X_CHANGE_KEYBOARD_CONTROL => decode_change_keyboard_control(context, bytes),
        X_CHANGE_POINTER_CONTROL => decode_change_pointer_control(context, bytes),
        X_GET_POINTER_CONTROL => {
            require_exact_len(
                X_GET_POINTER_CONTROL,
                X_GET_POINTER_CONTROL_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Core(crate::XCoreRequest::GetPointerControl))
        }
        X_SET_SCREEN_SAVER => decode_set_screen_saver(context, bytes),
        X_GET_SCREEN_SAVER => {
            require_exact_len(X_GET_SCREEN_SAVER, X_GET_SCREEN_SAVER_REQ_LEN, bytes.len())?;
            Ok(XWireRequest::Core(crate::XCoreRequest::GetScreenSaver))
        }
        X_GET_MOTION_EVENTS => {
            require_exact_len(
                X_GET_MOTION_EVENTS,
                X_GET_MOTION_EVENTS_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Core(crate::XCoreRequest::GetMotionEvents {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                start: context.byte_order.u32(&bytes[8..12]),
                stop: context.byte_order.u32(&bytes[12..16]),
            }))
        }
        X_LIST_HOSTS => {
            require_exact_len(X_LIST_HOSTS, X_LIST_HOSTS_REQ_LEN, bytes.len())?;
            Ok(XWireRequest::Core(crate::XCoreRequest::ListHosts))
        }
        X_CHANGE_HOSTS => decode_change_hosts(context, bytes),
        X_SET_ACCESS_CONTROL => {
            require_exact_len(
                X_SET_ACCESS_CONTROL,
                X_SET_ACCESS_CONTROL_REQ_LEN,
                bytes.len(),
            )?;
            if bytes[1] > 1 {
                return Err(XWireParseError::InvalidValue(u32::from(bytes[1])));
            }
            Ok(XWireRequest::Core(crate::XCoreRequest::SetAccessControl))
        }
        X_GET_KEYBOARD_CONTROL => {
            require_exact_len(X_GET_KEYBOARD_CONTROL, 4, bytes.len())?;
            Ok(XWireRequest::Core(crate::XCoreRequest::GetKeyboardControl))
        }
        X_BELL => {
            require_exact_len(X_BELL, 4, bytes.len())?;
            Ok(XWireRequest::Core(crate::XCoreRequest::Bell))
        }
        X_FORCE_SCREEN_SAVER => {
            require_exact_len(X_FORCE_SCREEN_SAVER, 4, bytes.len())?;
            // An out-of-range mode is a Value error the client must see with
            // its sequence, not a decode failure, so it is carried through.
            Ok(XWireRequest::Core(crate::XCoreRequest::ForceScreenSaver {
                mode: bytes[1],
            }))
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
            Ok(XWireRequest::Core(
                crate::XCoreRequest::CopyGraphicsContext {
                    source: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                    destination: XResourceId::new(
                        u64::from(context.byte_order.u32(&bytes[8..12])),
                        1,
                    ),
                    value_mask,
                },
            ))
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
            Ok(XWireRequest::Core(crate::XCoreRequest::SetDashes {
                gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                dash_offset: context.byte_order.u16(&bytes[8..10]),
                dashes: bytes[X_SET_DASHES_REQ_LEN..X_SET_DASHES_REQ_LEN + dash_len].to_vec(),
            }))
        }
        X_SET_FONT_PATH => {
            require_len(X_SET_FONT_PATH, X_SET_FONT_PATH_REQ_LEN, bytes.len())?;
            // The path list frames the request exactly: a count, then
            // strings each led by its length, padded to four.
            let count = usize::from(context.byte_order.u16(&bytes[4..6]));
            let mut cursor = X_SET_FONT_PATH_REQ_LEN;
            for _ in 0..count {
                let len = *bytes.get(cursor).ok_or(XWireParseError::InvalidLength {
                    opcode: X_SET_FONT_PATH,
                    expected_at_least: cursor + 1,
                    actual: bytes.len(),
                })?;
                cursor += 1 + usize::from(len);
            }
            require_exact_len(X_SET_FONT_PATH, (cursor + 3) & !3, bytes.len())?;
            Ok(XWireRequest::Core(crate::XCoreRequest::SetFontPath))
        }
        X_GET_FONT_PATH => {
            require_exact_len(X_GET_FONT_PATH, X_GET_FONT_PATH_REQ_LEN, bytes.len())?;
            Ok(XWireRequest::Core(crate::XCoreRequest::GetFontPath))
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
            Ok(XWireRequest::Core(crate::XCoreRequest::CopyPlane {
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
            }))
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
            Ok(XWireRequest::Core(crate::XCoreRequest::FreeColormap {
                colormap: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
            }))
        }
        X_COPY_COLORMAP_AND_FREE => decode_copy_colormap_and_free(context, bytes),
        X_LIST_INSTALLED_COLORMAPS => {
            require_exact_len(
                X_LIST_INSTALLED_COLORMAPS,
                X_LIST_INSTALLED_COLORMAPS_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Core(
                crate::XCoreRequest::ListInstalledColormaps {
                    window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                },
            ))
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
        X_FREE_COLORS => decode_free_colors(context, bytes),
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
            Ok(XWireRequest::Extension(
                crate::XExtensionRequest::GeQueryVersion {
                    major_version: context.byte_order.u16(&bytes[4..6]),
                    minor_version: context.byte_order.u16(&bytes[6..8]),
                },
            ))
        }
        // Deliberately no length check beyond the preamble's: NoOperation may
        // carry any amount of padding, and all of it is ignored.
        X_NO_OPERATION => Ok(XWireRequest::Core(crate::XCoreRequest::NoOperation)),
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
