fn dispatch_randr_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
            XWireRequest::Randr(crate::XRandrRequest::RandrQueryVersion { .. })
            | XWireRequest::Randr(crate::XRandrRequest::RandrSelectInput { .. })
            | XWireRequest::Randr(crate::XRandrRequest::RandrGetScreenSizeRange { .. })
            | XWireRequest::Randr(crate::XRandrRequest::RandrGetScreenResources { .. })
            | XWireRequest::Randr(crate::XRandrRequest::RandrGetOutputInfo { .. })
            | XWireRequest::Randr(crate::XRandrRequest::RandrGetOutputProperty { .. })
            | XWireRequest::Randr(crate::XRandrRequest::RandrGetCrtcInfo { .. })
            | XWireRequest::Randr(crate::XRandrRequest::RandrGetCrtcGammaSize { .. })
            | XWireRequest::Randr(crate::XRandrRequest::RandrGetCrtcGamma { .. })
            | XWireRequest::Randr(crate::XRandrRequest::RandrGetCrtcTransform { .. })
            | XWireRequest::Randr(crate::XRandrRequest::RandrGetPanning { .. })
            | XWireRequest::Randr(crate::XRandrRequest::RandrGetOutputPrimary { .. })
            | XWireRequest::Randr(crate::XRandrRequest::RandrGetProviders { .. })
            | XWireRequest::Randr(crate::XRandrRequest::RandrGetMonitors { .. })
    ) {
        return Unhandled(request);
    }
    Handled(match request {
                XWireRequest::Randr(crate::XRandrRequest::RandrQueryVersion { .. }) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::RandrQueryVersion {
                        sequence: context.sequence,
                        major_version: 1,
                        minor_version: 5,
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::Randr(crate::XRandrRequest::RandrSelectInput { window, .. }) => {
                    let outputs = if window.local.raw() == u64::from(X_SETUP_DEFAULT_ROOT) {
                        Vec::new()
                    } else if let Err(error) = runtime.validate_window_access(context.namespace, window) {
                        vec![XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            u16::from(crate::X_RANDR_SELECT_INPUT_MINOR_OPCODE),
                            u32::try_from(window.local.raw()).unwrap_or(0)))]
                    } else {
                        Vec::new()
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Randr(crate::XRandrRequest::RandrGetScreenSizeRange { window }) => {
                    let root_size = runtime
                        .output_topology()
                        .root_size()
                        .expect("validated output topology");
                    let root_width = u16::try_from(root_size.width).expect("validated output width");
                    let root_height = u16::try_from(root_size.height).expect("validated output height");
                    let output = if window.local.raw() == u64::from(X_SETUP_DEFAULT_ROOT) {
                        XClientOutput::Reply(XClientReply::RandrGetScreenSizeRange {
                            sequence: context.sequence,
                            min_width: root_width,
                            min_height: root_height,
                            max_width: root_width,
                            max_height: root_height,
                        })
                    } else if let Err(error) = runtime.validate_window_access(context.namespace, window) {
                        XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            u16::from(crate::X_RANDR_GET_SCREEN_SIZE_RANGE_MINOR_OPCODE),
                            u32::try_from(window.local.raw()).unwrap_or(0)))
                    } else {
                        XClientOutput::Reply(XClientReply::RandrGetScreenSizeRange {
                            sequence: context.sequence,
                            min_width: root_width,
                            min_height: root_height,
                            max_width: root_width,
                            max_height: root_height,
                        })
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Randr(crate::XRandrRequest::RandrGetScreenResources { window, .. }) => {
                    let resources = randr_resources(runtime.output_topology());
                    let output = if window.local.raw() == u64::from(X_SETUP_DEFAULT_ROOT) {
                        XClientOutput::Reply(XClientReply::RandrGetScreenResources {
                            sequence: context.sequence,
                            timestamp: resources.timestamp,
                            crtcs: resources.crtcs.clone(),
                            outputs: resources.outputs.clone(),
                            modes: resources.modes.clone(),
                        })
                    } else if let Err(error) = runtime.validate_window_access(context.namespace, window) {
                        XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            u16::from(crate::X_RANDR_GET_SCREEN_RESOURCES_MINOR_OPCODE),
                            u32::try_from(window.local.raw()).unwrap_or(0)))
                    } else {
                        XClientOutput::Reply(XClientReply::RandrGetScreenResources {
                            sequence: context.sequence,
                            timestamp: resources.timestamp,
                            crtcs: resources.crtcs,
                            outputs: resources.outputs,
                            modes: resources.modes,
                        })
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Randr(crate::XRandrRequest::RandrGetOutputInfo { output, .. }) => {
                    let resources = randr_resources(runtime.output_topology());
                    let client_output = resources
                        .outputs
                        .iter()
                        .position(|candidate| *candidate == output)
                        .map(|index| {
                            let entry = &runtime.output_topology().outputs[index];
                            let mode = resources.modes[index].id;
                            XClientOutput::Reply(XClientReply::RandrGetOutputInfo {
                                sequence: context.sequence,
                                timestamp: resources.timestamp,
                                crtc: resources.crtcs[index],
                                mm_width: logical_pixels_to_millimeters(entry.logical.width),
                                mm_height: logical_pixels_to_millimeters(entry.logical.height),
                                crtcs: vec![resources.crtcs[index]],
                                modes: vec![mode],
                                name: format!("SOPHIA-{}", entry.output.raw()).into_bytes(),
                            })
                        })
                        .unwrap_or_else(|| {
                            XClientOutput::Error(crate::XClientError {
                                code: XErrorCode::BadValue,
                                sequence: context.sequence,
                                resource_id: output,
                                minor_code: crate::X_RANDR_GET_OUTPUT_INFO_MINOR_OPCODE.into(),
                                major_code: context.major_opcode,
                            })
                        });
                    XDispatchResult {
                        response: None,
                        outputs: vec![client_output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Randr(crate::XRandrRequest::RandrGetOutputProperty {
                    output,
                    property,
                    property_type,
                    long_offset,
                    long_length,
                    delete: _,
                    pending: _,
                }) => {
                    let resources = randr_resources(runtime.output_topology());
                    let client_output = if !resources.outputs.contains(&output) {
                        XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadValue,
                            sequence: context.sequence,
                            resource_id: output,
                            minor_code: crate::X_RANDR_GET_OUTPUT_PROPERTY_MINOR_OPCODE.into(),
                            major_code: context.major_opcode,
                        })
                    } else if atoms.name(property).is_none() {
                        XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadAtom,
                            sequence: context.sequence,
                            resource_id: property,
                            minor_code: crate::X_RANDR_GET_OUTPUT_PROPERTY_MINOR_OPCODE.into(),
                            major_code: context.major_opcode,
                        })
                    } else if atoms.name(property)
                        == Some(crate::X_ATOM_NAME_RANDR_NON_DESKTOP)
                    {
                        let mut value = Vec::with_capacity(4);
                        context.byte_order.push_u32(&mut value, 0);
                        randr_output_property_from_read(
                            &context,
                            output,
                            crate::read_property_value(
                                crate::X_ATOM_CARDINAL,
                                32,
                                &value,
                                property_type,
                                long_offset,
                                long_length,
                            ),
                        )
                    } else {
                        XClientOutput::Reply(XClientReply::RandrGetOutputProperty {
                            sequence: context.sequence,
                            property_type: 0,
                            bytes_after: 0,
                            format: 0,
                            data: Vec::new(),
                        })
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![client_output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Randr(crate::XRandrRequest::RandrGetCrtcInfo { crtc, .. }) => {
                    let resources = randr_resources(runtime.output_topology());
                    let client_output = resources
                        .crtcs
                        .iter()
                        .position(|candidate| *candidate == crtc)
                        .map(|index| {
                            let entry = &runtime.output_topology().outputs[index];
                            XClientOutput::Reply(XClientReply::RandrGetCrtcInfo {
                                sequence: context.sequence,
                                timestamp: resources.timestamp,
                                x: i16::try_from(entry.logical.x).unwrap_or(i16::MAX),
                                y: i16::try_from(entry.logical.y).unwrap_or(i16::MAX),
                                width: u16::try_from(entry.logical.width).expect("validated output width"),
                                height: u16::try_from(entry.logical.height)
                                    .expect("validated output height"),
                                mode: resources.modes[index].id,
                                outputs: vec![resources.outputs[index]],
                            })
                        })
                        .unwrap_or_else(|| {
                            XClientOutput::Error(crate::XClientError {
                                code: XErrorCode::BadValue,
                                sequence: context.sequence,
                                resource_id: crtc,
                                minor_code: crate::X_RANDR_GET_CRTC_INFO_MINOR_OPCODE.into(),
                                major_code: context.major_opcode,
                            })
                        });
                    XDispatchResult {
                        response: None,
                        outputs: vec![client_output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Randr(crate::XRandrRequest::RandrGetCrtcGammaSize { crtc }) => {
                    let resources = randr_resources(runtime.output_topology());
                    let client_output = if resources.crtcs.contains(&crtc) {
                        XClientOutput::Reply(XClientReply::RandrGetCrtcGammaSize {
                            sequence: context.sequence,
                            size: 0,
                        })
                    } else {
                        XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadValue,
                            sequence: context.sequence,
                            resource_id: crtc,
                            minor_code: crate::X_RANDR_GET_CRTC_GAMMA_SIZE_MINOR_OPCODE.into(),
                            major_code: context.major_opcode,
                        })
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![client_output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Randr(crate::XRandrRequest::RandrGetCrtcGamma { crtc }) => {
                    let resources = randr_resources(runtime.output_topology());
                    let client_output = if resources.crtcs.contains(&crtc) {
                        XClientOutput::Reply(XClientReply::RandrGetCrtcGamma {
                            sequence: context.sequence,
                        })
                    } else {
                        XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadValue,
                            sequence: context.sequence,
                            resource_id: crtc,
                            minor_code: crate::X_RANDR_GET_CRTC_GAMMA_MINOR_OPCODE.into(),
                            major_code: context.major_opcode,
                        })
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![client_output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Randr(crate::XRandrRequest::RandrGetCrtcTransform { crtc }) => {
                    let resources = randr_resources(runtime.output_topology());
                    let client_output = if resources.crtcs.contains(&crtc) {
                        XClientOutput::Reply(XClientReply::RandrGetCrtcTransform {
                            sequence: context.sequence,
                        })
                    } else {
                        XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadValue,
                            sequence: context.sequence,
                            resource_id: crtc,
                            minor_code: crate::X_RANDR_GET_CRTC_TRANSFORM_MINOR_OPCODE.into(),
                            major_code: context.major_opcode,
                        })
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![client_output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Randr(crate::XRandrRequest::RandrGetPanning { crtc }) => {
                    let resources = randr_resources(runtime.output_topology());
                    let client_output = if resources.crtcs.contains(&crtc) {
                        XClientOutput::Reply(XClientReply::RandrGetPanning {
                            sequence: context.sequence,
                            timestamp: resources.timestamp,
                        })
                    } else {
                        XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadValue,
                            sequence: context.sequence,
                            resource_id: crtc,
                            minor_code: crate::X_RANDR_GET_PANNING_MINOR_OPCODE.into(),
                            major_code: context.major_opcode,
                        })
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![client_output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Randr(crate::XRandrRequest::RandrGetOutputPrimary { window }) => {
                    let resources = randr_resources(runtime.output_topology());
                    let primary = runtime
                        .output_topology()
                        .outputs
                        .iter()
                        .position(|entry| entry.output == runtime.output_topology().primary)
                        .map(|index| resources.outputs[index])
                        .expect("validated primary output");
                    let output = if window.local.raw() == u64::from(X_SETUP_DEFAULT_ROOT) {
                        XClientOutput::Reply(XClientReply::RandrGetOutputPrimary {
                            sequence: context.sequence,
                            output: primary,
                        })
                    } else if let Err(error) = runtime.validate_window_access(context.namespace, window) {
                        XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            u16::from(crate::X_RANDR_GET_OUTPUT_PRIMARY_MINOR_OPCODE),
                            u32::try_from(window.local.raw()).unwrap_or(0)))
                    } else {
                        XClientOutput::Reply(XClientReply::RandrGetOutputPrimary {
                            sequence: context.sequence,
                            output: primary,
                        })
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Randr(crate::XRandrRequest::RandrGetProviders { window }) => {
                    let timestamp = u32::try_from(runtime.output_topology().generation)
                        .unwrap_or(u32::MAX)
                        .max(1);
                    let output = if window.local.raw() == u64::from(X_SETUP_DEFAULT_ROOT) {
                        XClientOutput::Reply(XClientReply::RandrGetProviders {
                            sequence: context.sequence,
                            timestamp,
                        })
                    } else if let Err(error) = runtime.validate_window_access(context.namespace, window) {
                        XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            u16::from(crate::X_RANDR_GET_PROVIDERS_MINOR_OPCODE),
                            u32::try_from(window.local.raw()).unwrap_or(0)))
                    } else {
                        XClientOutput::Reply(XClientReply::RandrGetProviders {
                            sequence: context.sequence,
                            timestamp,
                        })
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Randr(crate::XRandrRequest::RandrGetMonitors { window, .. }) => {
                    let timestamp = u32::try_from(runtime.output_topology().generation)
                        .unwrap_or(u32::MAX)
                        .max(1);
                    let monitors = randr_monitors(runtime.output_topology(), atoms);
                    let output = if window.local.raw() == u64::from(X_SETUP_DEFAULT_ROOT) {
                        XClientOutput::Reply(XClientReply::RandrGetMonitors {
                            sequence: context.sequence,
                            timestamp,
                            monitors: monitors.clone(),
                        })
                    } else if let Err(error) = runtime.validate_window_access(context.namespace, window) {
                        XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            u16::from(crate::X_RANDR_GET_MONITORS_MINOR_OPCODE),
                            u32::try_from(window.local.raw()).unwrap_or(0)))
                    } else {
                        XClientOutput::Reply(XClientReply::RandrGetMonitors {
                            sequence: context.sequence,
                            timestamp,
                            monitors,
                        })
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
        _ => unreachable!("request family checked before dispatch"),
    })
}

#[derive(Clone, Debug)]
struct XRandrResources {
    timestamp: u32,
    crtcs: Vec<u32>,
    outputs: Vec<u32>,
    modes: Vec<XRandrModeInfo>,
}

fn randr_resources(snapshot: &OutputTopologySnapshot) -> XRandrResources {
    let timestamp = u32::try_from(snapshot.generation)
        .unwrap_or(u32::MAX)
        .max(1);
    let mut crtcs = Vec::with_capacity(snapshot.outputs.len());
    let mut outputs = Vec::with_capacity(snapshot.outputs.len());
    let mut modes = Vec::with_capacity(snapshot.outputs.len());
    for entry in &snapshot.outputs {
        // Output identity is Engine-owned and survives topology reordering.
        // The protocol caps the topology at 16 entries; folding the opaque ID
        // keeps it outside client resource ranges while remaining stable.
        let identity = stable_randr_identity(entry.output.raw());
        let crtc = 0x1000_0000 | identity;
        let output = 0x2000_0000 | identity;
        let mode = stable_randr_mode_id(
            entry.logical.width,
            entry.logical.height,
            entry.refresh_millihz,
        );
        crtcs.push(crtc);
        outputs.push(output);
        modes.push(XRandrModeInfo {
            id: mode,
            width: u16::try_from(entry.logical.width).expect("validated output width"),
            height: u16::try_from(entry.logical.height).expect("validated output height"),
            refresh_millihz: entry.refresh_millihz,
            timing: entry.timing,
            name: format!(
                "{}x{}@{}",
                entry.logical.width,
                entry.logical.height,
                entry.refresh_millihz / 1_000
            )
            .into_bytes(),
        });
    }
    XRandrResources {
        timestamp,
        crtcs,
        outputs,
        modes,
    }
}

pub(crate) fn stable_randr_identity(raw: u64) -> u32 {
    let folded = raw ^ (raw >> 32);
    (u32::try_from(folded & 0x0fff_ffff).unwrap_or(0)).max(1)
}

pub(crate) fn stable_randr_mode_id(width: i32, height: i32, refresh_millihz: u32) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for value in [width as u32, height as u32, refresh_millihz] {
        hash ^= value;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    0x3000_0000 | (hash & 0x0fff_ffff).max(1)
}

fn logical_pixels_to_millimeters(pixels: i32) -> u32 {
    u32::try_from(i64::from(pixels).saturating_mul(254).saturating_add(480) / 960)
        .unwrap_or(u32::MAX)
        .max(1)
}

fn randr_monitors(
    snapshot: &OutputTopologySnapshot,
    atoms: &mut XAtomTable,
) -> Vec<XRandrMonitorInfo> {
    snapshot
        .outputs
        .iter()
        .map(|entry| {
            let name = atoms
                .intern(format!("SOPHIA-{}", entry.output.raw()), false)
                .ok()
                .flatten()
                .unwrap_or(X_ATOM_NONE);
            XRandrMonitorInfo {
                name,
                primary: entry.output == snapshot.primary,
                x: i16::try_from(entry.logical.x).unwrap_or(i16::MAX),
                y: i16::try_from(entry.logical.y).unwrap_or(i16::MAX),
                width: u16::try_from(entry.logical.width).unwrap_or(u16::MAX),
                height: u16::try_from(entry.logical.height).unwrap_or(u16::MAX),
                mm_width: logical_pixels_to_millimeters(entry.logical.width),
                mm_height: logical_pixels_to_millimeters(entry.logical.height),
                outputs: vec![0x2000_0000 | stable_randr_identity(entry.output.raw())],
            }
        })
        .collect()
}
