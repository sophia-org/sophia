//! Transition-only diagnostics. These observations grant no input authority.
use sophia_backend_live::{
    LiveNativeHeadRecord, LivePresentedInputProjection, LiveProductionNativeScanout,
};
use sophia_engine::{PresentedContentTransform, RenderHeadId};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Mapping {
    owner: u64,
    physical: LiveNativeHeadRecord,
    target_generation: u64,
    head_scale: u32,
    transform: PresentedContentTransform,
}

#[derive(Default)]
pub(super) struct ContentMappingEvidence {
    heads: BTreeMap<RenderHeadId, Mapping>,
}

impl ContentMappingEvidence {
    pub fn observe(
        &mut self,
        native: &LiveProductionNativeScanout,
        projections: &[LivePresentedInputProjection],
    ) {
        self.heads
            .retain(|head, _| native.head_table.head(*head).is_some());
        for (head_index, head) in native
            .heads
            .iter()
            .enumerate()
            .filter(|(_, head)| head.enabled)
        {
            let Some(physical) = native.head_table.head(head.head) else {
                continue;
            };
            let Some(binding) = projections
                .iter()
                .find(|p| p.output == physical.output)
                .and_then(|p| p.content.as_ref())
                .filter(|b| b.authority_current)
            else {
                continue;
            };
            let owner = native.retirement_owner_identity();
            if self.heads.get(&head.head).is_some_and(|old| {
                old.owner == owner
                    && &old.physical == physical
                    && old.target_generation == head.target_generation
                    && old.head_scale == head.scale
                    && old.transform == binding.transform
            }) {
                continue;
            }
            let mapping = Mapping {
                owner,
                physical: physical.clone(),
                target_generation: head.target_generation,
                head_scale: head.scale,
                transform: binding.transform.clone(),
            };
            let r = mapping.transform.viewport;
            // Inspect the already-held card descriptor; no device open or ioctl.
            let device = rustix::fs::fstat(native.card(head_index))
                .ok()
                .map(|stat| {
                    format!(
                        "{}:{}",
                        rustix::fs::major(stat.st_rdev),
                        rustix::fs::minor(stat.st_rdev)
                    )
                })
                .unwrap_or_else(|| "unavailable".into());
            crate::session_println!(
                "sophia_shell_output_mapping schema=1 owner={} card_index={} device={} connector={} connector_id={} head={} output={} target_generation={} layout_generation={} head_scale={} x={} y={} width={} height={}",
                mapping.owner,
                physical.card_index,
                device,
                physical.connector_name,
                physical.connector_id,
                physical.head.raw(),
                physical.output.raw(),
                mapping.target_generation,
                mapping.transform.layout_generation,
                mapping.head_scale,
                r.x,
                r.y,
                r.width,
                r.height,
            );
            self.heads.insert(head.head, mapping);
        }
    }
}
