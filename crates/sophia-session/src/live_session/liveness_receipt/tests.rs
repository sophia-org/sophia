#![cfg(test)]
// The emitter shape is proved against the capture filter in-crate, so the
// rows the filter admits are the rows the owner prints.
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/live_session/liveness_receipt.rs"
));
