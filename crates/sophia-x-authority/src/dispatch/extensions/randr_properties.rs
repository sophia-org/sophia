/// Atom existence does not imply a property exists on an output (notably
/// EDID). Enumeration, metadata and reads share this bounded definition.
struct RandrOutputProperty {
    name: &'static str,
    property_type: crate::XAtom,
    value: i32,
    range: [i32; 2],
}

const RANDR_OUTPUT_PROPERTIES: &[RandrOutputProperty] = &[RandrOutputProperty {
    name: crate::X_ATOM_NAME_RANDR_NON_DESKTOP,
    property_type: crate::X_ATOM_INTEGER,
    value: 0,
    range: [0, 1],
}];

fn dispatch_randr_properties(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &XAuthorityRuntime,
    atoms: &XAtomTable,
) -> XDispatchFamilyResult {
    use crate::XRandrRequest::*;
    let (output, minor) = match &request {
        XWireRequest::Randr(RandrListOutputProperties { output }) => {
            (*output, crate::X_RANDR_LIST_OUTPUT_PROPERTIES_MINOR_OPCODE)
        }
        XWireRequest::Randr(RandrQueryOutputProperty { output, .. }) => {
            (*output, crate::X_RANDR_QUERY_OUTPUT_PROPERTY_MINOR_OPCODE)
        }
        XWireRequest::Randr(RandrGetOutputProperty { output, .. }) => {
            (*output, crate::X_RANDR_GET_OUTPUT_PROPERTY_MINOR_OPCODE)
        }
        _ => return Unhandled(request),
    };
    let error = |code, resource_id| {
        XClientOutput::Error(crate::XClientError {
            code,
            resource_id,
            sequence: context.sequence,
            minor_code: minor.into(),
            major_code: context.major_opcode,
        })
    };
    let reply = XClientOutput::Reply;
    let result = if !randr_resources(runtime.output_topology())
        .outputs
        .contains(&output)
    {
        error(XErrorCode::RandrBadOutput, output)
    } else {
        match request {
            XWireRequest::Randr(RandrListOutputProperties { .. }) => {
                reply(XClientReply::RandrListOutputProperties {
                    sequence: context.sequence,
                    atoms: RANDR_OUTPUT_PROPERTIES
                        .iter()
                        .map(|property| {
                            atoms
                                .atom(property.name)
                                .expect("fixed output property atom")
                        })
                        .collect(),
                })
            }
            XWireRequest::Randr(RandrQueryOutputProperty { property, .. }) => {
                match atoms.name(property) {
                    None => error(XErrorCode::BadAtom, property),
                    Some(name) => match RANDR_OUTPUT_PROPERTIES.iter().find(|p| p.name == name) {
                        None => error(XErrorCode::BadName, property),
                        Some(p) => reply(XClientReply::RandrQueryOutputProperty {
                            sequence: context.sequence,
                            pending: false,
                            range: true,
                            immutable: true,
                            valid_values: p.range.to_vec(),
                        }),
                    },
                }
            }
            XWireRequest::Randr(RandrGetOutputProperty {
                property,
                property_type,
                long_offset,
                long_length,
                delete,
                ..
            }) => {
                if atoms.name(property).is_none() {
                    error(XErrorCode::BadAtom, property)
                } else if property_type != 0 && atoms.name(property_type).is_none() {
                    error(XErrorCode::BadAtom, property_type)
                } else if let Some(p) = RANDR_OUTPUT_PROPERTIES
                    .iter()
                    .find(|p| Some(p.name) == atoms.name(property))
                {
                    if delete {
                        error(XErrorCode::BadAccess, property)
                    } else {
                        // No pending value exists for these immutable facts;
                        // pending reads select the current value as well.
                        let mut value = Vec::with_capacity(4);
                        context.byte_order.push_u32(&mut value, p.value as u32);
                        randr_output_property_from_read(
                            &context,
                            long_offset,
                            crate::read_property_value(
                                p.property_type,
                                32,
                                &value,
                                property_type,
                                long_offset,
                                long_length,
                            ),
                        )
                    }
                } else {
                    reply(XClientReply::RandrGetOutputProperty {
                        sequence: context.sequence,
                        property_type: 0,
                        bytes_after: 0,
                        format: 0,
                        data: Vec::new(),
                    })
                }
            }
            _ => unreachable!("property request classified above"),
        }
    };
    Handled(XDispatchResult {
        response: None,
        outputs: vec![result],
        metadata_candidates: Vec::new(),
    })
}
