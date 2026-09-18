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

pub(crate) fn present_accepted(
    client: XServerFrontendClientId,
    transaction: sophia_protocol::TransactionId,
    window: XResourceId,
    pixmap: XResourceId,
    serial: u32,
    pending: usize,
) {
    tracing::debug!(target: "sophia_application_evidence",
        "sophia_x_present_submission schema=1 client={} transaction={} window_token={} pixmap_token={} serial={} pending_count={} status=accepted",
        client.raw(), transaction.raw(), token(window), token(pixmap), serial, pending,
    );
}

/// A writer records `written` only after write_all and flush succeed. Queue
/// admission is a separate observation, never a claim of peer consumption.
pub(crate) fn present_event(
    client: XServerFrontendClientId,
    transaction: Option<sophia_protocol::TransactionId>,
    status: &'static str,
    event: XClientEvent,
) {
    let (sequence, event_id, window, serial, kind, pixmap) = match event {
        XClientEvent::PresentCompleteNotify {
            sequence,
            event_id,
            window,
            serial,
            kind,
            ..
        } => (
            sequence,
            event_id,
            window,
            serial,
            if kind == 0 { "complete" } else { "msc" },
            None,
        ),
        XClientEvent::PresentIdleNotify {
            sequence,
            event_id,
            window,
            serial,
            pixmap,
            ..
        } => (sequence, event_id, window, serial, "idle", Some(pixmap)),
        _ => return,
    };
    tracing::debug!(target: "sophia_application_evidence",
        "sophia_x_present_delivery schema=1 client={} transaction={} sequence={} window_token={} subscription_token={} pixmap_token={} serial={} kind={} status={}",
        client.raw(), transaction.map_or(0, |id| id.raw()), sequence,
        token(window), token(event_id), pixmap.map_or(0, token), serial, kind, status,
    );
}
