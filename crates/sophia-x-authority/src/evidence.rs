//! Value-free lifecycle evidence. Tokens correlate resources within this process;
//! they are never resource authority and do not disclose wire identifiers.
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::sync::OnceLock;

use crate::{XClientEvent, XDispatchResult, XResourceId, XServerFrontendClientId};

fn token(resource: XResourceId) -> u64 {
    static TOKENS: OnceLock<RandomState> = OnceLock::new();
    TOKENS.get_or_init(RandomState::new).hash_one(resource)
}

pub(crate) fn window_dispatch(
    client: XServerFrontendClientId,
    sequence: u16,
    major: u8,
    requested_kind: Option<u16>,
    output: &XDispatchResult,
) {
    if !matches!(major, 1 | 2 | 4 | 7..=12 | 18 | 19) {
        return;
    }
    let Some(response) = &output.response else {
        return;
    };
    let requested_kind = match requested_kind {
        Some(0) => "inherit",
        Some(1) => "input_output",
        Some(2) => "input_only",
        Some(_) => "invalid",
        None => "unspecified",
    };
    for surface in &response.surfaces {
        tracing::debug!(target: "sophia_application_evidence",
            "sophia_x_window_lifecycle schema=1 client={} transaction={} sequence={} major={} surface={} generation={} window_token={} requested_kind={} role={:?} mapped={} width={} height={}",
            client.raw(), response.transaction.raw(), sequence, major,
            surface.surface.index(), surface.surface.generation(),
            token(XResourceId { local: surface.local_id }), requested_kind,
            surface.presentation, surface.mapped, surface.geometry.width, surface.geometry.height,
        );
    }
    for surface in &response.removed_surfaces {
        tracing::debug!(target: "sophia_application_evidence",
            "sophia_x_window_lifecycle schema=1 client={} transaction={} sequence={} major={} surface={} generation={} status=removed",
            client.raw(), response.transaction.raw(), sequence, major,
            surface.index(), surface.generation(),
        );
    }
}

pub(crate) use present::{accepted as present_accepted, delivery as present_event};

mod present;
pub use present::{PresentEvidenceScope, aggregate_present_evidence, flush_present_evidence};
