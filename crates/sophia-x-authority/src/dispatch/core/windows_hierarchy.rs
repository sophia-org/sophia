// DestroyWindow, ReparentWindow, the subwindow requests, CirculateWindow
// and UnmapWindow: a window's place in the hierarchy and its map state.
// Included by dispatch.rs beside windows.rs; one module with it.

fn dispatch_window_hierarchy_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    _atoms: &mut XAtomTable,
    properties: &mut XPropertyTable,
) -> XDispatchResult {
    match request {
                XWireRequest::Core(crate::XCoreRequest::DestroyWindow { window }) => {
                    let transaction = context.transaction;
                    let mut response = XAuthorityResponsePacket::accepted(transaction);
                    let outputs = match runtime
                        .destroy_window_subtree(context.namespace, window)
                    {
                        Ok(destroyed) => {
                            // Destruction order is descendants first, and the
                            // notifications follow it: a client watching a
                            // subtree learns about a child before the parent
                            // that contained it.
                            destroyed
                                .into_iter()
                                .flat_map(|destroyed| {
                                    properties
                                        .remove_window(context.namespace, destroyed.window);
                                    response.removed_surfaces.push(destroyed.surface);
                                    // A mapped window is unmapped as part of
                                    // being destroyed, and that unmap is
                                    // reported in its own right, before the
                                    // destroy. `from_configure` is false: an
                                    // explicit request caused it.
                                    let unmap = (destroyed.was_mapped && destroyed.is_subtree_root).then_some(
                                        XClientOutput::Event(
                                            crate::XClientEvent::UnmapNotify {
                                                sequence: context.sequence,
                                                event: destroyed.window,
                                                window: destroyed.window,
                                                from_configure: false,
                                            },
                                        ),
                                    );
                                    // Addressed to the window itself. The router
                                    // adds the parent-addressed copy for
                                    // SubstructureNotify selectors, the same way
                                    // it does for map and unmap, so both forms
                                    // come from one path.
                                    unmap.into_iter().chain(std::iter::once(
                                        XClientOutput::Event(
                                            crate::XClientEvent::DestroyNotify {
                                                sequence: context.sequence,
                                                event: destroyed.window,
                                                window: destroyed.window,
                                            },
                                        ),
                                    ))
                                })
                                .collect()
                        }
                        Err(error) => {
                            response = XAuthorityResponsePacket::rejected(transaction, error);
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                u32::try_from(window.local.raw()).unwrap_or(0)))]
                        }
                    };
                    XDispatchResult {
                        response: Some(response),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::ReparentWindow {
                    window,
                    parent,
                    x,
                    y,
                }) => {
                    let transaction = context.transaction;
                    let (response, outputs) = match runtime.reparent_window(
                        context.namespace,
                        window,
                        parent,
                        x,
                        y,
                        u64::from(context.sequence),
                    ) {
                        Ok(reparent) => {
                            let mut response = XAuthorityResponsePacket::accepted(transaction);
                            response.surfaces.extend(reparent.surfaces);
                            // Reported as the protocol orders it (t184): a
                            // mapped window is unmapped, reparented and
                            // mapped again. Each notice is addressed to the
                            // window for its StructureNotify selectors and to
                            // a parent for that parent's SubstructureNotify
                            // selectors -- the old parent for the unmap and
                            // the reparent, the new one for the reparent and
                            // the remap -- as a destroy's parent copy is; the
                            // router delivers each by the window it names.
                            let sequence = context.sequence;
                            let override_redirect = reparent.override_redirect;
                            let notice = |event| {
                                XClientOutput::Event(crate::XClientEvent::ReparentNotify {
                                    sequence,
                                    event,
                                    window,
                                    parent,
                                    x,
                                    y,
                                    override_redirect,
                                })
                            };
                            let mut outputs = Vec::new();
                            if reparent.was_mapped {
                                for event in [window, reparent.old_parent] {
                                    outputs.push(XClientOutput::Event(crate::XClientEvent::UnmapNotify {
                                        sequence,
                                        event,
                                        window,
                                        from_configure: false,
                                    }));
                                }
                            }
                            outputs.push(notice(window));
                            outputs.push(notice(reparent.old_parent));
                            if parent != reparent.old_parent {
                                outputs.push(notice(parent));
                            }
                            if reparent.was_mapped {
                                for event in [window, parent] {
                                    outputs.push(XClientOutput::Event(crate::XClientEvent::MapNotify {
                                        sequence,
                                        event,
                                        window,
                                        override_redirect,
                                    }));
                                }
                            }
                            (response, outputs)
                        }
                        Err(error) => (
                            XAuthorityResponsePacket::rejected(transaction, error),
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                u32::try_from(window.local.raw()).unwrap_or(0),
                            ))],
                        ),
                    };
                    XDispatchResult {
                        response: Some(response),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::DestroySubwindows { window }) => {
                    let transaction = context.transaction;
                    let mut response = XAuthorityResponsePacket::accepted(transaction);
                    let outputs = match runtime
                        .destroy_direct_subwindows(context.namespace, window)
                    {
                        Ok(destroyed) => destroyed
                            .into_iter()
                            .flat_map(|destroyed| {
                                properties.remove_window(context.namespace, destroyed.window);
                                response.removed_surfaces.push(destroyed.surface);
                                // As for a single destroy: a mapped window is
                                // unmapped on its way out, and that is reported
                                // before the destroy.
                                let unmap = (destroyed.was_mapped && destroyed.is_subtree_root).then_some(
                                    XClientOutput::Event(crate::XClientEvent::UnmapNotify {
                                        sequence: context.sequence,
                                        event: destroyed.window,
                                        window: destroyed.window,
                                        from_configure: false,
                                    }),
                                );
                                unmap.into_iter().chain(std::iter::once(
                                    XClientOutput::Event(crate::XClientEvent::DestroyNotify {
                                        sequence: context.sequence,
                                        event: destroyed.window,
                                        window: destroyed.window,
                                    }),
                                ))
                            })
                            .collect(),
                        Err(error) => {
                            response = XAuthorityResponsePacket::rejected(transaction, error);
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                u32::try_from(window.local.raw()).unwrap_or(0),
                            ))]
                        }
                    };
                    XDispatchResult {
                        response: Some(response),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::MapSubwindows { window, withheld }) => {
                    let transaction = context.transaction;
                    let mut response = XAuthorityResponsePacket::accepted(transaction);
                    let outputs = match runtime.map_direct_subwindows(
                        context.namespace,
                        window,
                        u64::from(context.sequence),
                        &withheld,
                    ) {
                        Ok(surfaces) => {
                            response.surfaces = surfaces;
                            response
                                .surfaces
                                .iter()
                                .flat_map(|surface| {
                                    let window = XResourceId {
                                        local: surface.local_id,
                                    };
                                    let map_state = runtime
                                        .window_map_state(context.namespace, window)
                                        .ok();
                                    if !matches!(
                                        map_state,
                                        Some(crate::XMapState::Unviewable | crate::XMapState::Viewable)
                                    ) {
                                        return Vec::new();
                                    }
                                    let override_redirect = runtime
                                        .window_override_redirect(context.namespace, window)
                                        .unwrap_or(false);
                                    let mut outputs = vec![XClientOutput::Event(
                                        XClientEvent::MapNotify {
                                            sequence: context.sequence,
                                            event: window,
                                            window,
                                            override_redirect,
                                        },
                                    )];
                                    if map_state == Some(crate::XMapState::Viewable)
                                        && !runtime.window_is_input_only(window)
                                    {
                                        outputs.push(XClientOutput::Event(
                                            XClientEvent::VisibilityNotify {
                                            sequence: context.sequence,
                                            window,
                                            state: 0,
                                            },
                                        ));
                                        outputs.push(XClientOutput::Event(XClientEvent::Expose {
                                            sequence: context.sequence,
                                            window,
                                            x: 0,
                                            y: 0,
                                            width: clamp_u16(surface.geometry.width),
                                            height: clamp_u16(surface.geometry.height),
                                            count: 0,
                                        }));
                                    }
                                    outputs
                                })
                                .collect()
                        }
                        Err(error) => {
                            response = XAuthorityResponsePacket::rejected(transaction, error);
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                u32::try_from(window.local.raw()).unwrap_or(0)))]
                        }
                    };
                    XDispatchResult {
                        response: Some(response),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                // Every mapped child, top to bottom, each with the UnmapNotify
                // UnmapWindow would give it; the router adds the parent's copy.
                XWireRequest::Core(crate::XCoreRequest::UnmapSubwindows { window }) => {
                    let transaction = context.transaction;
                    let mut response = XAuthorityResponsePacket::accepted(transaction);
                    let outputs = match runtime.unmap_direct_subwindows(context.namespace, window) {
                        Ok(unmapped) => unmapped
                            .into_iter()
                            .map(|(child, surface)| {
                                response.surfaces.push(surface);
                                XClientOutput::Event(crate::XClientEvent::UnmapNotify {
                                    sequence: context.sequence,
                                    event: child,
                                    window: child,
                                    from_configure: false,
                                })
                            })
                            .collect(),
                        Err(error) => {
                            response = XAuthorityResponsePacket::rejected(transaction, error);
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                u32::try_from(window.local.raw()).unwrap_or(0)))]
                        }
                    };
                    XDispatchResult {
                        response: Some(response),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                // The lowest occluded child to the top, or the highest
                // occluding one to the bottom, with a CirculateNotify to it
                // (the router adds the parent's copy). No Expose: windows are
                // retained surfaces here, and raising one uncovers nothing
                // that was lost. A redirected circulate never reaches this
                // arm; the socket layer turns it into a CirculateRequest.
                XWireRequest::Core(crate::XCoreRequest::CirculateWindow { window, direction }) => {
                    let transaction = context.transaction;
                    let mut response = XAuthorityResponsePacket::accepted(transaction);
                    let moved = runtime
                        .circulate_candidate(context.namespace, window, direction)
                        .and_then(|candidate| match candidate {
                            Some(child) => runtime
                                .circulate_window(context.namespace, child, direction)
                                .map(|surface| Some((child, surface))),
                            None => Ok(None),
                        });
                    let outputs = match moved {
                        Ok(Some((child, surface))) => {
                            response.surfaces.push(surface);
                            vec![XClientOutput::Event(crate::XClientEvent::CirculateNotify {
                                sequence: context.sequence,
                                event: child,
                                window: child,
                                place: direction,
                            })]
                        }
                        Ok(None) => Vec::new(),
                        Err(error) => {
                            response = XAuthorityResponsePacket::rejected(transaction, error);
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                u32::try_from(window.local.raw()).unwrap_or(0)))]
                        }
                    };
                    XDispatchResult {
                        response: Some(response),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Core(crate::XCoreRequest::UnmapWindow { window }) => {
                    let transaction = context.transaction;
                    let mut response = XAuthorityResponsePacket::accepted(transaction);
                    let outputs = match runtime.unmap_window(context.namespace, window) {
                        Ok(Some(surface)) => {
                            response.surfaces.push(surface);
                            // Addressed to the window itself; the router adds
                            // the parent-addressed copy for SubstructureNotify
                            // selectors, as it does for map and destroy.
                            //
                            // `from_configure` is false: this is a client
                            // asking, not a window falling out of view because
                            // an ancestor was reconfigured.
                            vec![XClientOutput::Event(crate::XClientEvent::UnmapNotify {
                                sequence: context.sequence,
                                event: window,
                                window,
                                from_configure: false,
                            })]
                        }
                        // Already unmapped: the request has no effect, and an
                        // event here would report a transition that never
                        // happened.
                        Ok(None) => Vec::new(),
                        Err(error) => {
                            response = XAuthorityResponsePacket::rejected(transaction, error);
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                u32::try_from(window.local.raw()).unwrap_or(0)))]
                        }
                    };
                    XDispatchResult {
                        response: Some(response),
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
        _ => unreachable!("request family checked before dispatch"),
    }
}
