//! Distinctly-valued typed fixtures for the t252 B5 native launcher (r7)
//! kinds: the flat opening/focus/focus-revoked/activation-outcome/closed/
//! allocation-request/input-ack/activate records, `NativeInput` (fixed,
//! zero-padded text) and the whole `NativeCandidate`.

use super::{aid, fmap, grant, margins_of, oid};
use sophia_shell_protocol::*;
use std::collections::BTreeMap;

#[allow(clippy::too_many_arguments)]
fn binding(
    grant_connection_epoch: u64,
    grant_content_epoch: u64,
    opening: u64,
    output_id: u64,
    output_generation: u64,
    allocation_id: u64,
    allocation_generation: u64,
    catalog_generation: u64,
    candidate_generation: u64,
    presentation_epoch: u64,
    interaction_generation: u64,
    state_revision: u64,
    focus_lease: u64,
) -> NativeLauncherBinding {
    NativeLauncherBinding {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        opening,
        output: oid(output_id, output_generation),
        allocation: aid(allocation_id, allocation_generation),
        catalog_generation,
        candidate_generation,
        presentation_epoch,
        interaction_generation,
        state_revision,
        focus_lease,
    }
}

/// The 13 flattened binding fields every wrapping record below repeats,
/// keyed as this module's KDL bodies name them (`state_revision` for the
/// plain-binding kinds, or the caller renames it to `binding_state_revision`
/// via [`fmap!`]'s explicit-name form when an outer event also has one).
macro_rules! binding_map {
    ($b:expr, $state_field:ident) => {{
        let grant_connection_epoch = $b.grant.connection_epoch;
        let grant_content_epoch = $b.grant.content_grant_epoch;
        let opening = $b.opening;
        let output_id = $b.output.id;
        let output_generation = $b.output.generation;
        let allocation_id = $b.allocation.id;
        let allocation_generation = $b.allocation.generation;
        let catalog_generation = $b.catalog_generation;
        let candidate_generation = $b.candidate_generation;
        let presentation_epoch = $b.presentation_epoch;
        let interaction_generation = $b.interaction_generation;
        let $state_field = $b.state_revision;
        let focus_lease = $b.focus_lease;
        fmap!(
            grant_connection_epoch,
            grant_content_epoch,
            opening,
            output_id,
            output_generation,
            allocation_id,
            allocation_generation,
            catalog_generation,
            candidate_generation,
            presentation_epoch,
            interaction_generation,
            $state_field,
            focus_lease
        )
    }};
}

pub fn native_opening() -> (u64, NativeLauncherOpening, BTreeMap<&'static str, i128>) {
    let transaction = 3001u64;
    let grant_connection_epoch = 401u64;
    let grant_content_epoch = 402u64;
    let opening = 403u64;
    let output_id = 404u64;
    let output_generation = 405u64;
    let catalog_generation = 406u64;
    let state_revision = 1u64;
    let value = NativeLauncherOpening {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        opening,
        output: oid(output_id, output_generation),
        catalog_generation,
        state_revision,
    };
    let expected = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        opening,
        output_id,
        output_generation,
        catalog_generation,
        state_revision
    );
    (transaction, value, expected)
}

pub fn native_focus() -> (u64, NativeLauncherBinding, BTreeMap<&'static str, i128>) {
    let transaction = 3002u64;
    let value = binding(
        411, 412, 413, 414, 415, 416, 417, 418, 419, 420, 421, 422, 423,
    );
    let mut expected = binding_map!(value, state_revision);
    expected.insert("transaction", transaction as i128);
    (transaction, value, expected)
}

pub fn native_focus_revoked() -> (
    u64,
    NativeLauncherFocusRevoked,
    BTreeMap<&'static str, i128>,
) {
    let transaction = 3003u64;
    let binding_value = binding(
        431, 432, 433, 434, 435, 436, 437, 438, 439, 440, 441, 442, 443,
    );
    let reason = 5u16;
    let value = NativeLauncherFocusRevoked {
        binding: binding_value,
        reason,
    };
    let mut expected = binding_map!(binding_value, state_revision);
    expected.insert("transaction", transaction as i128);
    expected.insert("reason", reason as i128);
    (transaction, value, expected)
}

pub fn native_input() -> (u64, NativeLauncherInput, BTreeMap<&'static str, i128>) {
    let transaction = 3004u64;
    let binding_value = binding(
        451, 452, 453, 454, 455, 456, 457, 458, 459, 460, 461, 462, 463,
    );
    let event_id = 71u64;
    let state_revision = 500u64; // strictly greater than the binding's (462), since kind != Accept
    let event = NativeLauncherEvent {
        binding: binding_value,
        event_id,
        state_revision,
    };
    let issued_mono_usec = 999u64;
    let kind = NativeLauncherInputKind::Text;
    let text = "open terminal".to_owned();
    let value = NativeLauncherInput {
        event,
        issued_mono_usec,
        kind,
        text: text.clone(),
    };
    let mut expected = binding_map!(binding_value, binding_state_revision);
    expected.insert("transaction", transaction as i128);
    expected.insert("event_id", event_id as i128);
    expected.insert("state_revision", state_revision as i128);
    expected.insert("issued_mono_usec", issued_mono_usec as i128);
    expected.insert("kind", kind as i128);
    (transaction, value, expected)
}

pub fn native_activation_outcome() -> (
    u64,
    NativeLauncherActivationOutcome,
    BTreeMap<&'static str, i128>,
) {
    let transaction = 3005u64;
    let binding_value = binding(
        471, 472, 473, 474, 475, 476, 477, 478, 479, 480, 481, 482, 483,
    );
    let event_id = 72u64;
    let state_revision = 482u64; // equal to the binding's own, as `activation()` requires
    let event = NativeLauncherEvent {
        binding: binding_value,
        event_id,
        state_revision,
    };
    let cause = 2u16;
    let slot = 9u16;
    let activation = NativeLauncherActivation { event, cause, slot };
    let status = 1u16;
    let reason = 0u16;
    let value = NativeLauncherActivationOutcome {
        activation,
        status,
        reason,
    };
    let mut expected = binding_map!(binding_value, binding_state_revision);
    expected.insert("transaction", transaction as i128);
    expected.insert("event_id", event_id as i128);
    expected.insert("state_revision", state_revision as i128);
    expected.insert("cause", cause as i128);
    expected.insert("slot", slot as i128);
    expected.insert("status", status as i128);
    expected.insert("reason", reason as i128);
    (transaction, value, expected)
}

pub fn native_closed() -> (u64, NativeLauncherClosed, BTreeMap<&'static str, i128>) {
    let transaction = 3006u64;
    let grant_connection_epoch = 491u64;
    let grant_content_epoch = 492u64;
    let opening = 493u64;
    let reason = 3u16;
    let value = NativeLauncherClosed {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        opening,
        reason,
    };
    let expected = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        opening,
        reason
    );
    (transaction, value, expected)
}

pub fn native_allocation_request() -> (
    u64,
    NativeLauncherAllocationRequest,
    BTreeMap<&'static str, i128>,
) {
    let transaction = 3007u64;
    let grant_connection_epoch = 501u64;
    let grant_content_epoch = 502u64;
    let opening = 503u64;
    let output_id = 504u64;
    let output_generation = 505u64;
    let request_id = 506u64;
    let prior_id = 507u64;
    let prior_generation = 508u64;
    let operation = 2u16;
    let edge = 3u16;
    let desired_width = 640u32;
    let desired_height = 480u32;
    let margin_top = -5i16;
    let margin_right = 6i16;
    let margin_bottom = -7i16;
    let margin_left = 8i16;
    let value = NativeLauncherAllocationRequest {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        opening,
        output: oid(output_id, output_generation),
        request_id,
        prior: aid(prior_id, prior_generation),
        operation,
        edge,
        desired_width,
        desired_height,
        margins: margins_of(margin_top, margin_right, margin_bottom, margin_left),
    };
    let expected = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        opening,
        output_id,
        output_generation,
        request_id,
        prior_id,
        prior_generation,
        operation,
        edge,
        desired_width,
        desired_height,
        margin_top,
        margin_right,
        margin_bottom,
        margin_left
    );
    (transaction, value, expected)
}

pub fn native_input_ack() -> (u64, NativeLauncherInputAck, BTreeMap<&'static str, i128>) {
    let transaction = 3008u64;
    let binding_value = binding(
        511, 512, 513, 514, 515, 516, 517, 518, 519, 520, 521, 522, 523,
    );
    let event_id = 73u64;
    let state_revision = 600u64; // >= the binding's own, as `event()` requires
    let event = NativeLauncherEvent {
        binding: binding_value,
        event_id,
        state_revision,
    };
    let disposition = 1u16;
    let value = NativeLauncherInputAck { event, disposition };
    let mut expected = binding_map!(binding_value, binding_state_revision);
    expected.insert("transaction", transaction as i128);
    expected.insert("event_id", event_id as i128);
    expected.insert("state_revision", state_revision as i128);
    expected.insert("disposition", disposition as i128);
    (transaction, value, expected)
}

pub fn native_activate() -> (u64, NativeLauncherActivation, BTreeMap<&'static str, i128>) {
    let transaction = 3009u64;
    let binding_value = binding(
        531, 532, 533, 534, 535, 536, 537, 538, 539, 540, 541, 542, 543,
    );
    let event_id = 74u64;
    let state_revision = 542u64; // equal to the binding's own, as `activation()` requires
    let event = NativeLauncherEvent {
        binding: binding_value,
        event_id,
        state_revision,
    };
    let cause = 1u16;
    let slot = 17u16;
    let value = NativeLauncherActivation { event, cause, slot };
    let mut expected = binding_map!(binding_value, binding_state_revision);
    expected.insert("transaction", transaction as i128);
    expected.insert("event_id", event_id as i128);
    expected.insert("state_revision", state_revision as i128);
    expected.insert("cause", cause as i128);
    expected.insert("slot", slot as i128);
    (transaction, value, expected)
}

/// One whole native candidate: one panel-shaped surface, one placement and
/// one target, plus the one displayed catalog row the target's `action_id`
/// echoes.
#[allow(clippy::type_complexity)]
pub fn native_candidate() -> (
    u64,
    NativeContentCandidate,
    BTreeMap<&'static str, i128>,
    BTreeMap<&'static str, i128>,
    BTreeMap<&'static str, i128>,
    BTreeMap<&'static str, i128>,
    BTreeMap<&'static str, i128>,
) {
    let transaction = 3010u64;
    let grant_connection_epoch = 551u64;
    let grant_content_epoch = 552u64;
    let candidate_generation = 553u64;
    let output_id = 554u64;
    let output_generation = 555u64;
    let facts_generation = 556u64;
    let pacing_permit = 557u64;
    let interaction_generation = 558u64;
    let opening = 559u64;
    let catalog_generation = 560u64;
    let state_revision = 561u64;
    let selected = 42u16;
    let surface_count = 1u16;
    let placement_count = 1u16;
    let target_count = 1u16;
    let row_count = 1u16;

    let allocation_id = 601u64;
    let allocation_generation = 602u64;
    let scale_generation = 603u64;
    let role = 3u16;
    let edge = 2u16;
    let margin_top = -1i16;
    let margin_right = 2i16;
    let margin_bottom = -3i16;
    let margin_left = 4i16;
    let reservation_extent = 0u32;
    let parent_surface_index = u16::MAX;
    let anchor_x = 0i32;
    let anchor_y = 0i32;
    let anchor_width = 0u32;
    let anchor_height = 0u32;
    let surface = ContentSurface {
        allocation: aid(allocation_id, allocation_generation),
        scale_generation,
        role,
        edge,
        margins: margins_of(margin_top, margin_right, margin_bottom, margin_left),
        reservation_extent,
        parent_surface_index,
        anchor_parent_rect: ContentPixelRect {
            x: anchor_x,
            y: anchor_y,
            width: anchor_width,
            height: anchor_height,
        },
    };
    let surface_expected = fmap!(
        allocation_id,
        allocation_generation,
        scale_generation,
        role,
        edge,
        margin_top,
        margin_right,
        margin_bottom,
        margin_left,
        reservation_extent,
        parent_surface_index,
        anchor_x,
        anchor_y,
        anchor_width,
        anchor_height
    );

    let resource_id = 611u64;
    let resource_generation = 612u64;
    let surface_index = 0u16;
    let destination_x_px = 5i32;
    let destination_y_px = 6i32;
    let placement = ContentPlacement {
        resource: ContentResourceId {
            id: resource_id,
            generation: resource_generation,
        },
        surface_index,
        destination_x_px,
        destination_y_px,
    };
    let placement_expected = fmap!(
        resource_id,
        resource_generation,
        surface_index,
        destination_x_px,
        destination_y_px
    );

    let surface_index = 0u16;
    let action_kind = 2u16;
    let target_id = 621u64;
    let target_generation = 622u64;
    let action_id = 42u64; // echoes the one displayed catalog row's slot
    let bounds_x = 1i32;
    let bounds_y = 2i32;
    let bounds_width = 100u32;
    let bounds_height = 50u32;
    let target = ContentTarget {
        surface_index,
        action_kind,
        target_id,
        target_generation,
        action_id,
        bounds_px: ContentPixelRect {
            x: bounds_x,
            y: bounds_y,
            width: bounds_width,
            height: bounds_height,
        },
    };
    let target_expected = fmap!(
        surface_index,
        action_kind,
        target_id,
        target_generation,
        action_id,
        bounds_x,
        bounds_y,
        bounds_width,
        bounds_height
    );

    let slot = 42u16;
    let row_expected = fmap!(slot);

    let candidate = NativeContentCandidate {
        candidate: ContentCandidate {
            grant: grant(grant_connection_epoch, grant_content_epoch),
            candidate_generation,
            output: oid(output_id, output_generation),
            facts_generation,
            pacing_permit,
            interaction_generation,
            surfaces: vec![surface],
            placements: vec![placement],
            targets: vec![target],
        },
        opening,
        catalog_generation,
        state_revision,
        selected,
        rows: vec![slot],
    };
    let prefix_expected = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        candidate_generation,
        output_id,
        output_generation,
        facts_generation,
        pacing_permit,
        interaction_generation,
        opening,
        catalog_generation,
        state_revision,
        selected,
        surface_count,
        placement_count,
        target_count,
        row_count
    );
    (
        transaction,
        candidate,
        prefix_expected,
        surface_expected,
        placement_expected,
        target_expected,
        row_expected,
    )
}
