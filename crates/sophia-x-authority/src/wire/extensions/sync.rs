fn decode_sync_value(context: XWireClientContext, bytes: &[u8]) -> i64 {
    let high = i64::from(context.byte_order.u32(&bytes[8..12]) as i32);
    let low = i64::from(context.byte_order.u32(&bytes[12..16]));
    (high << 32) | low
}

fn decode_sync(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    match bytes[1] {
        X_SYNC_INITIALIZE_MINOR_OPCODE => {
            require_exact_len(X_SYNC_MAJOR_OPCODE, 8, bytes.len())?;
            Ok(XWireRequest::Sync(crate::XSyncRequest::SyncInitialize {
                desired_major: bytes[4],
                desired_minor: bytes[5],
            }))
        }
        X_SYNC_LIST_SYSTEM_COUNTERS_MINOR_OPCODE => {
            require_exact_len(X_SYNC_MAJOR_OPCODE, 4, bytes.len())?;
            Ok(XWireRequest::Sync(crate::XSyncRequest::SyncListSystemCounters))
        }
        X_SYNC_CREATE_COUNTER_MINOR_OPCODE => {
            require_exact_len(X_SYNC_MAJOR_OPCODE, 16, bytes.len())?;
            let counter = context.byte_order.u32(&bytes[4..8]);
            context.validate_new_resource_id(counter)?;
            Ok(XWireRequest::Sync(crate::XSyncRequest::SyncCreateCounter {
                counter: XResourceId::new(u64::from(counter), 1),
                initial_value: decode_sync_value(context, bytes),
            }))
        }
        X_SYNC_SET_COUNTER_MINOR_OPCODE | X_SYNC_CHANGE_COUNTER_MINOR_OPCODE => {
            require_exact_len(X_SYNC_MAJOR_OPCODE, 16, bytes.len())?;
            let counter =
                XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1);
            let value = decode_sync_value(context, bytes);
            if bytes[1] == X_SYNC_SET_COUNTER_MINOR_OPCODE {
                Ok(XWireRequest::Sync(crate::XSyncRequest::SyncSetCounter { counter, value }))
            } else {
                Ok(XWireRequest::Sync(crate::XSyncRequest::SyncChangeCounter {
                    counter,
                    delta: value,
                }))
            }
        }
        X_SYNC_QUERY_COUNTER_MINOR_OPCODE | X_SYNC_DESTROY_COUNTER_MINOR_OPCODE => {
            require_exact_len(X_SYNC_MAJOR_OPCODE, 8, bytes.len())?;
            let counter =
                XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1);
            if bytes[1] == X_SYNC_QUERY_COUNTER_MINOR_OPCODE {
                Ok(XWireRequest::Sync(crate::XSyncRequest::SyncQueryCounter { counter }))
            } else {
                Ok(XWireRequest::Sync(crate::XSyncRequest::SyncDestroyCounter { counter }))
            }
        }
        X_SYNC_DESTROY_FENCE_MINOR_OPCODE => {
            require_exact_len(X_SYNC_MAJOR_OPCODE, 8, bytes.len())?;
            Ok(XWireRequest::Sync(crate::XSyncRequest::SyncDestroyFence {
                fence: XResourceId::new(
                    u64::from(context.byte_order.u32(&bytes[4..8])),
                    1,
                ),
            }))
        }
        other => Err(XWireParseError::UnknownOpcode(other)),
    }
}
