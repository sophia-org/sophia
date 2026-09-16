use super::*;

#[derive(Debug)]
pub struct LiveProductionRendererImageHandoff {
    expected: Vec<sophia_renderer_live::LiveRendererImageId>,
    heads: Vec<LiveProductionRendererImageHeadHandoff>,
}

#[derive(Debug)]
struct LiveProductionRendererImageHeadHandoff {
    output: OutputId,
    card_index: usize,
    connector_id: u32,
    snapshots: Vec<sophia_renderer_live::LiveRendererImageSnapshot>,
}

impl LiveProductionRendererImageHandoff {
    pub const fn len(&self) -> usize {
        self.expected.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.expected.is_empty()
    }

    pub fn image_ids(&self) -> &[sophia_renderer_live::LiveRendererImageId] {
        &self.expected
    }
}

fn validate_renderer_image_handoff_ids(
    expected: &[sophia_renderer_live::LiveRendererImageId],
    actual: &[sophia_renderer_live::LiveRendererImageId],
) -> Result<(), &'static str> {
    match crate::reduce_live_renderer_image_handoff_admission(expected, Some(actual)) {
        crate::LiveRendererImageHandoffAdmission::Ready => Ok(()),
        crate::LiveRendererImageHandoffAdmission::InvalidIdentity => {
            Err("renderer-image handoff contains an invalid image identity")
        }
        crate::LiveRendererImageHandoffAdmission::DuplicateIdentity => {
            Err("renderer-image handoff contains a duplicate image identity")
        }
        crate::LiveRendererImageHandoffAdmission::CoverageMismatch => {
            Err("renderer-image handoff does not cover the retained scene")
        }
        crate::LiveRendererImageHandoffAdmission::Missing => {
            Err("renderer-image handoff is unexpectedly missing")
        }
    }
}

impl LiveProductionNativeScanout {
    /// Capture the session's retained scene across every enabled head. An
    /// image is only required in the stores that actually rendered it.
    pub fn export_renderer_image_handoff(
        &mut self,
        expected: &[sophia_renderer_live::LiveRendererImageId],
    ) -> Result<LiveProductionRendererImageHandoff, Box<dyn std::error::Error>> {
        let indices = self
            .heads
            .iter()
            .enumerate()
            .filter(|(_, head)| head.enabled)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let captured =
            crate::collect_live_renderer_image_handoff(expected, indices.len(), |owner, image| {
                Ok(self.exporters[indices[owner]].export_promoted_renderer_image(image)?)
            })?;
        let heads = indices
            .into_iter()
            .zip(captured)
            .map(
                |(index, snapshots)| LiveProductionRendererImageHeadHandoff {
                    output: self.heads[index].output.id,
                    card_index: self.heads[index].group,
                    connector_id: self.heads[index].selection.connector_id(),
                    snapshots,
                },
            )
            .collect();
        Ok(LiveProductionRendererImageHandoff {
            expected: expected.to_vec(),
            heads,
        })
    }

    pub fn restore_renderer_image_handoff(
        &mut self,
        handoff: &LiveProductionRendererImageHandoff,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let expected_count = handoff.expected.len();
        let active = self
            .heads
            .iter()
            .enumerate()
            .filter(|(_, head)| head.enabled)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if active.len() != handoff.heads.len() {
            return Err("renderer-image handoff head coverage changed during replacement".into());
        }
        // Resolve all owners before importing anything. Connector numbers are
        // card-local; neither vector order nor a connector alone identifies a head.
        let mut mapped = BTreeSet::new();
        let mut indices = Vec::with_capacity(active.len());
        let mut owners = Vec::with_capacity(active.len());
        let mut images = BTreeSet::new();
        for source in &handoff.heads {
            let index = active
                .iter()
                .copied()
                .find(|index| {
                    let head = &self.heads[*index];
                    head.output.id == source.output
                        && head.group == source.card_index
                        && head.selection.connector_id() == source.connector_id
                })
                .ok_or("renderer-image handoff names an unavailable connector")?;
            if !mapped.insert(index) {
                return Err("renderer-image handoff contains a duplicate head".into());
            }
            if !self.exporters[index].renderer_image_owner_initialized() {
                return Err("replacement renderer image owner is not initialized".into());
            }
            let ids = source
                .snapshots
                .iter()
                .map(sophia_renderer_live::LiveRendererImageSnapshot::image_id)
                .collect::<Vec<_>>();
            images.extend(ids.iter().copied());
            // Shared workers have one image store per card; private workers
            // have one per head. Restore each image only once in either mode.
            let group = self.heads[index].group;
            let owner = if self.groups[group].renderer_core.is_some() {
                self.heads.len() + group
            } else {
                index
            };
            owners.push((owner, ids));
            indices.push(index);
        }
        validate_renderer_image_handoff_ids(
            &handoff.expected,
            &images.into_iter().collect::<Vec<_>>(),
        )?;
        let plan = crate::plan_live_renderer_image_restore(&owners)
            .map_err(|_| "renderer-image handoff contains invalid owner coverage")?;
        for ((index, source), selected) in indices.into_iter().zip(&handoff.heads).zip(plan) {
            let mut selected = selected.into_iter().peekable();
            for (position, snapshot) in source.snapshots.iter().enumerate() {
                if selected.peek() != Some(&position) {
                    continue;
                }
                selected.next();
                if !self.exporters[index].restore_promoted_renderer_image(snapshot.try_clone()?)? {
                    return Err("replacement renderer rejected a retained image snapshot".into());
                }
            }
        }
        Ok(expected_count)
    }
}
