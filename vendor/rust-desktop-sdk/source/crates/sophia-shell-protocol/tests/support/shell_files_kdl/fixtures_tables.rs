//! The table-shaped kinds: `Outputs` (a prefix plus `ContentOutputFactsEntry`
//! rows) and `Candidate` (a prefix plus `ContentSurface`/`ContentPlacement`/
//! `ContentTarget` rows). Grouped together, and apart from
//! [`super::fixtures`]'s flat records, because both share the same shape:
//! build each row's typed value and its field-value table together, then a
//! prefix whose counts are the row vectors' lengths.

use super::{aid, fmap, grant, margins_of, oid, prect, rid};
use sophia_shell_protocol::*;
use std::collections::BTreeMap;

fn output_row(
    output_id: u64,
    output_generation: u64,
    local_width: u32,
    local_height: u32,
    scale_numerator: u32,
    scale_denominator: u32,
    scale_generation: u64,
) -> (ContentOutputFactsEntry, BTreeMap<&'static str, i128>) {
    let entry = ContentOutputFactsEntry {
        output: oid(output_id, output_generation),
        local_width,
        local_height,
        scale_numerator,
        scale_denominator,
        scale_generation,
    };
    let expected = fmap!(
        output_id,
        output_generation,
        local_width,
        local_height,
        scale_numerator,
        scale_denominator,
        scale_generation
    );
    (entry, expected)
}

/// Three distinct output rows, the whole `ContentOutputFacts` record they
/// belong to, its transaction, the prefix's field-value table and each row's
/// own table in row order.
#[allow(clippy::type_complexity)]
pub fn outputs() -> (
    u64,
    ContentOutputFacts,
    BTreeMap<&'static str, i128>,
    Vec<BTreeMap<&'static str, i128>>,
) {
    let (row0, expected0) = output_row(101, 1, 800, 600, 3, 2, 5);
    let (row1, expected1) = output_row(102, 2, 1024, 768, 5, 3, 6);
    let (row2, expected2) = output_row(103, 3, 1920, 1080, 7, 4, 7);
    let outputs = vec![row0, row1, row2];
    let rows_expected = vec![expected0, expected1, expected2];

    let transaction = 99u64;
    let grant_connection_epoch = 31u64;
    let grant_content_epoch = 32u64;
    let facts_generation = 44u64;
    let output_count = outputs.len() as u64;
    let record = ContentOutputFacts {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        facts_generation,
        outputs,
    };
    let expected_prefix = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        facts_generation,
        output_count
    );
    (transaction, record, expected_prefix, rows_expected)
}

#[allow(clippy::too_many_arguments)]
fn surface_row(
    allocation_id: u64,
    allocation_generation: u64,
    scale_generation: u64,
    role: u16,
    edge: u16,
    margin_top: i16,
    margin_right: i16,
    margin_bottom: i16,
    margin_left: i16,
    reservation_extent: u32,
    parent_surface_index: u16,
    anchor_x: i32,
    anchor_y: i32,
    anchor_width: u32,
    anchor_height: u32,
) -> (ContentSurface, BTreeMap<&'static str, i128>) {
    let row = ContentSurface {
        allocation: aid(allocation_id, allocation_generation),
        scale_generation,
        role,
        edge,
        margins: margins_of(margin_top, margin_right, margin_bottom, margin_left),
        reservation_extent,
        parent_surface_index,
        anchor_parent_rect: prect(anchor_x, anchor_y, anchor_width, anchor_height),
    };
    let expected = fmap!(
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
    (row, expected)
}

fn placement_row(
    resource_id: u64,
    resource_generation: u64,
    surface_index: u16,
    destination_x_px: i32,
    destination_y_px: i32,
) -> (ContentPlacement, BTreeMap<&'static str, i128>) {
    let row = ContentPlacement {
        resource: rid(resource_id, resource_generation),
        surface_index,
        destination_x_px,
        destination_y_px,
    };
    let expected = fmap!(
        resource_id,
        resource_generation,
        surface_index,
        destination_x_px,
        destination_y_px
    );
    (row, expected)
}

#[allow(clippy::too_many_arguments)]
fn target_row(
    surface_index: u16,
    action_kind: u16,
    target_id: u64,
    target_generation: u64,
    action_id: u64,
    bounds_x: i32,
    bounds_y: i32,
    bounds_width: u32,
    bounds_height: u32,
) -> (ContentTarget, BTreeMap<&'static str, i128>) {
    let row = ContentTarget {
        surface_index,
        action_kind,
        target_id,
        target_generation,
        action_id,
        bounds_px: prect(bounds_x, bounds_y, bounds_width, bounds_height),
    };
    let expected = fmap!(
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
    (row, expected)
}

/// One whole candidate: two surfaces (a panel and a popout anchored to it),
/// two placements and two targets, its transaction, the prefix's field-value
/// table and each table's row-by-row field-value tables.
#[allow(clippy::type_complexity)]
pub fn candidate() -> (
    u64,
    ContentCandidate,
    BTreeMap<&'static str, i128>,
    Vec<BTreeMap<&'static str, i128>>,
    Vec<BTreeMap<&'static str, i128>>,
    Vec<BTreeMap<&'static str, i128>>,
) {
    // Surface 0: a panel (role=1), no anchor or reservation parent.
    let (surface0, surface0_expected) =
        surface_row(301, 302, 91, 1, 2, -5, 6, -7, 8, 100, u16::MAX, 0, 0, 0, 0);
    // Surface 1: a popout (role=2) anchored to surface 0.
    let (surface1, surface1_expected) =
        surface_row(303, 304, 92, 2, 3, -9, 10, -11, 12, 0, 0, 1, 2, 50, 60);
    let surfaces = vec![surface0, surface1];
    let surfaces_expected = vec![surface0_expected, surface1_expected];

    let (placement0, placement0_expected) = placement_row(401, 402, 0, 10, 11);
    let (placement1, placement1_expected) = placement_row(403, 404, 1, 12, 13);
    let placements = vec![placement0, placement1];
    let placements_expected = vec![placement0_expected, placement1_expected];

    let (target0, target0_expected) = target_row(0, 1, 501, 502, 503, 1, 2, 20, 21);
    let (target1, target1_expected) = target_row(1, 1, 504, 505, 506, 3, 4, 22, 23);
    let targets = vec![target0, target1];
    let targets_expected = vec![target0_expected, target1_expected];

    let transaction = 919u64;
    let grant_connection_epoch = 211u64;
    let grant_content_epoch = 212u64;
    let candidate_generation = 81u64;
    let output_id = 231u64;
    let output_generation = 232u64;
    let facts_generation = 82u64;
    let pacing_permit = 83u64;
    let interaction_generation = 84u64;
    let surface_count = surfaces.len() as u64;
    let placement_count = placements.len() as u64;
    let target_count = targets.len() as u64;

    let candidate = ContentCandidate {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        candidate_generation,
        output: oid(output_id, output_generation),
        facts_generation,
        pacing_permit,
        interaction_generation,
        surfaces,
        placements,
        targets,
    };
    let expected_prefix = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        candidate_generation,
        output_id,
        output_generation,
        facts_generation,
        pacing_permit,
        interaction_generation,
        surface_count,
        placement_count,
        target_count
    );
    (
        transaction,
        candidate,
        expected_prefix,
        surfaces_expected,
        placements_expected,
        targets_expected,
    )
}
