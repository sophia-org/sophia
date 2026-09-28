//! Output configuration values: neutral packet and data-model tests. Split from the
//! `protocol` binary, whose other modules need the socket codecs, so these
//! build and run without the IPC module.
use sophia_protocol::*;

include!("protocol/output_fixture.rs");
include!("protocol/output_configuration.rs");
