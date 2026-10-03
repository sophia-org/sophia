//! Clock admission follows layout authority, not the availability of pixels.
use super::*;
use crate::live_session::{LiveWmSession, PersistentLiveLayout};
use sophia_backend_live::{LivePresentClockPlacement, LiveProductionVisualRuntime};
use sophia_protocol::{PolicyOutputProjection, Rect, SurfaceId};

pub(in crate::live_session) fn select_outputs(
    runtime: Option<&LiveProductionVisualRuntime>,
    layout: &PersistentLiveLayout,
    wm: Option<&LiveWmSession>,
    admissions: &[XPresentClockAdmission],
) -> BTreeMap<SurfaceId, Option<OutputId>> {
    let Some(runtime) = runtime else {
        return BTreeMap::new();
    };
    // Only reached for a nonempty admission batch, never on an idle pass.
    let projections = wm
        .and_then(|wm| wm.public.as_ref())
        .map(|p| p.reducer.committed());
    let primary = wm
        .and_then(LiveWmSession::published_output_snapshot)
        .map(|snapshot| snapshot.primary_output);
    select_with_projections(runtime, layout, projections.as_deref(), admissions, primary)
}

pub(super) fn select_with_projections(
    runtime: &LiveProductionVisualRuntime,
    layout: &PersistentLiveLayout,
    projections: Option<&[PolicyOutputProjection]>,
    admissions: &[XPresentClockAdmission],
    primary: Option<OutputId>,
) -> BTreeMap<SurfaceId, Option<OutputId>> {
    runtime
        .present_clock_outputs_for_placements(
            admissions
                .iter()
                .filter_map(|admission| admission.target)
                .map(|(surface, geometry)| {
                    (
                        surface,
                        ordinary_placement(layout, projections, surface, geometry),
                    )
                }),
            primary,
        )
        .into_iter()
        .collect()
}

fn ordinary_placement(
    layout: &PersistentLiveLayout,
    projections: Option<&[PolicyOutputProjection]>,
    surface: SurfaceId,
    fallback: Rect,
) -> Option<LivePresentClockPlacement> {
    // target exists only while X says Viewable. In Direct mode that fact can
    // precede Session's map observation; there is no external hiding policy.
    if layout.engine_owns_initial_placement {
        return Some(layout.layers.get(&surface).map_or(
            LivePresentClockPlacement {
                geometry: fallback,
                output: None,
            },
            |layer| LivePresentClockPlacement {
                geometry: layer.geometry,
                output: layer.output,
            },
        ));
    }
    // With an external WM a managed window becomes Viewable only through
    // Session's AdmitSurface, after its role is known. A Viewable target whose
    // role has not arrived yet is therefore client-positioned.
    if layout.is_client_positioned(surface) || !layout.presentation_roles.contains_key(&surface) {
        let visible = popup_visible(layout, projections, surface);
        return visible.then_some(LivePresentClockPlacement {
            geometry: fallback,
            output: None,
        });
    }
    managed_placement(layout, projections, surface)
}

fn popup_visible(
    layout: &PersistentLiveLayout,
    projections: Option<&[PolicyOutputProjection]>,
    surface: SurfaceId,
) -> bool {
    let mut current = surface;
    for _ in 0..=layout.presentation_owners.len() {
        // X's target snapshot supplies this popup's current mapped state even
        // before Session consumes its map/remap batch. Ancestors still require
        // their own mapping and policy visibility. Unknown ancestry can only
        // occur before the corresponding authority observation is consumed.
        if current != surface && layout.presentation_roles.contains_key(&current) {
            if !layout.mapped_surfaces.contains(&current) {
                return false;
            }
            if !layout.is_client_positioned(current) {
                return managed_placement(layout, projections, current).is_some();
            }
        }
        match layout.presentation_owners.get(&current) {
            Some(owner) => current = *owner,
            None => return true,
        }
    }
    false
}

fn managed_placement(
    layout: &PersistentLiveLayout,
    projections: Option<&[PolicyOutputProjection]>,
    surface: SurfaceId,
) -> Option<LivePresentClockPlacement> {
    // AdmitSurface runs at staging, before a sibling resize or state ack can
    // release the epoch. Only admissions owned by this epoch use its layers;
    // already-managed windows retain their committed placement until commit.
    let pending = layout.pending.as_ref().filter(|pending| {
        pending.admission_surfaces.contains(&surface)
            && !matches!(
                layout.admissions.state(surface),
                sophia_engine::SurfacePresentationAdmissionState::Inactive
                    | sophia_engine::SurfacePresentationAdmissionState::Managed
            )
    });
    if let Some(pending) = pending {
        if pending
            .presentation_states
            .get(&surface)
            .is_some_and(|state| state.minimized)
        {
            return None;
        }
        return pending
            .layers
            .iter()
            .find(|layer| layer.surface == surface)
            .and_then(|layer| {
                layer.output.map(|output| LivePresentClockPlacement {
                    geometry: layer.geometry,
                    output: Some(output),
                })
            });
    }
    // Reducer rectangles include chrome. Session layers are reconciled content
    // coordinates and exist when their projection commits, even without pixels.
    let layer = layout.layers.get(&surface)?;
    let output = layer.output?;
    projections?
        .iter()
        .any(|projection| {
            projection.output == output
                && projection.placements.iter().any(|placement| {
                    placement.surface == surface && !placement.presentation.minimized
                })
        })
        .then_some(LivePresentClockPlacement {
            geometry: layer.geometry,
            output: layer.output,
        })
}
