// Decoded request families. This representation does not change X11 bytes.
include!("requests/core.rs");
include!("requests/shm.rs");
include!("requests/dri3.rs");
include!("requests/xfixes.rs");
include!("requests/present.rs");
include!("requests/extension.rs");
include!("requests/render.rs");
include!("requests/shape.rs");
include!("requests/glx.rs");
include!("requests/xtest.rs");
include!("requests/randr.rs");
include!("requests/xkb.rs");
include!("requests/sync.rs");
include!("requests/xi.rs");

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XWireRequest {
    Authority(XAuthorityRequestPacket),
    Core(XCoreRequest),
    Shm(XShmRequest),
    Dri3(XDri3Request),
    Xfixes(XFixesRequest),
    Present(XPresentRequest),
    Extension(XExtensionRequest),
    Render(XRenderRequest),
    Shape(XShapeRequest),
    Glx(XGlxRequest),
    XTest(XTestRequest),
    Randr(XRandrRequest),
    Xkb(XkbRequest),
    Sync(XSyncRequest),
    Xi(XInputRequest),
}
