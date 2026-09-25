fn dispatch_sync_request(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &mut XAuthorityRuntime,
    _atoms: &mut XAtomTable,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
            XWireRequest::Sync(crate::XSyncRequest::SyncInitialize { .. })
            | XWireRequest::Sync(crate::XSyncRequest::SyncListSystemCounters)
            | XWireRequest::Sync(crate::XSyncRequest::SyncCreateCounter { .. })
            | XWireRequest::Sync(crate::XSyncRequest::SyncSetCounter { .. })
            | XWireRequest::Sync(crate::XSyncRequest::SyncChangeCounter { .. })
            | XWireRequest::Sync(crate::XSyncRequest::SyncQueryCounter { .. })
            | XWireRequest::Sync(crate::XSyncRequest::SyncDestroyCounter { .. })
            | XWireRequest::Sync(crate::XSyncRequest::SyncDestroyFence { .. })
    ) {
        return Unhandled(request);
    }
    Handled(match request {
                XWireRequest::Sync(crate::XSyncRequest::SyncInitialize { .. }) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(XClientReply::SyncInitialize {
                        sequence: context.sequence,
                        major_version: 3,
                        minor_version: 1,
                    })],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::Sync(crate::XSyncRequest::SyncListSystemCounters) => XDispatchResult {
                    response: None,
                    outputs: vec![XClientOutput::Reply(
                        XClientReply::SyncListSystemCounters {
                            sequence: context.sequence,
                        },
                    )],
                    metadata_candidates: Vec::new(),
                },
                XWireRequest::Sync(crate::XSyncRequest::SyncCreateCounter {
                    counter,
                    initial_value,
                }) => {
                    let outputs = runtime
                        .create_sync_counter(
                            context.namespace,
                            counter,
                            u64::from(context.sequence),
                            initial_value,
                        )
                        .err()
                        .map(|error| {
                            XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_SYNC_CREATE_COUNTER_MINOR_OPCODE),
                                u32::try_from(counter.local.raw()).unwrap_or(0)))
                        })
                        .into_iter()
                        .collect();
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Sync(crate::XSyncRequest::SyncSetCounter { counter, value }) => {
                    let outputs = runtime
                        .set_sync_counter(context.namespace, counter, value)
                        .err()
                        .map(|error| {
                            XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_SYNC_SET_COUNTER_MINOR_OPCODE),
                                u32::try_from(counter.local.raw()).unwrap_or(0)))
                        })
                        .into_iter()
                        .collect();
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Sync(crate::XSyncRequest::SyncChangeCounter { counter, delta }) => {
                    let outputs = runtime
                        .change_sync_counter(context.namespace, counter, delta)
                        .err()
                        .map(|error| {
                            XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_SYNC_CHANGE_COUNTER_MINOR_OPCODE),
                                u32::try_from(counter.local.raw()).unwrap_or(0)))
                        })
                        .into_iter()
                        .collect();
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Sync(crate::XSyncRequest::SyncQueryCounter { counter }) => {
                    let outputs = match runtime.sync_counter(context.namespace, counter) {
                        Ok(value) => vec![XClientOutput::Reply(XClientReply::SyncQueryCounter {
                            sequence: context.sequence,
                            value,
                        })],
                        Err(error) => vec![XClientOutput::Error(x_error_from_runtime(
                            error,
                            context.sequence,
                            context.major_opcode,
                            u16::from(crate::X_SYNC_QUERY_COUNTER_MINOR_OPCODE),
                            u32::try_from(counter.local.raw()).unwrap_or(0)))],
                    };
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Sync(crate::XSyncRequest::SyncDestroyCounter { counter }) => {
                    let outputs = runtime
                        .destroy_sync_counter(context.namespace, counter)
                        .err()
                        .map(|error| {
                            XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_SYNC_DESTROY_COUNTER_MINOR_OPCODE),
                                u32::try_from(counter.local.raw()).unwrap_or(0)))
                        })
                        .into_iter()
                        .collect();
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
                XWireRequest::Sync(crate::XSyncRequest::SyncDestroyFence { fence }) => {
                    let outputs = runtime
                        .destroy_dri3_fence(context.namespace, fence)
                        .err()
                        .map(|error| {
                            XClientOutput::Error(x_error_from_runtime(
                                error,
                                context.sequence,
                                context.major_opcode,
                                u16::from(crate::X_SYNC_DESTROY_FENCE_MINOR_OPCODE),
                                u32::try_from(fence.local.raw()).unwrap_or(0)))
                        })
                        .into_iter()
                        .collect();
                    XDispatchResult {
                        response: None,
                        outputs,
                        metadata_candidates: Vec::new(),
                    }
                }
        _ => unreachable!("request family checked before dispatch"),
    })
}
