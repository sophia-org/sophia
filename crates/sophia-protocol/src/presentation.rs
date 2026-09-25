//! Stateless shape checks shared by independent transfer and Engine admission.
//! Scene membership, negotiated authority and generation history belong to Engine.
use std::collections::{BTreeMap, BTreeSet};

use crate::*;

fn valid_rect(r: Rect) -> bool {
    r.width > 0
        && r.height > 0
        && r.x.checked_add(r.width).is_some()
        && r.y.checked_add(r.height).is_some()
}

fn contains(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && i64::from(inner.x) + i64::from(inner.width)
            <= i64::from(outer.x) + i64::from(outer.width)
        && i64::from(inner.y) + i64::from(inner.height)
            <= i64::from(outer.y) + i64::from(outer.height)
}

fn overlaps(a: Rect, b: Rect) -> bool {
    i64::from(a.x) < i64::from(b.x) + i64::from(b.width)
        && i64::from(b.x) < i64::from(a.x) + i64::from(a.width)
        && i64::from(a.y) < i64::from(b.y) + i64::from(b.height)
        && i64::from(b.y) < i64::from(a.y) + i64::from(a.height)
}

pub fn validate_policy_presentation_shape(p: &PolicyPresentation) -> Result<(), &'static str> {
    if p.generation == 0
        || p.outputs.is_empty()
        || p.outputs.len() > POLICY_MAX_PRESENTATION_OUTPUTS
        || p.instances.len() > POLICY_MAX_SURFACE_INSTANCES
        || p.regions.len() > POLICY_MAX_PRESENTATION_REGIONS
        || p.bindings.len() > POLICY_MAX_PRESENTATION_BINDINGS
    {
        return Err("invalid presentation identity or count");
    }
    let mut outputs = BTreeMap::new();
    for o in &p.outputs {
        if !o.output.is_valid()
            || o.generation == 0
            || !valid_rect(o.coverage)
            || outputs.insert(o.output, o).is_some()
        {
            return Err("invalid presentation output");
        }
    }
    let mut ids = BTreeSet::new();
    let mut orders = BTreeSet::new();
    for (id, generation, output, geometry, clip, z_index, action) in p
        .instances
        .iter()
        .map(|i| {
            (
                i.id,
                i.generation,
                i.output,
                i.destination,
                i.clip,
                i.z_index,
                i.action,
            )
        })
        .chain(p.regions.iter().map(|r| {
            (
                r.id,
                r.generation,
                r.output,
                r.geometry,
                r.clip,
                r.z_index,
                r.action,
            )
        }))
    {
        let coverage = outputs
            .get(&output)
            .ok_or("unknown presentation output")?
            .coverage;
        if id == 0
            || generation == 0
            || !ids.insert(id)
            || !orders.insert((output, z_index))
            || !valid_rect(geometry)
            || !valid_rect(clip)
            || !contains(coverage, clip)
            || !overlaps(geometry, clip)
            || action.is_some_and(|a| !a.is_valid())
        {
            return Err("invalid presentation target");
        }
    }
    for i in &p.instances {
        if !i.source.is_valid() || !(1..=1000).contains(&i.opacity_millis) {
            return Err("invalid presentation source or opacity");
        }
    }
    for o in &p.outputs {
        if o.mode == PolicyPresentationMode::ReplaceApplications
            && !p.regions.iter().any(|r| {
                r.output == o.output
                    && r.role == PolicyPresentationRegionRole::Backdrop
                    && r.geometry == o.coverage
                    && r.clip == o.coverage
                    && r.z_index == 0
                    && r.action.is_none()
            })
        {
            return Err("replacement presentation needs a full backdrop");
        }
    }
    if let Some(output) = p.keyboard_output {
        if !outputs.contains_key(&output)
            || p.bindings.is_empty()
            || p.outputs
                .iter()
                .any(|o| o.mode != PolicyPresentationMode::ReplaceApplications)
        {
            return Err("invalid modal presentation");
        }
    } else if !p.bindings.is_empty() {
        return Err("bindings require a modal presentation");
    }
    let mut chords = BTreeSet::new();
    for b in &p.bindings {
        if !b.action.is_valid()
            || b.keycode == 0
            || b.keycode > 0x2ff
            || b.modifiers.bits & !WmModifierMask::SUPPORTED != 0
            || !chords.insert((b.keycode, b.modifiers.bits))
            || b.keycode == 14
                && b.modifiers.bits & (WmModifierMask::CONTROL | WmModifierMask::ALT)
                    == WmModifierMask::CONTROL | WmModifierMask::ALT
        {
            return Err("invalid or reserved presentation binding");
        }
    }
    Ok(())
}
/// Validates the identity shape only. The session's presented owner must still
/// match the connection, publication and completed output epoch.
pub fn valid_policy_presentation_identity(identity: crate::PolicyPresentationIdentity) -> bool {
    identity.publication_generation != 0
        && identity.output.is_valid()
        && identity.output_generation != 0
        && identity.presentation_epoch != 0
        && (identity.target_id == 0) == (identity.target_generation == 0)
}

pub fn validate_policy_presentation_actions(
    presentation: &crate::PolicyPresentation,
    actions: &[crate::PolicyActionRegistration],
) -> Result<(), &'static str> {
    let registered = actions
        .iter()
        .filter(|a| a.session_operation_slot.is_none())
        .map(|a| a.action)
        .collect::<std::collections::BTreeSet<_>>();
    if presentation
        .instances
        .iter()
        .filter_map(|i| i.action)
        .chain(presentation.regions.iter().filter_map(|r| r.action))
        .chain(presentation.bindings.iter().map(|b| b.action))
        .all(|action| registered.contains(&action))
    {
        Ok(())
    } else {
        Err("presentation action is not a registered pure policy action")
    }
}
