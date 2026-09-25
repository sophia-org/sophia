use super::PolicyProjectionReducer;
use sophia_protocol::{PolicyPresentation, PolicyPresentationMode, PolicySceneSnapshot};

fn matches_scene(p: &PolicyPresentation, scene: &PolicySceneSnapshot) -> bool {
    let outputs = scene
        .outputs
        .iter()
        .map(|o| (o.output, o))
        .collect::<std::collections::BTreeMap<_, _>>();
    let surfaces = scene
        .surfaces
        .iter()
        .map(|s| s.surface)
        .collect::<std::collections::BTreeSet<_>>();
    p.outputs.iter().all(|o| {
        outputs.get(&o.output).is_some_and(|current| {
            o.generation == current.generation
                && super::validation::rect_contains(current.bounds, o.coverage)
                && (o.mode != PolicyPresentationMode::ReplaceApplications
                    || o.coverage == current.bounds)
        })
    }) && p.instances.iter().all(|i| surfaces.contains(&i.source))
}

impl PolicyProjectionReducer {
    pub fn presentation_publication(&self) -> Option<(u64, &PolicyPresentation)> {
        Some((self.active_epoch?, self.presentation.as_ref()?))
    }

    /// The reducer checks publication membership, not presentation completion.
    /// Only the session's presented-input owner can originate this cause.
    pub fn presentation_cause_is_current(
        &self,
        cause: sophia_protocol::PolicyRequestCause,
    ) -> bool {
        let sophia_protocol::PolicyRequestCause::PresentationAction {
            action, identity, ..
        } = cause
        else {
            return true;
        };
        let Some(p) = &self.presentation else {
            return false;
        };
        if p.generation != identity.publication_generation
            || !p
                .outputs
                .iter()
                .any(|o| o.output == identity.output && o.generation == identity.output_generation)
        {
            return false;
        }
        if identity.target_id == 0 {
            p.keyboard_output == Some(identity.output)
                && p.bindings.iter().any(|b| b.action == action)
        } else {
            p.instances.iter().any(|i| {
                i.id == identity.target_id
                    && i.generation == identity.target_generation
                    && i.output == identity.output
                    && i.action == Some(action)
            }) || p.regions.iter().any(|r| {
                r.id == identity.target_id
                    && r.generation == identity.target_generation
                    && r.output == identity.output
                    && r.action == Some(action)
            })
        }
    }

    /// Local revocation cannot depend on transport queue capacity. Advancing
    /// the commit serial also invalidates any already-staged late successor.
    pub fn revoke_presentation(&mut self) {
        if self.presentation.take().is_some() {
            self.commit_serial = self.commit_serial.saturating_add(1);
        }
    }

    pub(super) fn validate_presentation(
        &self,
        next: Option<&PolicyPresentation>,
    ) -> Result<(), &'static str> {
        let Some(next) = next else {
            return Ok(());
        };
        sophia_protocol::validate_policy_presentation_shape(next)?;
        if !matches_scene(next, &self.scene) {
            return Err("presentation escaped current scene");
        }
        if self.presentation.as_ref() == Some(next) {
            return Ok(());
        }
        if next.generation <= self.greatest_presentation_generation {
            return Err("stale presentation generation");
        }
        let previous_instances = self
            .presentation
            .iter()
            .flat_map(|p| &p.instances)
            .map(|i| (i.id, i))
            .collect::<std::collections::BTreeMap<_, _>>();
        let previous_regions = self
            .presentation
            .iter()
            .flat_map(|p| &p.regions)
            .map(|r| (r.id, r))
            .collect::<std::collections::BTreeMap<_, _>>();
        for instance in &next.instances {
            if let Some(old) = previous_instances.get(&instance.id) {
                if *old != instance && instance.generation <= old.generation {
                    return Err("instance changed without a fresh generation");
                }
            } else if instance.id <= self.greatest_presentation_target {
                return Err("retired presentation target reused");
            }
        }
        for region in &next.regions {
            if let Some(old) = previous_regions.get(&region.id) {
                if *old != region && region.generation <= old.generation {
                    return Err("region changed without a fresh generation");
                }
            } else if region.id <= self.greatest_presentation_target {
                return Err("retired presentation target reused");
            }
        }
        Ok(())
    }

    pub(super) fn commit_presentation(&mut self, presentation: Option<PolicyPresentation>) {
        if let Some(p) = &presentation {
            self.greatest_presentation_generation = p.generation;
            self.greatest_presentation_target = self.greatest_presentation_target.max(
                p.instances
                    .iter()
                    .map(|i| i.id)
                    .chain(p.regions.iter().map(|r| r.id))
                    .max()
                    .unwrap_or(0),
            );
        }
        self.presentation = presentation;
    }

    pub(super) fn revalidate_presentation_scene(&mut self, scene: &PolicySceneSnapshot) {
        let topology_changed = self.scene.outputs.len() != scene.outputs.len()
            || self.scene.outputs.iter().any(|old| {
                !scene.outputs.iter().any(|new| {
                    old.output == new.output
                        && old.generation == new.generation
                        && old.bounds == new.bounds
                        && old.work_area == new.work_area
                })
            });
        if topology_changed
            || self
                .presentation
                .as_ref()
                .is_some_and(|p| !matches_scene(p, scene))
        {
            self.revoke_presentation();
        }
    }
}
