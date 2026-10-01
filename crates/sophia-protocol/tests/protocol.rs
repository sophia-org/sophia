//! Remaining socket framing and broker packets. Its neutral modules
//! are separate binaries (`protocol_*.rs`) that build without the IPC module.
use sophia_protocol::*;

include!("protocol/framing.rs");
include!("protocol/broker_v1.rs");
