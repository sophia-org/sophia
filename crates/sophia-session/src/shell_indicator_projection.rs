//! Project Engine's committed indicator publication onto the shell wire.
//!
//! Lives outside `live_session` because it is a pure mapping between an Engine
//! publication and a protocol snapshot: it opens no transport, holds no session
//! state, and reads nothing the native backend owns. Keeping it here is what
//! lets the default build, and a host that only wants the mapping, use it
//! without compiling the live session in.

use sophia_protocol::{OutputId, ShellIndicator, ShellIndicatorSnapshot, ShellOutputStatus};

/// Compare the wire projection without cloning labels or building a snapshot.
/// Focus may change without an indicator generation change.
pub fn indicator_snapshot_matches(
    snapshot: &ShellIndicatorSnapshot,
    publication: &sophia_engine::PolicyIndicatorPublication,
    active_output: Option<OutputId>,
    connection_epoch: u64,
) -> bool {
    snapshot.connection_epoch == connection_epoch
        && snapshot.generation == publication.generation
        && snapshot.active_output == active_output
        && snapshot.statuses.len() == publication.output_statuses.len()
        && snapshot
            .statuses
            .iter()
            .zip(&publication.output_statuses)
            .all(|(s, p)| {
                s.output == p.output && s.focus_bits == p.focus_bits && s.layout == p.layout
            })
        && snapshot.indicators.len() == publication.indicators.len()
        && snapshot
            .indicators
            .iter()
            .zip(&publication.indicators)
            .all(|(s, p)| {
                s.output == p.output
                    && s.indicator == p.indicator
                    && s.action == p.action.map_or(0, sophia_protocol::WmActionId::raw)
                    && s.slot == p.slot
                    && s.state_bits == p.state_bits
                    && s.label == p.label
            })
}

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
