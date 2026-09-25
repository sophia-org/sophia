use crate::{
    CompositorDisplayCommand as Command, CompositorNodeId, CompositorRect, CompositorRgb8,
    CompositorSurfacePreview, DescriptorOverlayNodeRole as Role, DescriptorOverlayProjection,
    PolicyOverviewPublication,
};
use sophia_protocol::{
    OutputId, Rect, ShellOverviewCandidate, ShellOverviewCatalog, validate_shell_overview_catalog,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverviewHitTarget {
    pub workspace: u16,
    pub window: u16,
    pub geometry: Rect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverviewProjection {
    pub overlay: DescriptorOverlayProjection,
    pub targets: Vec<OverviewHitTarget>,
}

/// Catalog slots map by position to the paired, immutable WM publication.
/// Only Engine creates geometry and resolves the corresponding surface image.
pub fn overview_projection(
    publication: &PolicyOverviewPublication,
    catalog: &ShellOverviewCatalog,
    candidate: &ShellOverviewCandidate,
    output: OutputId,
    projection: u64,
    bounds: Rect,
) -> Result<OverviewProjection, &'static str> {
    validate_shell_overview_catalog(catalog).map_err(|_| "invalid overview catalog")?;
    if projection == 0
        || bounds.width < 16
        || bounds.height < 16
        || !candidate.visible
        || candidate.activate
        || candidate.connection_epoch != catalog.connection_epoch
        || candidate.catalog_generation != catalog.generation
        || candidate.candidate_generation == 0
        || publication.workspaces.len() != catalog.workspaces.len()
    {
        return Err("stale overview projection");
    }
    let selected = catalog
        .workspaces
        .iter()
        .position(|row| row.slot == candidate.workspace)
        .ok_or("unknown overview workspace")?;
    if catalog.workspaces[selected].output != output
        || candidate.window != 0
            && !catalog.workspaces[selected]
                .windows
                .contains(&candidate.window)
    {
        return Err("overview selection is outside its output");
    }
    for (wm, shell) in publication.workspaces.iter().zip(&catalog.workspaces) {
        if wm.output != shell.output || wm.placements.len() != shell.windows.len() {
            return Err("overview catalog mapping changed");
        }
    }
    let node = |slot, role| CompositorNodeId::DescriptorOverlay {
        projection,
        slot,
        role,
    };
    let rect = |slot, role, geometry, color| {
        Command::Rect(CompositorRect {
            node: node(slot, role),
            generation: candidate.candidate_generation,
            geometry,
            color,
            opacity: 255,
        })
    };
    let background = CompositorRgb8 {
        red: 18,
        green: 21,
        blue: 27,
    };
    let workspace_color = CompositorRgb8 {
        red: 35,
        green: 39,
        blue: 48,
    };
    let selected_color = CompositorRgb8 {
        red: 105,
        green: 172,
        blue: 235,
    };
    let mut commands = vec![rect(u16::MAX, Role::Panel, bounds, background)];
    let mut targets = Vec::new();
    let rows: Vec<_> = catalog
        .workspaces
        .iter()
        .enumerate()
        .filter(|(_, row)| row.output == output)
        .collect();
    let chosen_row = rows
        .iter()
        .position(|(index, _)| *index == selected)
        .unwrap();
    let row_height = (bounds.height / 2).max(1);
    let gap = (bounds.height / 32).clamp(2, 32);
    // At most three strips are visible. Large catalogs never grow a frame beyond
    // the compositor command bound; off-strip windows are clipped before emission.
    for row_index in chosen_row.saturating_sub(1)..=(chosen_row + 1).min(rows.len() - 1) {
        let (index, row) = rows[row_index];
        let workspace = &publication.workspaces[index];
        if workspace.bounds.is_empty() {
            return Err("invalid overview workspace bounds");
        }
        let scale = (i64::from(row_height - gap) * 1000 / i64::from(workspace.bounds.height))
            .min(i64::from(bounds.width - gap * 2) * 1000 / i64::from(workspace.bounds.width))
            .clamp(1, 600);
        let scaled = |value: i64| value * scale / 1000;
        let top = i64::from(bounds.y)
            + i64::from(bounds.height - row_height) / 2
            + (row_index as i64 - chosen_row as i64) * i64::from(row_height + gap);
        let strip = clip(
            Rect {
                x: bounds.x,
                y: top as i32,
                width: bounds.width,
                height: row_height,
            },
            bounds,
        );
        if strip.is_empty() {
            continue;
        }
        commands.push(rect(row.slot, Role::Panel, strip, workspace_color));
        targets.push(OverviewHitTarget {
            workspace: row.slot,
            window: 0,
            geometry: strip,
        });
        let focus_index = if index == selected {
            row.windows
                .iter()
                .position(|slot| *slot == candidate.window)
        } else {
            row.windows.iter().position(|slot| *slot == row.focused)
        };
        let center = focus_index
            .map(|i| workspace.placements[i].geometry)
            .unwrap_or(workspace.bounds);
        let camera = i64::from(center.x) + i64::from(center.width) / 2;
        for (placement, slot) in workspace.placements.iter().zip(&row.windows) {
            let source = placement.geometry;
            let geometry = Rect {
                x: i32::try_from(
                    i64::from(bounds.x)
                        + i64::from(bounds.width) / 2
                        + scaled(i64::from(source.x) - camera),
                )
                .map_err(|_| "preview x overflow")?,
                y: i32::try_from(top + scaled(i64::from(source.y) - i64::from(workspace.bounds.y)))
                    .map_err(|_| "preview y overflow")?,
                width: scaled(i64::from(source.width)).max(1) as i32,
                height: scaled(i64::from(source.height)).max(1) as i32,
            };
            let visible = clip(geometry, strip);
            if visible.is_empty() {
                continue;
            }
            if index == selected && *slot == candidate.window {
                let ring = clip(
                    Rect {
                        x: geometry.x.saturating_sub(2),
                        y: geometry.y.saturating_sub(2),
                        width: geometry.width.saturating_add(4),
                        height: geometry.height.saturating_add(4),
                    },
                    strip,
                );
                commands.push(rect(*slot, Role::Selection, ring, selected_color));
            }
            commands.push(Command::SurfacePreview(CompositorSurfacePreview {
                node: node(*slot, Role::Row),
                generation: candidate.candidate_generation,
                surface: placement.surface,
                geometry,
                clip: visible,
            }));
            targets.push(OverviewHitTarget {
                workspace: row.slot,
                window: *slot,
                geometry: visible,
            });
        }
    }
    if commands.len() > crate::MAX_COMPOSITOR_DISPLAY_COMMANDS {
        return Err("overview command capacity exceeded");
    }
    Ok(OverviewProjection {
        overlay: DescriptorOverlayProjection {
            output,
            generation: candidate.candidate_generation,
            geometry: bounds,
            commands,
            targets: Vec::new(),
        },
        targets,
    })
}

fn clip(rect: Rect, bounds: Rect) -> Rect {
    let x = rect.x.max(bounds.x);
    let y = rect.y.max(bounds.y);
    Rect {
        x,
        y,
        width: rect
            .x
            .saturating_add(rect.width)
            .min(bounds.x.saturating_add(bounds.width))
            .saturating_sub(x)
            .max(0),
        height: rect
            .y
            .saturating_add(rect.height)
            .min(bounds.y.saturating_add(bounds.height))
            .saturating_sub(y)
            .max(0),
    }
}
