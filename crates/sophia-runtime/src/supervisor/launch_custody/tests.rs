#![cfg(test)]
// Direct custody cases live with the other device-free harnesses; `new` is
// reachable because this module is a child of the one under test.
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/liveness_custody.rs"
));
