use crate::image::X_IMAGE_FORMAT_Z_PIXMAP;
use crate::{
    X_ATOM_NONE, X_BIG_REQUESTS_EXTENSION_NAME, X_BIG_REQUESTS_MAJOR_OPCODE,
    X_MIT_SHM_EXTENSION_NAME, X_MIT_SHM_MAJOR_OPCODE, X_RANDR_EXTENSION_NAME, X_RANDR_MAJOR_OPCODE,
    X_SETUP_ARGB_VISUAL, X_SETUP_DEFAULT_COLORMAP, X_SETUP_DEFAULT_ROOT, X_SETUP_DEFAULT_VISUAL,
    X_SOPHIA_PRESENT_EXTENSION_NAME, X_SOPHIA_PRESENT_MAJOR_OPCODE, XAtomTable,
    XAuthorityRequestKind, XAuthorityResponseOutcome, XAuthorityResponsePacket, XAuthorityRuntime,
    XAuthorityRuntimeError, XByteOrder, XClientEvent, XClientOutput, XClientReply, XColorRgb16,
    XColormapError, XErrorCode, XGlxContextConfig, XMetadataPropertyCandidate, XPolyTextItem,
    XPropertyError, XPropertyTable, XPutImageSemantics, XRandrModeInfo, XRandrMonitorInfo,
    XResourceId, XTextDraw, XWindowGeometryUpdate, XWireParseError, XWireRequest, XXiDeviceClass,
    XXiDeviceInfo, XXiLegacyDeviceClass, XXiLegacyDeviceInfo, decode_x_size_hints,
    decode_x_transient_for, decode_x_window_type_facts, encode_x_client_output,
    metadata_property_candidate, x_error_from_runtime, x_error_from_wire_parse,
    x_lookup_color_name, x_selection_failure_event, x_true_color_visual,
};
use sophia_protocol::{NamespaceId, OutputTopologySnapshot, Rect, Region, TransactionId};

include!("dispatch/active_window.rs");
include!("dispatch/core/drawing.rs");
include!("dispatch/core/grabs.rs");
include!("dispatch/core/input_discovery.rs");
include!("dispatch/core/properties.rs");
include!("dispatch/core/resources.rs");
include!("dispatch/core/windows.rs");
include!("dispatch/extensions/dri3.rs");
include!("dispatch/extensions/glx.rs");
include!("dispatch/extensions/present.rs");
include!("dispatch/extensions/randr.rs");
include!("dispatch/extensions/shm.rs");
include!("dispatch/extensions/sync.rs");
include!("dispatch/extensions/versions.rs");
include!("dispatch/extensions/xi.rs");
include!("dispatch/extensions/xfixes.rs");
include!("dispatch/extensions/xf86_vidmode.rs");
include!("dispatch/extensions/xc_misc.rs");
include!("dispatch/extensions/render.rs");
include!("dispatch/extensions/shape.rs");
include!("dispatch/extensions/xtest.rs");
include!("dispatch/extensions/xkb.rs");

/// The GLX extensions Sophia offers.
///
/// The ES profiles are here because a client that translates to OpenGL ES --
/// which is how Chromium's ANGLE reaches a GL driver -- asks for an ES-profile
/// context, and libGL refuses that request against a server that does not
/// advertise them, before the server ever sees it. A client rendering desktop
/// GL never notices their absence, which is why one browser worked here and
/// another did not.
///
/// Advertising them is honest: Sophia runs no GL of its own. A context is
/// created by the client's driver and recorded here, so the profile it asks for
/// is the client's business and any profile it can create, Sophia can record.
const GLX_EXTENSIONS: &str = "GLX_EXT_libglvnd GLX_ARB_create_context GLX_ARB_create_context_profile GLX_ARB_framebuffer_sRGB GLX_EXT_framebuffer_sRGB GLX_EXT_create_context_es_profile GLX_EXT_create_context_es2_profile";
/// The same list plus texture-from-pixmap, where a provider backs it.
///
/// A client that derives EGL configurations from GLX consults this string as
/// well as the per-configuration bits: it emits a bind-capable configuration
/// only when both agree, so the string and the attributes are advertised from
/// the one capability.
const GLX_EXTENSIONS_WITH_PIXMAP_TEXTURES: &str = "GLX_EXT_libglvnd GLX_ARB_create_context GLX_ARB_create_context_profile GLX_ARB_framebuffer_sRGB GLX_EXT_framebuffer_sRGB GLX_EXT_create_context_es_profile GLX_EXT_create_context_es2_profile GLX_EXT_texture_from_pixmap";

const fn glx_extensions(pixmap_textures: bool) -> &'static str {
    if pixmap_textures {
        GLX_EXTENSIONS_WITH_PIXMAP_TEXTURES
    } else {
        GLX_EXTENSIONS
    }
}

fn glx_visual_configs() -> Vec<[u32; 18]> {
    crate::X_GLX_FB_CONFIGS[..2]
        .iter()
        .map(|config| {
            [
                config.visual,
                4,
                1,
                8,
                8,
                8,
                config.alpha,
                0,
                0,
                0,
                0,
                1,
                0,
                u32::from(config.color_bits()),
                24,
                config.stencil,
                0,
                0,
            ]
        })
        .collect()
}

/// Both GLX query versions describe the same color buffer, independently of
/// the native X visual's depth.
fn glx_fb_config(config: crate::XGlxFbConfig, pixmap_textures: bool) -> Vec<(u32, u32)> {
    let crate::XGlxFbConfig {
        id,
        visual,
        alpha,
        srgb,
        stencil,
        ..
    } = config;
    let mut attributes = vec![
        (crate::X_GLX_FBCONFIG_ID_ATTRIBUTE, id),
        (crate::X_GLX_VISUAL_ID_ATTRIBUTE, visual),
        (crate::X_GLX_X_RENDERABLE_ATTRIBUTE, 1),
        (
            crate::X_GLX_DRAWABLE_TYPE_ATTRIBUTE,
            crate::x_glx_drawable_type_mask(pixmap_textures),
        ),
        (
            crate::X_GLX_RENDER_TYPE_ATTRIBUTE,
            crate::X_GLX_RGBA_BIT_VALUE,
        ),
        (
            crate::X_GLX_X_VISUAL_TYPE_ATTRIBUTE,
            crate::X_GLX_TRUE_COLOR_VALUE,
        ),
        (
            crate::X_GLX_BUFFER_SIZE_ATTRIBUTE,
            u32::from(config.color_bits()),
        ),
        (crate::X_GLX_LEVEL_ATTRIBUTE, 0),
        (crate::X_GLX_DOUBLEBUFFER_ATTRIBUTE, 1),
        (crate::X_GLX_STEREO_ATTRIBUTE, 0),
        (crate::X_GLX_AUX_BUFFERS_ATTRIBUTE, 0),
        (crate::X_GLX_RED_SIZE_ATTRIBUTE, 8),
        (crate::X_GLX_GREEN_SIZE_ATTRIBUTE, 8),
        (crate::X_GLX_BLUE_SIZE_ATTRIBUTE, 8),
        (crate::X_GLX_ALPHA_SIZE_ATTRIBUTE, alpha),
        (crate::X_GLX_DEPTH_SIZE_ATTRIBUTE, 24),
        (crate::X_GLX_STENCIL_SIZE_ATTRIBUTE, stencil),
        (crate::X_GLX_ACCUM_RED_SIZE_ATTRIBUTE, 0),
        (crate::X_GLX_ACCUM_GREEN_SIZE_ATTRIBUTE, 0),
        (crate::X_GLX_ACCUM_BLUE_SIZE_ATTRIBUTE, 0),
        (crate::X_GLX_ACCUM_ALPHA_SIZE_ATTRIBUTE, 0),
        (
            crate::X_GLX_TRANSPARENT_TYPE_ATTRIBUTE,
            crate::X_GLX_NONE_VALUE,
        ),
        (
            crate::X_GLX_CONFIG_CAVEAT_ATTRIBUTE,
            crate::X_GLX_NONE_VALUE,
        ),
        // GLX 1.4's multisample attributes, answered as zero rather than
        // omitted: a client asking what Sophia offers gets "none", not silence.
        (crate::X_GLX_SAMPLE_BUFFERS_ATTRIBUTE, 0),
        (crate::X_GLX_SAMPLES_ATTRIBUTE, 0),
        (crate::X_GLX_FRAMEBUFFER_SRGB_CAPABLE_ATTRIBUTE, srgb),
        // Appended, because the catalog is read positionally by its tests and by
        // clients that index the reply. The maxima are the same constants the
        // pbuffer refusal enforces.
        (
            crate::X_GLX_MAX_PBUFFER_WIDTH_ATTRIBUTE,
            crate::X_GLX_MAX_PBUFFER_WIDTH,
        ),
        (
            crate::X_GLX_MAX_PBUFFER_HEIGHT_ATTRIBUTE,
            crate::X_GLX_MAX_PBUFFER_HEIGHT,
        ),
        (
            crate::X_GLX_MAX_PBUFFER_PIXELS_ATTRIBUTE,
            crate::X_GLX_MAX_PBUFFER_PIXELS,
        ),
    ];
    // Appended for the same reason as the maxima above: a reader that indexes
    // the reply keeps its offsets, and a server without pixmap textures emits
    // exactly the rows and attributes it always did.
    if pixmap_textures {
        attributes.extend([
            (
                crate::X_GLX_BIND_TO_TEXTURE_RGB_ATTRIBUTE,
                u32::from(config.bind_to_texture_rgb()),
            ),
            (
                crate::X_GLX_BIND_TO_TEXTURE_RGBA_ATTRIBUTE,
                u32::from(config.bind_to_texture_rgba()),
            ),
            // Advertised even though it is false: a driver comparing these for
            // equality reads an absent attribute differently from a false one.
            (
                crate::X_GLX_BIND_TO_MIPMAP_TEXTURE_ATTRIBUTE,
                u32::from(config.bind_to_mipmap_texture()),
            ),
            (
                crate::X_GLX_BIND_TO_TEXTURE_TARGETS_ATTRIBUTE,
                crate::X_GLX_TEXTURE_TARGETS_ALL,
            ),
            (crate::X_GLX_Y_INVERTED_ATTRIBUTE, 1),
        ]);
    }
    attributes
}

fn glx_fb_configs(pixmap_textures: bool) -> Vec<Vec<(u32, u32)>> {
    crate::x_glx_fb_configs(pixmap_textures)
        .iter()
        .copied()
        .map(|config| glx_fb_config(config, pixmap_textures))
        .collect()
}

fn glx_bad_value(context: &XDispatchContext, value: u32, minor: u8) -> XClientOutput {
    XClientOutput::Error(crate::XClientError {
        code: XErrorCode::BadValue,
        sequence: context.sequence,
        resource_id: value,
        minor_code: u16::from(minor),
        major_code: context.major_opcode,
    })
}

/// Whether this connection may fake input at the seat.
///
/// Rides the dispatch context so discovery and every request path share one
/// decision. Two decisions in two places could disagree, and the disagreement
/// would be a client that can see an extension it may not use.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum XTestAdmission {
    /// XTEST is absent for this client: missing from QueryExtension and
    /// ListExtensions, and every guessed opcode on its major answers
    /// BadAccess. The default, and what every connection outside an
    /// explicitly admitted private instance gets.
    #[default]
    Absent,
    Admitted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XDispatchContext {
    pub byte_order: XByteOrder,
    pub namespace: NamespaceId,
    /// Frontend-global identity for every Engine-visible effect of this request.
    /// The X11 sequence below is connection-local and exists only for wire
    /// replies, events, and errors.
    pub transaction: TransactionId,
    pub sequence: u16,
    pub major_opcode: u8,
    pub client_id: u64,
    pub injection: XTestAdmission,
    /// The server time this request is being served at, stamped once so
    /// every event the request generates carries the same value.
    ///
    /// Zero is the one timestamp a server may never generate: it is
    /// `CurrentTime` on the wire, and a client that reads it back cannot
    /// use it. Events left at zero are why the conformance suite's own
    /// `gettime` helper, which reads the time out of a PropertyNotify,
    /// could not establish a server time at all.
    pub server_time: crate::XTimestamp,
}

#[derive(Clone, Debug, PartialEq)]
pub struct XDispatchResult {
    pub response: Option<XAuthorityResponsePacket>,
    pub outputs: Vec<XClientOutput>,
    pub metadata_candidates: Vec<XMetadataPropertyCandidate>,
}

impl XDispatchResult {
    pub fn encoded_outputs(&self, byte_order: XByteOrder) -> Vec<Vec<u8>> {
        self.outputs
            .iter()
            .map(|output| encode_x_client_output(byte_order, output.clone()))
            .collect()
    }
}

enum XDispatchFamilyResult {
    Handled(XDispatchResult),
    Unhandled(XWireRequest),
}

use XDispatchFamilyResult::{Handled, Unhandled};

fn xkb_empty_device_reply(
    context: XDispatchContext,
    device_spec: u16,
    minor_opcode: u8,
    reply: impl FnOnce(u16, u8) -> XClientReply,
) -> XDispatchResult {
    const XKB_USE_CORE_KBD: u16 = 0x0100;
    let output = if matches!(device_spec, XKB_USE_CORE_KBD | 3) {
        XClientOutput::Reply(reply(context.sequence, 3))
    } else {
        XClientOutput::Error(crate::XClientError {
            code: XErrorCode::BadValue,
            sequence: context.sequence,
            resource_id: u32::from(device_spec),
            minor_code: minor_opcode.into(),
            major_code: context.major_opcode,
        })
    };
    XDispatchResult {
        response: None,
        outputs: vec![output],
        metadata_candidates: Vec::new(),
    }
}

pub fn dispatch_x11_wire_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
    properties: &mut XPropertyTable,
) -> XDispatchResult {
    let mut result = dispatch_x11_wire_request_inner(context, request, runtime, atoms, properties);
    // A request that made the focus window unviewable moved the focus by
    // itself. What that owes belongs with this request's own output, because
    // the protocol orders a reversion's FocusOut after the UnmapNotify that
    // caused it and before anything the client does next.
    result
        .outputs
        .extend(focus_reversion_outputs(context, runtime));
    // Behaviour behind `_NET_ACTIVE_WINDOW`: whatever this request did to the
    // input focus is on the root before the client hears the result.
    publish_noted_focus(runtime, properties, atoms, context.byte_order);
    result
}

fn dispatch_x11_wire_request_inner(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
    properties: &mut XPropertyTable,
) -> XDispatchResult {
    runtime.begin_dispatch();
    let request = match dispatch_xfixes_request(context, request, runtime, atoms) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_xf86_vidmode_request(context, request, runtime, atoms) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_xc_misc_request(context, request, runtime) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_render_request(context, request, runtime) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_render_picture_request(context, request, runtime) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_render_glyph_request(context, request, runtime) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_shape_request(context, request, runtime) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_xtest_request(context, request, runtime) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_dri3_request(context, request, runtime) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_present_request(context, request, runtime) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_randr_request(context, request, runtime, atoms) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_extension_version_request(context, request, runtime, atoms) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_xkb_request(context, request, runtime, atoms) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_glx_request(context, request, runtime, atoms) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_sync_request(context, request, runtime, atoms) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_x_input_request(context, request, runtime, atoms) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_shm_request(context, request, runtime, atoms) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_core_window_request(context, request, runtime, atoms, properties) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_core_property_request(context, request, runtime, atoms, properties)
    {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_core_grab_request(context, request, runtime, atoms, properties) {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request = match dispatch_core_resource_request(context, request, runtime, atoms, properties)
    {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    let request =
        match dispatch_core_input_discovery_request(context, request, runtime, atoms, properties) {
            Handled(result) => return result,
            Unhandled(request) => request,
        };
    let _request = match dispatch_core_drawing_request(context, request, runtime, atoms, properties)
    {
        Handled(result) => return result,
        Unhandled(request) => request,
    };
    unreachable!("extension request escaped its family dispatcher")
}

fn dispatch_text_draw(
    context: XDispatchContext,
    runtime: &mut XAuthorityRuntime,
    drawable: XResourceId,
    gc: XResourceId,
    mut draw: XTextDraw<'_>,
) -> XDispatchResult {
    let transaction = context.transaction;
    if let Err(error) = runtime.validate_drawable_access(context.namespace, drawable) {
        return core_draw_validation_error(
            context,
            transaction,
            error,
            XErrorCode::BadDrawable,
            drawable,
        );
    }
    let (gc_depth, gc_values, font) =
        match runtime.graphics_context_depth_values_and_font(context.namespace, gc) {
            Ok(record) => record,
            Err(error) => {
                return core_draw_validation_error(
                    context,
                    transaction,
                    error,
                    XErrorCode::BadGraphicsContext,
                    gc,
                );
            }
        };
    if runtime.drawable_depth(context.namespace, drawable) != Ok(gc_depth) {
        return core_draw_validation_error(
            context,
            transaction,
            XAuthorityRuntimeError::InvalidSurface,
            XErrorCode::BadMatch,
            drawable,
        );
    }
    draw.font = font;
    let response = runtime.draw_through_inferiors(
        transaction,
        context.namespace,
        drawable,
        gc_values.subwindow_mode,
        |runtime| {
            runtime.apply_text_draw(
                transaction,
                context.namespace,
                drawable,
                &[draw],
                &gc_values,
            )
        },
    );
    let outputs = if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
        vec![XClientOutput::Error(x_error_from_runtime(
            error,
            context.sequence,
            context.major_opcode,
            0,
            u32::try_from(drawable.local.raw()).unwrap_or(0),
        ))]
    } else {
        Vec::new()
    };
    XDispatchResult {
        response: Some(response),
        outputs,
        metadata_candidates: Vec::new(),
    }
}

fn dispatch_poly_text(
    context: XDispatchContext,
    runtime: &mut XAuthorityRuntime,
    drawable: XResourceId,
    gc: XResourceId,
    x: i16,
    baseline: i16,
    items: &[XPolyTextItem],
) -> XDispatchResult {
    let transaction = context.transaction;
    if let Err(error) = runtime.validate_drawable_access(context.namespace, drawable) {
        return core_draw_validation_error(
            context,
            transaction,
            error,
            XErrorCode::BadDrawable,
            drawable,
        );
    }
    let (gc_depth, gc_values, mut font) =
        match runtime.graphics_context_depth_values_and_font(context.namespace, gc) {
            Ok(record) => record,
            Err(error) => {
                return core_draw_validation_error(
                    context,
                    transaction,
                    error,
                    XErrorCode::BadGraphicsContext,
                    gc,
                );
            }
        };
    if runtime.drawable_depth(context.namespace, drawable) != Ok(gc_depth) {
        return core_draw_validation_error(
            context,
            transaction,
            XAuthorityRuntimeError::InvalidSurface,
            XErrorCode::BadMatch,
            drawable,
        );
    }

    let mut current_x = i32::from(x);
    let mut draws = Vec::new();
    let mut font_error = None;
    for item in items {
        match item {
            XPolyTextItem::Text { delta, chars } => {
                current_x = current_x.saturating_add(i32::from(*delta));
                let advance = font.metrics.text_extents(chars).overall_width;
                draws.push(XTextDraw {
                    x: current_x,
                    baseline: i32::from(baseline),
                    text: chars,
                    image: false,
                    font: font.clone(),
                });
                current_x = current_x.saturating_add(advance);
            }
            XPolyTextItem::Font { font: requested } => {
                match runtime.font_face(context.namespace, *requested) {
                    Ok(resolved) => font = resolved,
                    Err(error) => {
                        let code = if matches!(
                            error,
                            XAuthorityRuntimeError::InvalidNamespace
                                | XAuthorityRuntimeError::CrossNamespaceDenied
                        ) {
                            XErrorCode::BadAccess
                        } else {
                            XErrorCode::BadFont
                        };
                        font_error = Some(XClientOutput::Error(crate::XClientError {
                            code,
                            sequence: context.sequence,
                            resource_id: u32::try_from(requested.local.raw()).unwrap_or(0),
                            minor_code: 0,
                            major_code: context.major_opcode,
                        }));
                        break;
                    }
                }
            }
        }
    }

    let response = runtime.draw_through_inferiors(
        transaction,
        context.namespace,
        drawable,
        gc_values.subwindow_mode,
        |runtime| {
            runtime.apply_text_draw(transaction, context.namespace, drawable, &draws, &gc_values)
        },
    );
    let outputs = if let Some(error) = font_error {
        vec![error]
    } else if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
        vec![XClientOutput::Error(x_error_from_runtime(
            error,
            context.sequence,
            context.major_opcode,
            0,
            u32::try_from(drawable.local.raw()).unwrap_or(0),
        ))]
    } else {
        Vec::new()
    };
    XDispatchResult {
        response: Some(response),
        outputs,
        metadata_candidates: Vec::new(),
    }
}

fn grab_access_error(context: &XDispatchContext, window: XResourceId) -> XClientOutput {
    XClientOutput::Error(crate::XClientError {
        code: XErrorCode::BadAccess,
        sequence: context.sequence,
        resource_id: u32::try_from(window.local.raw()).unwrap_or(0),
        minor_code: 0,
        major_code: context.major_opcode,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct XExtensionQueryResult {
    present: bool,
    major_opcode: u8,
    first_event: u8,
    first_error: u8,
}

/// A client-supplied extension name, made safe to put in a log.
///
/// The name arrives as arbitrary bytes from a client that has not been
/// authenticated for anything in particular. Newlines and control characters
/// would let it forge evidence lines in a log an operator reads, and an
/// unbounded name would let it flood one, so both are cut off here.
fn loggable_extension_name(name: &str) -> String {
    const MAX_LOGGED_NAME_BYTES: usize = 64;
    name.chars()
        .take(MAX_LOGGED_NAME_BYTES)
        .map(|character| {
            if character.is_ascii_graphic() || character == ' ' {
                character
            } else {
                '.'
            }
        })
        .collect()
}

/// Every extension this authority advertises.
///
/// `ListExtensions` answers from here and `extension_query_result` answers per
/// name, so the two can disagree. A test asserts every name here reports
/// present, and that nothing outside it does -- a client that enumerates and
/// then queries must not be told different things.
pub(crate) fn advertised_extension_names(injection: XTestAdmission) -> Vec<String> {
    [
        X_SOPHIA_PRESENT_EXTENSION_NAME,
        X_MIT_SHM_EXTENSION_NAME,
        crate::X_DRI3_EXTENSION_NAME,
        crate::X_PRESENT_EXTENSION_NAME,
        crate::X_XFIXES_EXTENSION_NAME,
        crate::X_XC_MISC_EXTENSION_NAME,
        crate::X_RENDER_EXTENSION_NAME,
        crate::X_SHAPE_EXTENSION_NAME,
        crate::X_XF86_VIDMODE_EXTENSION_NAME,
        crate::X_GLX_EXTENSION_NAME,
        crate::X_SYNC_EXTENSION_NAME,
        X_RANDR_EXTENSION_NAME,
        crate::X_KEYBOARD_EXTENSION_NAME,
        crate::X_INPUT_EXTENSION_NAME,
        crate::X_GENERIC_EVENT_EXTENSION_NAME,
        X_BIG_REQUESTS_EXTENSION_NAME,
    ]
    .into_iter()
    .chain(
        // Named to a client that may use it and to nobody else. A client
        // refused injection must not find XTEST here and then meet BadAccess
        // on every request: that would be the server contradicting itself
        // within one connection.
        matches!(injection, XTestAdmission::Admitted).then_some(crate::X_TEST_EXTENSION_NAME),
    )
    .map(str::to_owned)
    .collect()
}

fn extension_query_result(name: &str, injection: XTestAdmission) -> XExtensionQueryResult {
    // Answered before the table, because this is the one name whose presence
    // depends on who is asking. Everything below is the same for every
    // client.
    if name == crate::X_TEST_EXTENSION_NAME {
        return XExtensionQueryResult {
            present: matches!(injection, XTestAdmission::Admitted),
            major_opcode: crate::X_TEST_MAJOR_OPCODE,
            // XTEST defines no events and no errors, so both bases are zero
            // and it raises core errors only.
            first_event: crate::X_TEST_FIRST_EVENT,
            first_error: crate::X_TEST_FIRST_ERROR,
        };
    }
    match name {
        X_SOPHIA_PRESENT_EXTENSION_NAME => XExtensionQueryResult {
            present: true,
            major_opcode: X_SOPHIA_PRESENT_MAJOR_OPCODE,
            first_event: 0,
            first_error: 0,
        },
        X_MIT_SHM_EXTENSION_NAME => XExtensionQueryResult {
            present: true,
            major_opcode: X_MIT_SHM_MAJOR_OPCODE,
            first_event: crate::X_MIT_SHM_FIRST_EVENT,
            first_error: 0,
        },
        crate::X_DRI3_EXTENSION_NAME => XExtensionQueryResult {
            present: true,
            major_opcode: crate::X_DRI3_MAJOR_OPCODE,
            first_event: 0,
            first_error: 0,
        },
        crate::X_PRESENT_EXTENSION_NAME => XExtensionQueryResult {
            present: true,
            major_opcode: crate::X_PRESENT_MAJOR_OPCODE,
            first_event: crate::X_PRESENT_FIRST_EVENT,
            first_error: 0,
        },
        crate::X_XFIXES_EXTENSION_NAME => XExtensionQueryResult {
            present: true,
            major_opcode: crate::X_XFIXES_MAJOR_OPCODE,
            first_event: crate::X_XFIXES_FIRST_EVENT,
            first_error: 0,
        },
        crate::X_XC_MISC_EXTENSION_NAME => XExtensionQueryResult {
            present: true,
            major_opcode: crate::X_XC_MISC_MAJOR_OPCODE,
            first_event: 0,
            first_error: 0,
        },
        // Advertised now that the requests behind the advertised version
        // answer. Presence alone licenses a client to send CreatePicture and
        // Composite -- the base protocol carries no version gate -- so this
        // arm could not be added until they worked.
        crate::X_RENDER_EXTENSION_NAME => XExtensionQueryResult {
            present: true,
            major_opcode: crate::X_RENDER_MAJOR_OPCODE,
            first_event: 0,
            first_error: crate::X_RENDER_FIRST_ERROR,
        },
        // Advertised now that an input shape genuinely makes clicks fall
        // through. Storing one without honouring it would have been a
        // silent lie to the client that asked for this -- a panel whose
        // transparent parts still swallow clicks looks like a broken shell,
        // not a missing extension.
        crate::X_SHAPE_EXTENSION_NAME => XExtensionQueryResult {
            present: true,
            major_opcode: crate::X_SHAPE_MAJOR_OPCODE,
            first_event: crate::X_SHAPE_FIRST_EVENT,
            first_error: 0,
        },
        crate::X_XF86_VIDMODE_EXTENSION_NAME => XExtensionQueryResult {
            present: true,
            major_opcode: crate::X_XF86_VIDMODE_MAJOR_OPCODE,
            // No events and no errors of its own: the two requests answered
            // here report through the core error codes.
            first_event: 0,
            first_error: 0,
        },
        crate::X_GLX_EXTENSION_NAME => XExtensionQueryResult {
            present: true,
            major_opcode: crate::X_GLX_MAJOR_OPCODE,
            first_event: crate::X_GLX_FIRST_EVENT,
            first_error: crate::X_GLX_FIRST_ERROR,
        },
        crate::X_SYNC_EXTENSION_NAME => XExtensionQueryResult {
            present: true,
            major_opcode: crate::X_SYNC_MAJOR_OPCODE,
            first_event: crate::X_SYNC_FIRST_EVENT,
            first_error: 0,
        },
        X_RANDR_EXTENSION_NAME => XExtensionQueryResult {
            present: true,
            major_opcode: X_RANDR_MAJOR_OPCODE,
            first_event: crate::X_RANDR_FIRST_EVENT,
            first_error: 0,
        },
        crate::X_KEYBOARD_EXTENSION_NAME => XExtensionQueryResult {
            present: true,
            major_opcode: crate::X_KEYBOARD_MAJOR_OPCODE,
            first_event: crate::X_KEYBOARD_FIRST_EVENT,
            first_error: 0,
        },
        crate::X_INPUT_EXTENSION_NAME => XExtensionQueryResult {
            present: true,
            major_opcode: crate::X_INPUT_MAJOR_OPCODE,
            first_event: crate::X_INPUT_FIRST_EVENT,
            first_error: crate::X_INPUT_FIRST_ERROR,
        },
        crate::X_GENERIC_EVENT_EXTENSION_NAME => XExtensionQueryResult {
            present: true,
            major_opcode: crate::X_GENERIC_EVENT_MAJOR_OPCODE,
            first_event: 0,
            first_error: 0,
        },
        X_BIG_REQUESTS_EXTENSION_NAME => XExtensionQueryResult {
            present: true,
            major_opcode: X_BIG_REQUESTS_MAJOR_OPCODE,
            first_event: 0,
            first_error: 0,
        },
        _ => XExtensionQueryResult {
            present: false,
            major_opcode: 0,
            first_event: 0,
            first_error: 0,
        },
    }
}

struct XShmImageCopy {
    byte_order: XByteOrder,
    offset: u32,
    total_width: u16,
    total_height: u16,
    src_x: u16,
    src_y: u16,
    src_width: u16,
    src_height: u16,
    depth: u8,
    format: u8,
}

/// Cuts a rectangle out of a client's shared-memory image.
///
/// The bytes arrive through `read` rather than from a SysV id, because a
/// segment can be named either way now and the arithmetic below is the same
/// for both. `read` is handed the offset and length this validated, so a
/// caller cannot be asked for a region the checks here did not approve.
fn copy_shm_image_region(
    copy: XShmImageCopy,
    read: impl FnOnce(usize, usize) -> Option<Vec<u8>>,
) -> Option<Vec<u8>> {
    let XShmImageCopy {
        byte_order,
        offset,
        total_width,
        total_height,
        src_x,
        src_y,
        src_width,
        src_height,
        depth,
        format,
    } = copy;
    const Z_PIXMAP: u8 = 2;
    const BYTES_PER_PIXEL: usize = 4;
    const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
    if format != Z_PIXMAP {
        return None;
    }
    let total_width = usize::from(total_width);
    let total_height = usize::from(total_height);
    let src_x = usize::from(src_x);
    let src_y = usize::from(src_y);
    let src_width = usize::from(src_width);
    let src_height = usize::from(src_height);
    if src_x.checked_add(src_width)? > total_width || src_y.checked_add(src_height)? > total_height
    {
        return None;
    }
    // Shared images use the setup-advertised pixel format and scanline pad,
    // including packed one-bit masks used by GTK during startup. Decode through
    // the core upload path before cropping into the canonical pixel store.
    let layout = crate::image::XImageLayout::new(
        format,
        depth,
        u16::try_from(total_width).ok()?,
        u16::try_from(total_height).ok()?,
        u32::MAX,
    )
    .ok()?;
    if layout.payload_len > MAX_IMAGE_BYTES {
        return None;
    }
    let source = read(usize::try_from(offset).ok()?, layout.payload_len)?;
    let source = crate::image::decode_upload(
        format,
        depth,
        u16::try_from(total_width).ok()?,
        u16::try_from(total_height).ok()?,
        0,
        byte_order,
        &crate::XGraphicsContextValues::default(),
        &source,
    )
    .ok()?;
    let stride = total_width.checked_mul(BYTES_PER_PIXEL)?;
    let row_len = src_width.checked_mul(BYTES_PER_PIXEL)?;
    let mut image = Vec::with_capacity(row_len.checked_mul(src_height)?);
    for row in src_y..src_y.checked_add(src_height)? {
        let start = row
            .checked_mul(stride)?
            .checked_add(src_x.checked_mul(BYTES_PER_PIXEL)?)?;
        image.extend_from_slice(source.get(start..start.checked_add(row_len)?)?);
    }
    Some(image)
}

pub fn dispatch_x11_parse_error(
    context: XDispatchContext,
    minor_code: u16,
    error: XWireParseError,
) -> XDispatchResult {
    XDispatchResult {
        response: None,
        outputs: vec![XClientOutput::Error(x_error_from_wire_parse(
            &error,
            context.sequence,
            context.major_opcode,
            minor_code,
        ))],
        metadata_candidates: Vec::new(),
    }
}

fn outputs_from_authority_response(
    context: XDispatchContext,
    kind: &XAuthorityRequestKind,
    response: &XAuthorityResponsePacket,
) -> Vec<XClientOutput> {
    if let Some(crate::XAuthoritySelectionArtifact::Clear {
        owner,
        selection,
        time,
    }) = response.selection_artifacts.first()
    {
        return vec![XClientOutput::Event(XClientEvent::SelectionClear {
            sequence: context.sequence,
            time: *time,
            owner: *owner,
            selection: *selection,
        })];
    }
    if let XAuthorityRequestKind::RequestSelection {
        requestor,
        selection,
        target,
        time,
        ..
    } = kind
        && let Some(artifact) = response.selection_artifacts.first()
    {
        return vec![XClientOutput::Event(match artifact {
            crate::XAuthoritySelectionArtifact::Failure(_) => {
                x_selection_failure_event(context.sequence, *time, *requestor, *selection, *target)
            }
            crate::XAuthoritySelectionArtifact::Request(request) => {
                XClientEvent::SelectionRequest {
                    sequence: context.sequence,
                    time: request.time,
                    owner: request.owner,
                    requestor: request.requestor,
                    selection: request.selection,
                    target: request.target,
                    property: request.property,
                }
            }
            crate::XAuthoritySelectionArtifact::Clear {
                owner,
                selection,
                time,
            } => XClientEvent::SelectionClear {
                sequence: context.sequence,
                time: *time,
                owner: *owner,
                selection: *selection,
            },
        })];
    }

    if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
        return vec![XClientOutput::Error(x_error_from_runtime(
            error,
            context.sequence,
            context.major_opcode,
            0,
            resource_from_kind(kind),
        ))];
    }

    match kind {
        // XLibre dix/window.c::CreateWindow sends CreateNotify only to
        // SubstructureNotify selectors on the parent. It never fabricates a
        // ConfigureNotify for the newly-created window. Socket-level parent
        // fanout owns CreateNotify because this pure dispatch boundary has no
        // subscriber table.
        XAuthorityRequestKind::CreateWindow { .. } => Vec::new(),
        XAuthorityRequestKind::MapWindow { window, .. } => {
            let override_redirect = response.surfaces.first().is_some_and(|surface| {
                surface.presentation == sophia_protocol::SurfacePresentationRole::ClientPositioned
            });
            if response
                .surfaces
                .first()
                .is_some_and(|surface| !surface.mapped)
            {
                return Vec::new();
            }
            let mut outputs = vec![XClientOutput::Event(XClientEvent::MapNotify {
                sequence: context.sequence,
                event: *window,
                window: *window,
                override_redirect,
            })];
            outputs.push(XClientOutput::Event(XClientEvent::VisibilityNotify {
                sequence: context.sequence,
                window: *window,
                state: 0,
            }));
            if let Some(surface) = response.surfaces.iter().find(|surface| surface.mapped) {
                outputs.push(XClientOutput::Event(XClientEvent::Expose {
                    sequence: context.sequence,
                    window: *window,
                    x: 0,
                    y: 0,
                    width: clamp_u16(surface.geometry.width),
                    height: clamp_u16(surface.geometry.height),
                    count: 0,
                }));
            }
            outputs
        }
        XAuthorityRequestKind::RequestSelection { .. } => Vec::new(),
        XAuthorityRequestKind::SetSelectionOwner { .. }
        | XAuthorityRequestKind::PresentPixmap { .. } => Vec::new(),
    }
}

fn resource_from_kind(kind: &XAuthorityRequestKind) -> u32 {
    let resource = match kind {
        XAuthorityRequestKind::CreateWindow { window, .. }
        | XAuthorityRequestKind::MapWindow { window, .. }
        | XAuthorityRequestKind::PresentPixmap { window, .. } => *window,
        XAuthorityRequestKind::SetSelectionOwner { owner, .. } => {
            owner.unwrap_or(XResourceId::NONE)
        }
        XAuthorityRequestKind::RequestSelection { requestor, .. } => *requestor,
    };
    u32::try_from(resource.local.raw()).unwrap_or(0)
}

fn atom_type_is_unknown(atoms: &XAtomTable, atom: u32) -> bool {
    atom != crate::X_PROPERTY_ANY_TYPE && atoms.name(atom).is_none()
}

fn x_client_outputs_from_property_read(
    context: &XDispatchContext,
    window: XResourceId,
    property: u32,
    result: Result<crate::XPropertyReadOutcome, XPropertyError>,
) -> Vec<XClientOutput> {
    match result {
        Ok(outcome) => {
            let mut outputs = vec![XClientOutput::Reply(XClientReply::GetProperty {
                sequence: context.sequence,
                property_type: outcome.reply.property_type,
                format: outcome.reply.format,
                bytes_after: outcome.reply.bytes_after,
                item_count: outcome.reply.item_count,
                bytes: outcome.reply.bytes,
            })];
            if outcome.deleted {
                outputs.push(XClientOutput::Event(XClientEvent::PropertyNotify {
                    sequence: context.sequence,
                    window,
                    atom: property,
                    time: 0,
                    new_value: false,
                }));
            }
            outputs
        }
        Err(error) => vec![XClientOutput::Error(crate::XClientError {
            code: x_error_from_property_read(error),
            sequence: context.sequence,
            resource_id: 0,
            minor_code: 0,
            major_code: context.major_opcode,
        })],
    }
}

fn randr_output_property_from_read(
    context: &XDispatchContext,
    output: u32,
    result: Result<crate::XPropertyReadReply, XPropertyError>,
) -> XClientOutput {
    match result {
        Ok(reply) => XClientOutput::Reply(XClientReply::RandrGetOutputProperty {
            sequence: context.sequence,
            property_type: reply.property_type,
            bytes_after: reply.bytes_after,
            format: reply.format,
            data: reply.bytes,
        }),
        Err(error) => XClientOutput::Error(crate::XClientError {
            code: x_error_from_property_read(error),
            sequence: context.sequence,
            resource_id: output,
            minor_code: crate::X_RANDR_GET_OUTPUT_PROPERTY_MINOR_OPCODE.into(),
            major_code: context.major_opcode,
        }),
    }
}

fn x_error_from_property_read(error: XPropertyError) -> XErrorCode {
    match error {
        XPropertyError::InvalidNamespace | XPropertyError::InvalidWindow => XErrorCode::BadWindow,
        XPropertyError::InvalidFormat(_)
        | XPropertyError::ValueTooLarge { .. }
        | XPropertyError::TableTooLarge { .. }
        | XPropertyError::TypeMismatch
        | XPropertyError::InvalidOffset => XErrorCode::BadValue,
        XPropertyError::AuthorityOwned => XErrorCode::BadAccess,
    }
}

pub(crate) fn clamp_i16(value: i32) -> i16 {
    value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

pub(crate) fn clamp_u16(value: i32) -> u16 {
    value.clamp(0, i32::from(u16::MAX)) as u16
}
