fn dispatch_core_property_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
    properties: &mut XPropertyTable,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
            XWireRequest::InternAtom { .. }
            | XWireRequest::GetAtomName { .. }
            | XWireRequest::ChangeProperty(..)
            | XWireRequest::DeleteProperty { .. }
            | XWireRequest::GetProperty(..)
            | XWireRequest::ListProperties { .. }
            | XWireRequest::GetSelectionOwner { .. }
            | XWireRequest::SendSelectionNotify { .. }
    ) {
        return Unhandled(request);
    }
    Handled(match request {
                XWireRequest::InternAtom {
                    only_if_exists,
                    name,
                } => {
                    let output = match atoms.intern(&name, only_if_exists) {
                        Ok(atom) => XClientOutput::Reply(XClientReply::InternAtom {
                            sequence: context.sequence,
                            atom: atom.unwrap_or(0),
                        }),
                        Err(_) => XClientOutput::Error(crate::XClientError {
                            code: crate::XErrorCode::BadValue,
                            sequence: context.sequence,
                            resource_id: 0,
                            minor_code: 0,
                            major_code: context.major_opcode,
                        }),
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::GetAtomName { atom } => {
                    let output = match atoms.name(atom) {
                        Some(name) => XClientOutput::Reply(XClientReply::GetAtomName {
                            sequence: context.sequence,
                            name: name.to_owned(),
                        }),
                        None => XClientOutput::Error(crate::XClientError {
                            code: crate::XErrorCode::BadAtom,
                            sequence: context.sequence,
                            resource_id: atom,
                            minor_code: 0,
                            major_code: context.major_opcode,
                        }),
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::ChangeProperty(change) => {
                    let transaction = context.transaction;
                    // GTK rewrites its initial EWMH hints between hide and
                    // show. An earlier map having published Engine state must
                    // not turn that next-map hint into a fatal protocol error.
                    // Pending and mapped surfaces remain authority-owned.
                    let initial_net_wm_state = atoms.name(change.property)
                        == Some(crate::X_ATOM_NAME_NET_WM_STATE)
                        && runtime.window_map_state(context.namespace, change.window)
                            == Ok(crate::XMapState::Unmapped)
                        && runtime.window_policy_map_pending(context.namespace, change.window)
                            == Ok(false);
                    let window_access = if change.window.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT) { Ok(()) } else { runtime.validate_window_access(context.namespace, change.window) };
                    // Both atoms must name something, and the window is
                    // checked before either: the protocol fixes that order,
                    // and a request with two things wrong must report the
                    // same one every server would.
                    let invalid_atom = window_access.is_ok().then(|| {
                        [change.property, change.property_type]
                            .into_iter()
                            .find(|atom| atoms.name(*atom).is_none())
                    }).flatten();
                    if let Some(atom) = invalid_atom {
                        return Handled(XDispatchResult {
                            response: None,
                            outputs: vec![XClientOutput::Error(crate::XClientError {
                                code: XErrorCode::BadAtom,
                                sequence: context.sequence,
                                resource_id: atom,
                                minor_code: 0,
                                major_code: context.major_opcode,
                            })],
                            metadata_candidates: Vec::new(),
                        });
                    }
                    let (output, metadata_candidates, response) = match window_access {
                        Err(error) => (
                            XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                u32::try_from(change.window.local.raw()).unwrap_or(0))),
                            Vec::new(),
                            None,
                        ),
                        Ok(()) => match if initial_net_wm_state {
                            properties.apply_initial_net_wm_state(context.namespace, change.clone(), atoms)
                        } else {
                            properties.apply_change(context.namespace, change.clone())
                        } {
                            Ok(record) => {
                                if let Some(Ok(constraints)) =
                                    decode_x_size_hints(&record, atoms, context.byte_order)
                                {
                                    let _ = runtime.set_window_constraints(
                                        context.namespace,
                                        record.window,
                                        constraints,
                                    );
                                }
                                let transient = decode_x_transient_for(
                                    &record,
                                    atoms,
                                    context.byte_order,
                                );
                                let window_type = decode_x_window_type_facts(
                                    &record,
                                    atoms,
                                    context.byte_order,
                                );
                                let response = transient.map(|decoded| {
                                    let decode_valid = decoded.is_ok();
                                    let owner = decoded.ok();
                                    let mut response =
                                        XAuthorityResponsePacket::accepted(transaction);
                                    if let Ok(surface) = runtime.set_window_transient_for(
                                        context.namespace,
                                        record.window,
                                        owner,
                                    ) {
                                        tracing::debug!(
                                            "sophia_x11_transient_for schema=1 window={} present=true decode_valid={} owner_is_root={} owner_reduced={} content=redacted",
                                            record.window.local.raw(),
                                            decode_valid,
                                            owner.is_some_and(|owner| owner.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT)),
                                            surface.presentation_owner.is_some(),
                                        );
                                        response.surfaces.push(surface);
                                    }
                                    response
                                }).or_else(|| window_type.map(|decoded| {
                                    let facts = decoded.unwrap_or_default();
                                    let mut response =
                                        XAuthorityResponsePacket::accepted(transaction);
                                    if let Ok(surface) = runtime.set_window_type_facts(
                                        context.namespace,
                                        record.window,
                                        facts,
                                    ) {
                                        tracing::debug!(
                                            "sophia_x11_window_type schema=2 window={} client_positioned={} kind={:?} placement={:?} decode_valid={} content=redacted",
                                            record.window.local.raw(),
                                            facts.client_positioned,
                                            facts.kind,
                                            facts.placement_preference,
                                            decoded.is_ok(),
                                        );
                                        response.surfaces.push(surface);
                                    }
                                    response
                                }));
                                let candidate = metadata_property_candidate(&record, atoms);
                                (
                                    XClientOutput::Event(XClientEvent::PropertyNotify {
                                        sequence: context.sequence,
                                        window: record.window,
                                        atom: record.property,
                                        time: context.server_time,
                                        new_value: true,
                                    }),
                                    candidate.into_iter().collect(),
                                    response,
                                )
                            }
                            Err(error) => (
                                XClientOutput::Error(crate::XClientError {
                                    // Appending or prepending to a property
                                    // whose type or format differs is the
                                    // protocol's Match error, not a Value
                                    // error: both arguments were in range and
                                    // it is the pair that does not agree.
                                    code: match error {
                                        crate::XPropertyError::AuthorityOwned => {
                                            crate::XErrorCode::BadAccess
                                        }
                                        crate::XPropertyError::TypeMismatch => {
                                            crate::XErrorCode::BadMatch
                                        }
                                        _ => crate::XErrorCode::BadValue,
                                    },
                                    sequence: context.sequence,
                                    resource_id: u32::try_from(change.window.local.raw()).unwrap_or(0),
                                    minor_code: 0,
                                    major_code: context.major_opcode,
                                }),
                                Vec::new(),
                                None,
                            ),
                        },
                    };
                    XDispatchResult {
                        response,
                        outputs: vec![output],
                        metadata_candidates,
                    }
                }
                XWireRequest::DeleteProperty { window, property } => {
                    let transaction = context.transaction;
                    let access = if window.local.raw() == u64::from(X_SETUP_DEFAULT_ROOT) {
                        Ok(())
                    } else {
                        runtime.validate_window_access(context.namespace, window)
                    };
                    let (outputs, response) = match access {
                        Err(error) => (
                            vec![XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                0,
                                u32::try_from(window.local.raw()).unwrap_or(0)))],
                            None,
                        ),
                        Ok(()) => {
                            let removed = properties.remove(context.namespace, window, property);
                            let Ok(removed) = removed else {
                                return XDispatchFamilyResult::Handled(XDispatchResult {
                                    response: None,
                                    outputs: vec![XClientOutput::Error(crate::XClientError {
                                        code: crate::XErrorCode::BadAccess,
                                        sequence: context.sequence,
                                        resource_id: property,
                                        minor_code: 0,
                                        major_code: context.major_opcode,
                                    })],
                                    metadata_candidates: Vec::new(),
                                });
                            };
                            let response = match atoms.name(property) {
                                Some("WM_TRANSIENT_FOR") => Some({
                                    let mut response =
                                        XAuthorityResponsePacket::accepted(transaction);
                                    if let Ok(surface) = runtime.set_window_transient_for(
                                        context.namespace,
                                        window,
                                        None,
                                    ) {
                                        response.surfaces.push(surface);
                                    }
                                    response
                                }),
                                Some("_NET_WM_WINDOW_TYPE") => Some({
                                    let mut response =
                                        XAuthorityResponsePacket::accepted(transaction);
                                    if let Ok(surface) = runtime.set_window_type_facts(
                                        context.namespace,
                                        window,
                                        crate::XWindowTypeFacts::default(),
                                    ) {
                                        response.surfaces.push(surface);
                                    }
                                    response
                                }),
                                _ => None,
                            };
                            let outputs = removed
                            .map(|_| {
                                if atoms.name(property) == Some("WM_NORMAL_HINTS") {
                                    let _ = runtime.set_window_constraints(
                                        context.namespace,
                                        window,
                                        sophia_protocol::SurfaceConstraints {
                                            min_size: None,
                                            max_size: None,
                                        },
                                    );
                                }
                                XClientOutput::Event(XClientEvent::PropertyNotify {
                                    sequence: context.sequence,
                                    window,
                                    atom: property,
                                    time: context.server_time,
                                    new_value: false,
                                })
                                })
                                .into_iter()
                            .collect();
                            (outputs, response)
                        }
                    };
                    XDispatchResult {
                        response,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::GetProperty(read) => {
                    let window = read.window;
                    let property = read.property;
                    let outputs = if property == crate::X_PROPERTY_ANY_TYPE
                        || atoms.name(read.property).is_none()
                        || atom_type_is_unknown(atoms, read.property_type)
                    {
                        vec![XClientOutput::Error(crate::XClientError {
                            code: crate::XErrorCode::BadAtom,
                            sequence: context.sequence,
                            resource_id: property,
                            minor_code: 0,
                            major_code: context.major_opcode,
                        })]
                    } else if window.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT) {
                        x_client_outputs_from_property_read(
                            &context,
                            window,
                            property,
                            properties.read_property(context.namespace, read),
                        )
                    } else if let Err(error) =
                        runtime.validate_window_access(context.namespace, window)
                    {
                        vec![XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            0,
                            u32::try_from(window.local.raw()).unwrap_or(0)))]
                    } else {
                        x_client_outputs_from_property_read(
                            &context,
                            window,
                            property,
                            properties.read_property(context.namespace, read),
                        )
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::ListProperties { window } => {
                    let output = if window.local.raw() == u64::from(X_SETUP_DEFAULT_ROOT) {
                        XClientOutput::Reply(XClientReply::ListProperties {
                            sequence: context.sequence,
                            atoms: properties.properties_for_window(context.namespace, window),
                        })
                    } else if let Err(error) = runtime.validate_window_access(context.namespace, window) {
                        XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            0,
                            u32::try_from(window.local.raw()).unwrap_or(0)))
                    } else {
                        XClientOutput::Reply(XClientReply::ListProperties {
                            sequence: context.sequence,
                            atoms: properties.properties_for_window(context.namespace, window),
                        })
                    };
                    XDispatchResult {
                        response: None,
                        outputs: vec![output],
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::GetSelectionOwner { selection } => XDispatchResult {
                    response: None,
                    // The selection is the request's only argument and its
                    // only error: an atom that names nothing is refused
                    // rather than answered with "nobody owns it", which is a
                    // different and reachable fact.
                    outputs: vec![if atoms.name(selection).is_none() {
                        XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadAtom,
                            sequence: context.sequence,
                            resource_id: selection,
                            minor_code: 0,
                            major_code: context.major_opcode,
                        })
                    } else {
                        XClientOutput::Reply(XClientReply::GetSelectionOwner {
                            sequence: context.sequence,
                            owner: runtime.selection_owner(context.namespace, selection),
                        })
                    }],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::SendSelectionNotify {
                    destination,
                    event_mask,
                    mut event,
                } => {
                    let requestor = match &event {
                        XClientEvent::SelectionNotify { requestor, .. } => Some(*requestor),
                        XClientEvent::ClientMessage { .. } => None,
                        _ => unreachable!("wire decoder admits only sendable events"),
                    };
                    let validation = if destination.local.raw() == u64::from(X_SETUP_DEFAULT_ROOT) {
                        Ok(())
                    } else {
                        runtime.validate_window_access(context.namespace, destination)
                    }
                    .and_then(|()| {
                        requestor
                            .map(|requestor| runtime.validate_window_access(context.namespace, requestor))
                            .unwrap_or(Ok(()))
                    });
                    let outputs = match validation {
                        Ok(())
                            if requestor
                                .is_none_or(|requestor| event_mask == 0 && destination == requestor) =>
                        {
                            match &mut event {
                                XClientEvent::SelectionNotify { sequence, .. }
                                | XClientEvent::ClientMessage { sequence, .. } => {
                                    *sequence = context.sequence;
                                }
                                _ => unreachable!("wire decoder admits only sendable events"),
                            }
                            vec![XClientOutput::Event(event)]
                        }
                        Ok(()) => vec![XClientOutput::Error(crate::XClientError {
                            code: XErrorCode::BadValue,
                            sequence: context.sequence,
                            resource_id: u32::try_from(destination.local.raw()).unwrap_or(0),
                            minor_code: 0,
                            major_code: context.major_opcode,
                        })],
                        Err(error) => vec![XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            0,
                            u32::try_from(destination.local.raw()).unwrap_or(0)))],
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
        _ => unreachable!("request family checked before dispatch"),
    })
}
