//! Distinctly-valued typed fixtures for the t252 B5 persistent catalog (r8)
//! kinds: the whole `Catalog` object (with r8 identities), the catalog
//! activation outcome/activate records and the whole `CatalogCandidate`.

use super::{aid, fmap, grant, oid};
use sophia_shell_protocol::*;
use std::collections::BTreeMap;

/// The whole `Catalog` object with r8 identities: two entries, the prefix's
/// field-value table and each entry row's own table in row order.
#[allow(clippy::type_complexity)]
pub fn catalog() -> (
    u64,
    ShellPersistentCatalog,
    BTreeMap<&'static str, i128>,
    Vec<BTreeMap<&'static str, i128>>,
) {
    let transaction = 4001u64;
    let connection_epoch = 71u64;
    let generation = 72u64;
    let entry_count = 2u64;
    let identities_present = 1u64;

    let entries = vec![
        ShellApplicationDescriptor {
            slot: 1,
            available: true,
            label: "Terminal".to_owned(),
            keywords: "shell console".to_owned(),
        },
        ShellApplicationDescriptor {
            slot: 2,
            available: false,
            label: "Editor".to_owned(),
            keywords: "text code".to_owned(),
        },
    ];
    let identities = std::collections::BTreeMap::from([
        (1u16, "registered:terminal".to_owned()),
        (2u16, "desktop:editor.desktop".to_owned()),
    ]);
    // `label`, `keywords` and `identity` are `type="text"` fields: `verify_block`
    // skips them (see `catalog_row_text` and `verify_text_field`), so only
    // the two numeric fields go in this row's expected map.
    let rows_expected = entries
        .iter()
        .map(|entry| {
            let slot = entry.slot;
            let available = u16::from(entry.available);
            fmap!(slot, available)
        })
        .collect::<Vec<_>>();

    let catalog = ShellPersistentCatalog {
        catalog: ShellApplicationCatalog {
            connection_epoch,
            generation,
            entries,
        },
        identities,
    };
    let expected_prefix = fmap!(
        transaction,
        connection_epoch,
        generation,
        entry_count,
        identities_present
    );
    (transaction, catalog, expected_prefix, rows_expected)
}

/// The exact text this fixture's catalog carries per row, in row order: used
/// by [`super::checks::verify_text_field`] alongside the numeric
/// `rows_expected` map `catalog` returns (which only carries each text
/// field's length, since `verify_block` cannot check strings).
pub fn catalog_row_text() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("Terminal", "shell console", "registered:terminal"),
        ("Editor", "text code", "desktop:editor.desktop"),
    ]
}

#[allow(clippy::too_many_arguments)]
fn action(
    grant_connection_epoch: u64,
    grant_content_epoch: u64,
    output_id: u64,
    output_generation: u64,
    candidate_generation: u64,
    presentation_epoch: u64,
    interaction_generation: u64,
    allocation_id: u64,
    allocation_generation: u64,
    target_id: u64,
    target_generation: u64,
    action_id: u64,
    event_id: u64,
) -> ContentAction {
    ContentAction {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        output: oid(output_id, output_generation),
        candidate_generation,
        presentation_epoch,
        interaction_generation,
        allocation: aid(allocation_id, allocation_generation),
        target_id,
        target_generation,
        action_id,
        event_id,
        kind: 1,
        reason: 0,
    }
}

macro_rules! action_map {
    ($a:expr) => {{
        let grant_connection_epoch = $a.grant.connection_epoch;
        let grant_content_epoch = $a.grant.content_grant_epoch;
        let output_id = $a.output.id;
        let output_generation = $a.output.generation;
        let candidate_generation = $a.candidate_generation;
        let presentation_epoch = $a.presentation_epoch;
        let interaction_generation = $a.interaction_generation;
        let allocation_id = $a.allocation.id;
        let allocation_generation = $a.allocation.generation;
        let target_id = $a.target_id;
        let target_generation = $a.target_generation;
        let action_id = $a.action_id;
        let event_id = $a.event_id;
        let kind = $a.kind;
        let content_reason = $a.reason;
        fmap!(
            grant_connection_epoch,
            grant_content_epoch,
            output_id,
            output_generation,
            candidate_generation,
            presentation_epoch,
            interaction_generation,
            allocation_id,
            allocation_generation,
            target_id,
            target_generation,
            action_id,
            event_id,
            kind,
            content_reason
        )
    }};
}

pub fn catalog_activation_outcome() -> (u64, CatalogActivationOutcome, BTreeMap<&'static str, i128>)
{
    let transaction = 4002u64;
    let content_action = action(81, 82, 831, 832, 84, 85, 86, 87, 88, 89, 90, 91, 92);
    let mut expected = action_map!(content_action);
    let catalog_generation = 93u64;
    let activation = CatalogActivation {
        action: content_action,
        catalog_generation,
    };
    let status = 1u16;
    let reason = 0u16;
    let value = CatalogActivationOutcome {
        activation,
        status,
        reason,
    };
    expected.insert("transaction", transaction as i128);
    expected.insert("catalog_generation", catalog_generation as i128);
    expected.insert("status", status as i128);
    expected.insert("reason", reason as i128);
    (transaction, value, expected)
}

pub fn catalog_activate() -> (u64, CatalogActivation, BTreeMap<&'static str, i128>) {
    let transaction = 4003u64;
    let content_action = action(
        101, 102, 1031, 1032, 104, 105, 106, 107, 108, 109, 110, 111, 112,
    );
    let mut expected = action_map!(content_action);
    let catalog_generation = 113u64;
    let value = CatalogActivation {
        action: content_action,
        catalog_generation,
    };
    expected.insert("transaction", transaction as i128);
    expected.insert("catalog_generation", catalog_generation as i128);
    (transaction, value, expected)
}

/// One whole catalog candidate: one panel surface (role=1), one placement
/// and one target (action_kind=3), its transaction, the prefix's
/// field-value table and each table's row-by-row field-value table.
#[allow(clippy::type_complexity)]
pub fn catalog_candidate() -> (
    u64,
    CatalogContentCandidate,
    BTreeMap<&'static str, i128>,
    Vec<BTreeMap<&'static str, i128>>,
    Vec<BTreeMap<&'static str, i128>>,
    Vec<BTreeMap<&'static str, i128>>,
) {
    let transaction = 4004u64;
    let grant_connection_epoch = 121u64;
    let grant_content_epoch = 122u64;
    let candidate_generation = 123u64;
    let output_id = 1241u64;
    let output_generation = 1242u64;
    let facts_generation = 125u64;
    let pacing_permit = 126u64;
    let interaction_generation = 127u64;
    let catalog_generation = 128u64;
    let surface_count = 1u16;
    let placement_count = 1u16;
    let target_count = 1u16;

    let allocation_id = 141u64;
    let allocation_generation = 142u64;
    let scale_generation = 143u64;
    let role = 1u16;
    let edge = 1u16;
    let margin_top = 0i16;
    let margin_right = 0i16;
    let margin_bottom = 0i16;
    let margin_left = 0i16;
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
        margins: ContentMargins {
            top: margin_top,
            right: margin_right,
            bottom: margin_bottom,
            left: margin_left,
        },
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

    let resource_id = 151u64;
    let resource_generation = 152u64;
    let surface_index = 0u16;
    let destination_x_px = 3i32;
    let destination_y_px = 4i32;
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
    let action_kind = 3u16;
    let target_id = 161u64;
    let target_generation = 162u64;
    let action_id = 4096u64;
    let bounds_x = 1i32;
    let bounds_y = 2i32;
    let bounds_width = 30u32;
    let bounds_height = 40u32;
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

    let candidate = CatalogContentCandidate {
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
        catalog_generation,
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
        catalog_generation,
        surface_count,
        placement_count,
        target_count
    );
    (
        transaction,
        candidate,
        expected_prefix,
        vec![surface_expected],
        vec![placement_expected],
        vec![target_expected],
    )
}
