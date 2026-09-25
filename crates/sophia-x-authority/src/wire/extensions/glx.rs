fn decode_glx(context: XWireClientContext, bytes: &[u8]) -> Result<XWireRequest, XWireParseError> {
    let id = |offset: usize| {
        XResourceId::new(
            u64::from(context.byte_order.u32(&bytes[offset..offset + 4])),
            1,
        )
    };
    match bytes[1] {
        X_GLX_QUERY_VERSION_MINOR_OPCODE => {
            require_exact_len(X_GLX_MAJOR_OPCODE, 12, bytes.len())?;
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxQueryVersion {
                major_version: context.byte_order.u32(&bytes[4..8]),
                minor_version: context.byte_order.u32(&bytes[8..12]),
            }))
        }
        X_GLX_GET_VISUAL_CONFIGS_MINOR_OPCODE => {
            require_exact_len(X_GLX_MAJOR_OPCODE, 8, bytes.len())?;
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxGetVisualConfigs {
                screen: context.byte_order.u32(&bytes[4..8]),
            }))
        }
        X_GLX_QUERY_EXTENSIONS_STRING_MINOR_OPCODE => {
            require_exact_len(X_GLX_MAJOR_OPCODE, 8, bytes.len())?;
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxQueryExtensionsString))
        }
        X_GLX_QUERY_SERVER_STRING_MINOR_OPCODE => {
            require_exact_len(X_GLX_MAJOR_OPCODE, 12, bytes.len())?;
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxQueryServerString {
                name: context.byte_order.u32(&bytes[8..12]),
            }))
        }
        X_GLX_GET_FB_CONFIGS_MINOR_OPCODE => {
            require_exact_len(X_GLX_MAJOR_OPCODE, 8, bytes.len())?;
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxGetFbConfigs {
                screen: context.byte_order.u32(&bytes[4..8]),
            }))
        }
        X_GLX_CLIENT_INFO_MINOR_OPCODE
        | X_GLX_SET_CLIENT_INFO_ARB_MINOR_OPCODE
        | X_GLX_SET_CLIENT_INFO_2_ARB_MINOR_OPCODE => {
            require_len(X_GLX_MAJOR_OPCODE, 16, bytes.len())?;
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxClientInfo))
        }
        X_GLX_CREATE_CONTEXT_MINOR_OPCODE => {
            require_exact_len(X_GLX_MAJOR_OPCODE, 24, bytes.len())?;
            let context_id = context.byte_order.u32(&bytes[4..8]);
            context.validate_new_resource_id(context_id)?;
            let share = context.byte_order.u32(&bytes[16..20]);
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxCreateContext {
                context: XResourceId::new(u64::from(context_id), 1),
                config: XGlxContextConfig::Visual(context.byte_order.u32(&bytes[8..12])),
                screen: context.byte_order.u32(&bytes[12..16]),
                share: (share != 0).then(|| XResourceId::new(u64::from(share), 1)),
                direct: bytes[20] != 0,
            }))
        }
        X_GLX_DESTROY_CONTEXT_MINOR_OPCODE => {
            require_exact_len(X_GLX_MAJOR_OPCODE, 8, bytes.len())?;
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxDestroyContext { context: id(4) }))
        }
        X_GLX_MAKE_CURRENT_MINOR_OPCODE => {
            require_exact_len(X_GLX_MAJOR_OPCODE, 16, bytes.len())?;
            let drawable = context.byte_order.u32(&bytes[4..8]);
            let context_id = context.byte_order.u32(&bytes[8..12]);
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxMakeCurrent {
                drawable: (drawable != 0)
                    .then(|| XResourceId::new(u64::from(drawable), 1)),
                context: (context_id != 0)
                    .then(|| XResourceId::new(u64::from(context_id), 1)),
                old_context_tag: context.byte_order.u32(&bytes[12..16]),
            }))
        }
        X_GLX_IS_DIRECT_MINOR_OPCODE => {
            require_exact_len(X_GLX_MAJOR_OPCODE, 8, bytes.len())?;
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxIsDirect { context: id(4) }))
        }
        X_GLX_CREATE_NEW_CONTEXT_MINOR_OPCODE => {
            require_exact_len(X_GLX_MAJOR_OPCODE, 28, bytes.len())?;
            let context_id = context.byte_order.u32(&bytes[4..8]);
            context.validate_new_resource_id(context_id)?;
            let share = context.byte_order.u32(&bytes[20..24]);
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxCreateContext {
                context: XResourceId::new(u64::from(context_id), 1),
                config: XGlxContextConfig::FbConfig(context.byte_order.u32(&bytes[8..12])),
                screen: context.byte_order.u32(&bytes[12..16]),
                share: (share != 0).then(|| XResourceId::new(u64::from(share), 1)),
                direct: bytes[24] != 0,
            }))
        }
        X_GLX_CREATE_CONTEXT_ATTRIBS_ARB_MINOR_OPCODE => {
            require_len(X_GLX_MAJOR_OPCODE, 28, bytes.len())?;
            let count = context.byte_order.u32(&bytes[24..28]) as usize;
            require_exact_len(
                X_GLX_MAJOR_OPCODE,
                28usize.saturating_add(count.saturating_mul(8)),
                bytes.len(),
            )?;
            let context_id = context.byte_order.u32(&bytes[4..8]);
            context.validate_new_resource_id(context_id)?;
            let share = context.byte_order.u32(&bytes[16..20]);
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxCreateContext {
                context: XResourceId::new(u64::from(context_id), 1),
                config: XGlxContextConfig::FbConfig(context.byte_order.u32(&bytes[8..12])),
                screen: context.byte_order.u32(&bytes[12..16]),
                share: (share != 0).then(|| XResourceId::new(u64::from(share), 1)),
                direct: bytes[20] != 0,
            }))
        }
        X_GLX_CREATE_WINDOW_MINOR_OPCODE => {
            require_len(X_GLX_MAJOR_OPCODE, 24, bytes.len())?;
            let count = context.byte_order.u32(&bytes[20..24]) as usize;
            require_exact_len(
                X_GLX_MAJOR_OPCODE,
                24usize.saturating_add(count.saturating_mul(8)),
                bytes.len(),
            )?;
            let glx = context.byte_order.u32(&bytes[16..20]);
            context.validate_new_resource_id(glx)?;
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxCreateWindow {
                screen: context.byte_order.u32(&bytes[4..8]),
                fbconfig: context.byte_order.u32(&bytes[8..12]),
                window: id(12),
                glx_window: XResourceId::new(u64::from(glx), 1),
            }))
        }
        X_GLX_CREATE_PBUFFER_MINOR_OPCODE => {
            require_len(X_GLX_MAJOR_OPCODE, 20, bytes.len())?;
            // The count is pairs, not words -- the same shape `CreateWindow`
            // carries, and the same trap.
            let count = context.byte_order.u32(&bytes[16..20]) as usize;
            require_exact_len(
                X_GLX_MAJOR_OPCODE,
                20usize.saturating_add(count.saturating_mul(8)),
                bytes.len(),
            )?;
            let pbuffer = context.byte_order.u32(&bytes[12..16]);
            context.validate_new_resource_id(pbuffer)?;
            // Reduce the attribute list here so the request stays a passive
            // record. A name Sophia does not implement is ignored rather than
            // refused: a client may legitimately ask for more than it needs.
            let (mut width, mut height, mut largest) = (0, 0, false);
            for pair in bytes[20..].chunks_exact(8) {
                let name = context.byte_order.u32(&pair[0..4]);
                let value = context.byte_order.u32(&pair[4..8]);
                match name {
                    X_GLX_PBUFFER_WIDTH_ATTRIBUTE => width = value,
                    X_GLX_PBUFFER_HEIGHT_ATTRIBUTE => height = value,
                    X_GLX_LARGEST_PBUFFER_ATTRIBUTE => largest = value != 0,
                    _ => {}
                }
            }
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxCreatePbuffer {
                screen: context.byte_order.u32(&bytes[4..8]),
                fbconfig: context.byte_order.u32(&bytes[8..12]),
                pbuffer: XResourceId::new(u64::from(pbuffer), 1),
                width,
                height,
                largest,
            }))
        }
        X_GLX_CREATE_PIXMAP_MINOR_OPCODE => {
            require_len(X_GLX_MAJOR_OPCODE, 24, bytes.len())?;
            // Pairs, not words, as everywhere else in this extension.
            let count = context.byte_order.u32(&bytes[20..24]) as usize;
            require_exact_len(
                X_GLX_MAJOR_OPCODE,
                24usize.saturating_add(count.saturating_mul(8)),
                bytes.len(),
            )?;
            let glx_pixmap = context.byte_order.u32(&bytes[16..20]);
            context.validate_new_resource_id(glx_pixmap)?;
            // Reduce the attribute list here so the request stays a passive
            // record, as CreatePbuffer does. Only the texture target changes
            // what the server must decide; the rest belong to the binding.
            let (mut target, mut format, mut mipmap) = (None, None, None);
            for pair in bytes[24..].chunks_exact(8) {
                let value = context.byte_order.u32(&pair[4..8]);
                match context.byte_order.u32(&pair[0..4]) {
                    X_GLX_TEXTURE_TARGET_ATTRIBUTE => target = Some(value),
                    X_GLX_TEXTURE_FORMAT_ATTRIBUTE => format = Some(value),
                    X_GLX_MIPMAP_TEXTURE_ATTRIBUTE => mipmap = Some(value != 0),
                    _ => {}
                }
            }
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxCreatePixmap {
                screen: context.byte_order.u32(&bytes[4..8]),
                fbconfig: context.byte_order.u32(&bytes[8..12]),
                pixmap: id(12),
                glx_pixmap: XResourceId::new(u64::from(glx_pixmap), 1),
                target,
                format,
                mipmap,
            }))
        }
        X_GLX_CREATE_GLX_PIXMAP_MINOR_OPCODE => {
            require_exact_len(X_GLX_MAJOR_OPCODE, 20, bytes.len())?;
            let glx_pixmap = context.byte_order.u32(&bytes[16..20]);
            context.validate_new_resource_id(glx_pixmap)?;
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxCreateGlxPixmap {
                screen: context.byte_order.u32(&bytes[4..8]),
                visual: context.byte_order.u32(&bytes[8..12]),
                pixmap: id(12),
                glx_pixmap: XResourceId::new(u64::from(glx_pixmap), 1),
            }))
        }
        minor @ (X_GLX_DESTROY_PIXMAP_MINOR_OPCODE | X_GLX_DESTROY_GLX_PIXMAP_MINOR_OPCODE) => {
            require_exact_len(X_GLX_MAJOR_OPCODE, 8, bytes.len())?;
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxDestroyPixmap {
                minor_opcode: minor,
                glx_pixmap: id(4),
            }))
        }
        X_GLX_DESTROY_PBUFFER_MINOR_OPCODE => {
            require_exact_len(X_GLX_MAJOR_OPCODE, 8, bytes.len())?;
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxDestroyPbuffer { pbuffer: id(4) }))
        }
        X_GLX_QUERY_CONTEXT_MINOR_OPCODE => {
            require_exact_len(X_GLX_MAJOR_OPCODE, 8, bytes.len())?;
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxQueryContext { context: id(4) }))
        }
        X_GLX_CHANGE_DRAWABLE_ATTRIBUTES_MINOR_OPCODE => {
            require_len(X_GLX_MAJOR_OPCODE, 12, bytes.len())?;
            // The count is pairs, as everywhere else in this extension.
            let count = context.byte_order.u32(&bytes[8..12]) as usize;
            require_exact_len(
                X_GLX_MAJOR_OPCODE,
                12usize.saturating_add(count.saturating_mul(8)),
                bytes.len(),
            )?;
            // The only attribute this sets is the event mask, and Sophia sends no
            // GLX events, so the values are validated and discarded.
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxChangeDrawableAttributes { drawable: id(4) }))
        }
        X_GLX_MAKE_CONTEXT_CURRENT_MINOR_OPCODE => {
            require_exact_len(X_GLX_MAJOR_OPCODE, 20, bytes.len())?;
            let context_id = context.byte_order.u32(&bytes[16..20]);
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxMakeContextCurrent {
                drawable: id(8),
                read_drawable: id(12),
                context: (context_id != 0)
                    .then(|| XResourceId::new(u64::from(context_id), 1)),
            }))
        }
        X_GLX_DELETE_WINDOW_MINOR_OPCODE => {
            require_exact_len(X_GLX_MAJOR_OPCODE, 8, bytes.len())?;
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxDeleteWindow { glx_window: id(4) }))
        }
        X_GLX_GET_DRAWABLE_ATTRIBUTES_MINOR_OPCODE => {
            require_exact_len(X_GLX_MAJOR_OPCODE, 8, bytes.len())?;
            Ok(XWireRequest::Glx(crate::XGlxRequest::GlxGetDrawableAttributes { drawable: id(4) }))
        }
        minor_opcode => Ok(XWireRequest::Glx(crate::XGlxRequest::GlxUnimplemented { minor_opcode })),
    }
}
