/// Decoded Randr requests; payloads retain their protocol representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XRandrRequest {
    RandrQueryVersion {
        major_version: u32,
        minor_version: u32,
    },
    RandrSelectInput {
        window: XResourceId,
        enable: u16,
    },
    RandrGetScreenSizeRange {
        window: XResourceId,
    },
    RandrGetScreenResources {
        window: XResourceId,
        current: bool,
    },
    RandrGetOutputInfo {
        output: u32,
        config_timestamp: u32,
    },
    RandrGetOutputProperty {
        output: u32,
        property: XAtom,
        property_type: XAtom,
        long_offset: u32,
        long_length: u32,
        delete: bool,
        pending: bool,
    },
    RandrGetCrtcInfo {
        crtc: u32,
        config_timestamp: u32,
    },
    RandrGetCrtcGammaSize {
        crtc: u32,
    },
    RandrGetCrtcGamma {
        crtc: u32,
    },
    RandrGetCrtcTransform {
        crtc: u32,
    },
    RandrGetPanning {
        crtc: u32,
    },
    RandrGetOutputPrimary {
        window: XResourceId,
    },
    RandrGetProviders {
        window: XResourceId,
    },
    RandrGetMonitors {
        window: XResourceId,
        get_active: bool,
    },
}
