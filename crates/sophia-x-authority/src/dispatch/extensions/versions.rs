fn dispatch_extension_version_request(
    context: XDispatchContext,
    request: XWireRequest,
    _runtime: &mut XAuthorityRuntime,
    _atoms: &mut XAtomTable,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
            XWireRequest::Shm(crate::XShmRequest::ShmQueryVersion)
            | XWireRequest::Dri3(crate::XDri3Request::Dri3QueryVersion { .. })
            | XWireRequest::Xfixes(crate::XFixesRequest::XfixesQueryVersion { .. })
    ) {
        return Unhandled(request);
    }
    Handled(match request {
                XWireRequest::Shm(crate::XShmRequest::ShmQueryVersion) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::ShmQueryVersion {
                        sequence: context.sequence,
                        major_version: 1,
                        minor_version: 2,
                        shared_pixmaps: false,
                        pixmap_format: 0,
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::Dri3(crate::XDri3Request::Dri3QueryVersion { major_version, minor_version }) => {
                    let (major_version, minor_version) = (major_version, minor_version).min((1, 3));
                    XDispatchResult {
                        response: None,
                        outputs: vec![XClientOutput::Reply(XClientReply::Dri3QueryVersion {
                            sequence: context.sequence,
                            major_version,
                            minor_version,
                        })],
                        metadata_candidates: Vec::new(),
                    }
                },
                XWireRequest::Xfixes(crate::XFixesRequest::XfixesQueryVersion {
                    major_version: major,
                    minor_version: minor,
                }) => {
                    // The protocol asks for the lower of the two versions, and
                    // this answered its own regardless. A client that asked for
                    // version 1 was told 6 and would then be entitled to send
                    // requests no version 1 client should know about.
                    // Versions order lexicographically, so the lower of the
                    // two is the smaller pair -- which keeps saying the right
                    // thing if either constant moves.
                    let (major_version, minor_version) = (major, minor).min((
                        crate::X_XFIXES_MAJOR_VERSION,
                        crate::X_XFIXES_MINOR_VERSION,
                    ));
                    XDispatchResult {
                        response: None,
                        outputs: vec![XClientOutput::Reply(XClientReply::XfixesQueryVersion {
                            sequence: context.sequence,
                            major_version,
                            minor_version,
                        })],
                        metadata_candidates: Vec::new(),
                    }
                }
        _ => unreachable!("request family checked before dispatch"),
    })
}
