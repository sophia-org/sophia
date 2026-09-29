//! The socket side of the protocol packets: framing, the output role's
//! `sophia_output_v1` and the broker. Its neutral modules
//! are separate binaries (`protocol_*.rs`) that build without the IPC module.
use sophia_protocol::*;

include!("protocol/framing.rs");
include!("protocol/output_fixture.rs");
include!("protocol/output_ipc.rs");
include!("protocol/broker_v1.rs");
