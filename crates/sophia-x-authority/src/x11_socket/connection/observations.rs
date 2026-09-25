fn x11_observed_request_stage(request: &crate::XWireRequest) -> X11ObservedRequestStage {
    match request {
        crate::XWireRequest::Glx(crate::XGlxRequest::GlxQueryServerString { .. }) => {
            X11ObservedRequestStage::GlxQueryServerString
        }
        crate::XWireRequest::Glx(crate::XGlxRequest::GlxGetVisualConfigs { .. }) => {
            X11ObservedRequestStage::GlxGetVisualConfigs
        }
        crate::XWireRequest::Glx(crate::XGlxRequest::GlxGetFbConfigs { .. }) => X11ObservedRequestStage::GlxGetFbConfigs,
        crate::XWireRequest::Glx(crate::XGlxRequest::GlxCreateContext { .. }) => X11ObservedRequestStage::GlxCreateContext,
        crate::XWireRequest::Shm(crate::XShmRequest::ShmCreateSegment { .. }) => X11ObservedRequestStage::ShmCreateSegment,
        crate::XWireRequest::Shm(crate::XShmRequest::ShmAttachFd { .. }) => X11ObservedRequestStage::ShmAttachFd,
        crate::XWireRequest::Glx(crate::XGlxRequest::GlxMakeCurrent { .. }) => X11ObservedRequestStage::GlxMakeCurrent,
        crate::XWireRequest::Glx(crate::XGlxRequest::GlxCreateWindow { .. }) => X11ObservedRequestStage::GlxCreateWindow,
        crate::XWireRequest::Dri3(crate::XDri3Request::Dri3PixmapFromBuffers { .. }) => {
            X11ObservedRequestStage::Dri3PixmapFromBuffers
        }
        crate::XWireRequest::Present(crate::XPresentRequest::PresentPixmap { .. })
        | crate::XWireRequest::Authority(crate::XAuthorityRequestPacket {
            kind: crate::XAuthorityRequestKind::PresentPixmap { .. },
            ..
        }) => X11ObservedRequestStage::PresentPixmap,
        crate::XWireRequest::Render(crate::XRenderRequest::RenderQueryPictFormats) => {
            X11ObservedRequestStage::RenderQueryPictFormats
        }
        crate::XWireRequest::Render(crate::XRenderRequest::RenderComposite { .. }) => X11ObservedRequestStage::RenderComposite,
        crate::XWireRequest::Render(crate::XRenderRequest::RenderCompositeGlyphs { .. }) => {
            X11ObservedRequestStage::RenderCompositeGlyphs
        }
        crate::XWireRequest::Core(crate::XCoreRequest::GetKeyboardMapping { .. }) => X11ObservedRequestStage::KeyboardMapping,
        crate::XWireRequest::Authority(crate::XAuthorityRequestPacket {
            kind: crate::XAuthorityRequestKind::RequestSelection { .. },
            ..
        }) => X11ObservedRequestStage::SelectionRequest,
        _ => X11ObservedRequestStage::Other,
    }
}
