//! Opt-in transport timing with the pinned independent C peer. The test owner
//! validates supplied candidates and settles them without any device access.
//! Run through `cargo xtask check output-file-performance`; ordinary tests
//! exercise the harness checks but never run a timing workload.
#[path = "support/output_file_performance/mod.rs"]
mod performance;

#[test]
#[ignore = "explicit isolated output transport performance qualification"]
fn output_file_performance() {
    if cfg!(debug_assertions) {
        panic!("qualification requires --release");
    }
    performance::run().unwrap();
}
