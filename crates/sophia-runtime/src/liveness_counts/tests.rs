#![cfg(test)]
// The proof lives with the other device-free harnesses; private items are
// reachable because this module is a child of the one under test.
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/liveness_counts.rs"
));
