//! Ids, surfaces, chrome descriptors, broker health packets and disclosure values: neutral packet and data-model tests. Split from the
//! `protocol` binary, whose other modules need the socket codecs, so these
//! build and run without the IPC module.
use sophia_protocol::*;

include!("protocol/data_model.rs");
