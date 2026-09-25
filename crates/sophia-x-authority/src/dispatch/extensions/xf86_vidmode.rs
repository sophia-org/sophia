fn dispatch_xf86_vidmode_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    _atoms: &mut XAtomTable,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
        XWireRequest::Extension(crate::XExtensionRequest::XF86VidModeQueryVersion)
            | XWireRequest::Extension(crate::XExtensionRequest::XF86VidModeGetModeLine { .. })
            | XWireRequest::Extension(crate::XExtensionRequest::XF86VidModeSetClientVersion { .. })
            | XWireRequest::Extension(crate::XExtensionRequest::XF86VidModeUnimplemented { .. })
    ) {
        return Unhandled(request);
    }
    Handled(match request {
        XWireRequest::Extension(crate::XExtensionRequest::XF86VidModeQueryVersion) => XDispatchResult {
            response: None,
            outputs: vec![XClientOutput::Reply(XClientReply::XF86VidModeQueryVersion {
                sequence: context.sequence,
                major_version: crate::X_XF86_VIDMODE_MAJOR_VERSION,
                minor_version: crate::X_XF86_VIDMODE_MINOR_VERSION,
            })],
            metadata_candidates: Vec::new(),
        },
        // Recorded by answering nothing, which is what the request expects.
        // The library sends it after seeing a major version of two or more,
        // and a refusal here would end the exchange one request after
        // `QueryVersion` had just succeeded.
        XWireRequest::Extension(crate::XExtensionRequest::XF86VidModeSetClientVersion { .. }) => XDispatchResult {
            response: None,
            outputs: Vec::new(),
            metadata_candidates: Vec::new(),
        },
        XWireRequest::Extension(crate::XExtensionRequest::XF86VidModeGetModeLine { screen }) => {
            let topology = runtime.output_topology();
            // One X screen, whatever the display count, so any other screen
            // number names something that does not exist.
            let timing = if screen == 0 {
                topology
                    .outputs
                    .iter()
                    .find(|entry| entry.output == topology.primary)
                    .and_then(|entry| entry.timing)
                    .filter(|timing| timing.is_valid())
            } else {
                None
            };
            let outputs = match timing {
                Some(timing) => vec![XClientOutput::Reply(
                    XClientReply::XF86VidModeGetModeLine {
                        sequence: context.sequence,
                        timing,
                    },
                )],
                // Refused rather than answered with a modeline nobody
                // measured. A client that receives invented timings computes a
                // refresh rate from them and believes it; one that receives an
                // error falls back to its own default and knows it did.
                None => vec![XClientOutput::Error(crate::XClientError {
                    code: XErrorCode::BadValue,
                    sequence: context.sequence,
                    resource_id: u32::from(screen),
                    minor_code: u16::from(crate::X_XF86_VIDMODE_GET_MODE_LINE_MINOR_OPCODE),
                    major_code: context.major_opcode,
                })],
            };
            XDispatchResult {
                response: None,
                outputs,
                metadata_candidates: Vec::new(),
            }
        }
        // Mode switching, gamma and viewport control all live in this
        // extension, and Sophia owns modesetting. Refusing by name says which
        // request was declined rather than leaving a client to conclude the
        // extension is broken.
        XWireRequest::Extension(crate::XExtensionRequest::XF86VidModeUnimplemented { minor_opcode }) => XDispatchResult {
            response: None,
            outputs: vec![XClientOutput::Error(crate::XClientError {
                code: if minor_opcode <= crate::X_XF86_VIDMODE_LAST_MINOR_OPCODE {
                    XErrorCode::BadImplementation
                } else {
                    XErrorCode::BadRequest
                },
                sequence: context.sequence,
                resource_id: 0,
                minor_code: u16::from(minor_opcode),
                major_code: context.major_opcode,
            })],
            metadata_candidates: Vec::new(),
        },
        other => return Unhandled(other),
    })
}
