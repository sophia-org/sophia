// Colormaps and colour cells on a static visual: QueryColors, the colormap
// requests, and the colour allocations and lookups. Included by
// dispatch.rs beside input_discovery.rs; one module with it.

fn dispatch_colormap_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    _atoms: &mut XAtomTable,
    _properties: &mut XPropertyTable,
) -> XDispatchResult {
    match request {
                XWireRequest::QueryColors { colormap, pixels } => {
                    let output = match runtime.colormap_visual(context.namespace, colormap) {
                        Err(_) => color_error(
                            context,
                            XErrorCode::BadColor,
                            u32::try_from(colormap.local.raw()).unwrap_or(0),
                        ),
                        Ok(visual_id) => {
                            let visual = x_true_color_visual(visual_id)
                                .expect("registered colormaps must name advertised visuals");
                            if let Some(invalid) = pixels
                                .iter()
                                .copied()
                                .find(|pixel| visual.query(*pixel).is_none())
                            {
                                color_error(context, XErrorCode::BadValue, invalid)
                            } else {
                                XClientOutput::Reply(XClientReply::QueryColors {
                                    sequence: context.sequence,
                                    colors: pixels
                                        .into_iter()
                                        .map(|pixel| {
                                            visual
                                                .query(pixel)
                                                .expect("pixels were validated before encoding")
                                        })
                                        .collect(),
                                })
                            }
                        }
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::CreateColormap {
                    alloc,
                    colormap,
                    window,
                    visual,
                } => {
                    let output = if alloc > 1 {
                        Some(color_error(
                            context,
                            XErrorCode::BadValue,
                            u32::from(alloc),
                        ))
                    } else if runtime.resource_id_in_use(colormap) {
                        Some(color_error(
                            context,
                            XErrorCode::BadIdChoice,
                            u32::try_from(colormap.local.raw()).unwrap_or(0),
                        ))
                    } else if window.local.raw() != u64::from(X_SETUP_DEFAULT_ROOT)
                        && runtime
                            .validate_window_access(context.namespace, window)
                            .is_err()
                    {
                        Some(color_error(
                            context,
                            XErrorCode::BadWindow,
                            u32::try_from(window.local.raw()).unwrap_or(0),
                        ))
                    } else if x_true_color_visual(visual).is_none() || alloc != 0 {
                        Some(color_error(context, XErrorCode::BadMatch, visual))
                    } else {
                        match runtime.create_colormap(
                            context.namespace,
                            colormap,
                            visual,
                            1,
                        ) {
                            Ok(()) => None,
                            Err(XColormapError::DuplicateId) => Some(color_error(
                                context,
                                XErrorCode::BadIdChoice,
                                u32::try_from(colormap.local.raw()).unwrap_or(0),
                            )),
                            Err(XColormapError::UnknownVisual) => {
                                Some(color_error(context, XErrorCode::BadMatch, visual))
                            }
                            Err(XColormapError::Access(_)) => Some(color_error(
                                context,
                                XErrorCode::BadAccess,
                                u32::try_from(colormap.local.raw()).unwrap_or(0),
                            )),
                        }
                    };
                    XDispatchResult {
                        response: None,
                        outputs: output.into_iter().collect(),
                        metadata_candidates: Vec::new(),
                    }
                }
                // A TrueColor visual has no allocable cells and a read-only
                // colormap, so these requests are answered rather than served.
                // They are decoded at all so the answer is the protocol's own
                // error: a client meeting BadRequest may treat it as fatal,
                // and these arrive on ordinary teardown paths.
                // The copy keeps the visual and moves only this client's
                // component references; other clients keep theirs.
                XWireRequest::CopyColormapAndFree { colormap, source } => {
                    let output = match runtime.colormap_visual(context.namespace, source) {
                        Err(_) => Some(color_error(
                            context,
                            XErrorCode::BadColor,
                            u32::try_from(source.local.raw()).unwrap_or(0),
                        )),
                        Ok(visual) => match runtime.create_colormap(
                            context.namespace,
                            colormap,
                            visual,
                            1,
                        ) {
                            Ok(()) => {
                                runtime.copy_color_allocations(context.namespace, context.client_id, source, colormap);
                                None
                            },
                            Err(XColormapError::DuplicateId) => Some(color_error(
                                context,
                                XErrorCode::BadIdChoice,
                                u32::try_from(colormap.local.raw()).unwrap_or(0),
                            )),
                            Err(XColormapError::UnknownVisual) => {
                                Some(color_error(context, XErrorCode::BadMatch, visual))
                            }
                            Err(XColormapError::Access(_)) => Some(color_error(
                                context,
                                XErrorCode::BadAccess,
                                u32::try_from(colormap.local.raw()).unwrap_or(0),
                            )),
                        },
                    };
                    XDispatchResult {
                        response: None,
                        outputs: output.into_iter().collect(),
                        metadata_candidates: Vec::new(),
                    }
                }
                // The one installed colormap is the default, always: the
                // setup advertises one installed map at most and at least,
                // and GetWindowAttributes reports every window's installed.
                XWireRequest::ListInstalledColormaps { window } => {
                    let outputs = if window.local.raw() != u64::from(X_SETUP_DEFAULT_ROOT)
                        && runtime
                            .validate_window_access(context.namespace, window)
                            .is_err()
                    {
                        vec![color_error(
                            context,
                            XErrorCode::BadWindow,
                            u32::try_from(window.local.raw()).unwrap_or(0),
                        )]
                    } else {
                        vec![XClientOutput::Reply(XClientReply::ListInstalledColormaps {
                            sequence: context.sequence,
                            colormaps: vec![crate::X_SETUP_DEFAULT_COLORMAP],
                        })]
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::FreeColors { colormap, plane_mask, ref pixels } => {
                    let error = runtime.free_colors(context.namespace, context.client_id, colormap, plane_mask, pixels);
                    XDispatchResult {
                        response: None,
                        outputs: error.map(|(code, value)| color_error(context, code, value)).into_iter().collect(),
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::ColormapRequest {
                    kind,
                    colormap,
                    invalid_value,
                } => {
                    let known = runtime.colormap_visual(context.namespace, colormap).is_ok();
                    let outputs = if !known {
                        vec![color_error(
                            context,
                            XErrorCode::BadColor,
                            u32::try_from(colormap.local.raw()).unwrap_or(0),
                        )]
                    } else if let Some(value) = invalid_value {
                        vec![color_error(context, XErrorCode::BadValue, value)]
                    } else {
                        match kind {
                            crate::XColormapRequestKind::AllocCells
                            | crate::XColormapRequestKind::AllocPlanes => {
                                vec![color_error(context, XErrorCode::BadAlloc, 0)]
                            }
                            crate::XColormapRequestKind::StoreColors
                            | crate::XColormapRequestKind::StoreNamedColor => {
                                vec![color_error(context, XErrorCode::BadAccess, 0)]
                            }
                            // Install/uninstall remain the fixed-map policy.
                            crate::XColormapRequestKind::Install
                            | crate::XColormapRequestKind::Uninstall => Vec::new(),
                        }
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::FreeColormap { colormap } => {
                    let outputs = match runtime.free_colormap(context.namespace, colormap) {
                        // A window left naming the freed colormap has None,
                        // and its ColormapChange selectors are told (t210).
                        Ok(()) if colormap.local.raw() != u64::from(crate::X_SETUP_DEFAULT_COLORMAP) => runtime
                            .release_window_colormaps(colormap)
                            .into_iter()
                            .map(|window| {
                                XClientOutput::Event(XClientEvent::ColormapNotify {
                                    sequence: context.sequence,
                                    window,
                                    colormap: 0,
                                    new: true,
                                    state: 0,
                                })
                            })
                            .collect(),
                        Ok(()) => Vec::new(),
                        Err(_) => vec![color_error(
                            context,
                            XErrorCode::BadColor,
                            u32::try_from(colormap.local.raw()).unwrap_or(0),
                        )],
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::AllocNamedColor { colormap, ref name }
                | XWireRequest::LookupColor { colormap, ref name } => {
                    let lookup = matches!(request, XWireRequest::LookupColor { .. });
                    let output = match runtime.colormap_visual(context.namespace, colormap) {
                        Err(_) => color_error(
                            context,
                            XErrorCode::BadColor,
                            u32::try_from(colormap.local.raw()).unwrap_or(0),
                        ),
                        Ok(visual_id) => match x_lookup_color_name(name) {
                            None => color_error(context, XErrorCode::BadName, 0),
                            Some(exact) => {
                                let visual = x_true_color_visual(visual_id)
                                    .expect("registered colormaps must name advertised visuals");
                                let screen = visual.screen_color(exact);
                                if !lookup {
                                    runtime.allocate_color(context.namespace, context.client_id, colormap, visual.pixel(screen));
                                }
                                XClientOutput::Reply(if lookup {
                                    XClientReply::LookupColor {
                                        sequence: context.sequence, exact, screen,
                                    }
                                } else { XClientReply::AllocNamedColor {
                                    sequence: context.sequence,
                                    pixel: visual.pixel(screen),
                                    exact,
                                    screen,
                                } })
                            }
                        },
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::AllocColor {
                    colormap,
                    red,
                    green,
                    blue,
                } => {
                    let output = match runtime.colormap_visual(context.namespace, colormap) {
                        Err(_) => color_error(
                            context,
                            XErrorCode::BadColor,
                            u32::try_from(colormap.local.raw()).unwrap_or(0),
                        ),
                        Ok(visual_id) => {
                            let visual = x_true_color_visual(visual_id)
                                .expect("registered colormaps must name advertised visuals");
                            let screen = visual.screen_color(XColorRgb16 { red, green, blue });
                            runtime.allocate_color(context.namespace, context.client_id, colormap, visual.pixel(screen));
                            XClientOutput::Reply(XClientReply::AllocColor {
                                sequence: context.sequence,
                                pixel: visual.pixel(screen),
                                red: screen.red,
                                green: screen.green,
                                blue: screen.blue,
                            })
                        }
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
        _ => unreachable!("request family checked before dispatch"),
    }
}
