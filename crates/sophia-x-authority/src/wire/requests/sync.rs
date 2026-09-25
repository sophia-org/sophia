/// Decoded Sync requests; payloads retain their protocol representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XSyncRequest {
    SyncInitialize {
        desired_major: u8,
        desired_minor: u8,
    },
    SyncListSystemCounters,
    SyncCreateCounter {
        counter: XResourceId,
        initial_value: i64,
    },
    SyncSetCounter {
        counter: XResourceId,
        value: i64,
    },
    SyncChangeCounter {
        counter: XResourceId,
        delta: i64,
    },
    SyncQueryCounter {
        counter: XResourceId,
    },
    SyncDestroyCounter {
        counter: XResourceId,
    },
    SyncDestroyFence {
        fence: XResourceId,
    },
}
