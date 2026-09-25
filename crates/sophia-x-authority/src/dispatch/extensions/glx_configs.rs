// The GLX extension strings, visual and framebuffer configurations the
// authority advertises, and the BadValue a GLX request answers with.
// Included by dispatch.rs; one module with it.

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
