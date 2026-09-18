//! Project Engine's committed indicator publication onto the shell wire.
//!
//! Lives outside `live_session` because it is a pure mapping between an Engine
//! publication and a protocol snapshot: it opens no transport, holds no session
//! state, and reads nothing the native backend owns. Keeping it here is what
//! lets the default build, and a host that only wants the mapping, use it
//! without compiling the live session in.

use sophia_protocol::{OutputId, ShellIndicator, ShellIndicatorSnapshot, ShellOutputStatus};

pub fn indicator_snapshot(
    publication: &sophia_engine::PolicyIndicatorPublication,
    active_output: Option<OutputId>,
    connection_epoch: u64,
) -> ShellIndicatorSnapshot {
    ShellIndicatorSnapshot {
        connection_epoch,
        generation: publication.generation,
        active_output,
        statuses: publication
            .output_statuses
            .iter()
            .map(|status| ShellOutputStatus {
                output: status.output,
                focus_bits: status.focus_bits,
                layout: status.layout.clone(),
            })
            .collect(),
        indicators: publication
            .indicators
            .iter()
            .map(|indicator| ShellIndicator {
                output: indicator.output,
                indicator: indicator.indicator,
                // Identities are allocated from one, so zero is free to mean
                // "not activatable" and can never collide with a real action.
                action: indicator.action.map_or(0, sophia_protocol::WmActionId::raw),
                slot: indicator.slot,
                state_bits: indicator.state_bits,
                label: indicator.label.clone(),
            })
            .collect(),
    }
}
