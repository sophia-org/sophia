use std::collections::BTreeSet;

use sophia_protocol::{PolicyOverviewWorkspace, PolicySceneSnapshot, validate_wm_overview};

mod projection;
pub use projection::*;
mod capture;
pub use capture::*;
mod authority;
pub use authority::*;

/// Committed spatial preview facts. Shell clients receive only remapped slots;
/// Engine retains these opaque scene identities for image sampling and selection.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PolicyOverviewPublication {
    pub connection_epoch: Option<u64>,
    pub generation: u64,
    pub workspaces: Vec<PolicyOverviewWorkspace>,
}

pub(crate) fn overview_matches_scene(
    workspaces: &[PolicyOverviewWorkspace],
    scene: &PolicySceneSnapshot,
) -> bool {
    if validate_wm_overview(workspaces).is_err() {
        return false;
    }
    let outputs: BTreeSet<_> = scene.outputs.iter().map(|output| output.output).collect();
    let surfaces: BTreeSet<_> = scene
        .surfaces
        .iter()
        .map(|surface| surface.surface)
        .collect();
    workspaces.iter().all(|workspace| {
        outputs.contains(&workspace.output)
            && workspace
                .placements
                .iter()
                .all(|placement| surfaces.contains(&placement.surface))
    })
}
