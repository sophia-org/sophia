/// Decoded Glx requests; payloads retain their protocol representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XGlxRequest {
    /// A SHAPE minor no version of the extension defines.
    /// A GLX minor Sophia does not answer. Decoded rather than refused at the
    /// parser, so the client gets a normal error against a sequence number it
    /// can attribute, which a parse failure would deny it.
    GlxUnimplemented {
        minor_opcode: u8,
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
    GlxQueryExtensionsString,
    GlxQueryServerString {
        name: u32,
    },
}
