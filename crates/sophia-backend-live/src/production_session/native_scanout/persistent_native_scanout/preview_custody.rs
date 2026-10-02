//! Foreign preview frames own immutable storage before entering any worker.
use super::*;
use sophia_renderer_live::{
    LiveOwnedMixedCompositionLayer as Layer, LiveRendererImageId as Image,
    LiveRendererSnapshotBudget as Budget, LiveRendererSnapshotEpoch as Epoch,
    LiveRetainedRendererImageSnapshot as Snapshot,
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LivePreviewImageRefusal {
    Pending {
        image: Image,
    },
    Missing {
        image: Image,
    },
    CrossDevice {
        image: Image,
    },
    Renderer {
        image: Image,
        detail: crate::LiveRendererScanoutBufferExportDetail,
    },
}
impl std::fmt::Display for LivePreviewImageRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Pending { .. } => "preview image preparation pending",
            Self::Missing { .. } => "preview image missing",
            Self::CrossDevice { .. } => "preview image crosses render devices",
            Self::Renderer { .. } => "preview image renderer failure",
        })
    }
}
impl std::error::Error for LivePreviewImageRefusal {}
impl LivePreviewImageRefusal {
    pub fn image(self) -> Image {
        match self {
            Self::Pending { image }
            | Self::Missing { image }
            | Self::CrossDevice { image }
            | Self::Renderer { image, .. } => image,
        }
    }
}

pub(super) struct PreviewImages {
    pub reads: sophia_renderer_live::LiveRendererImageReads,
    // Store identities are process-unique, not head indices or Arc addresses.
    pub owners: BTreeMap<Image, BTreeSet<u64>>,
    pub snapshots: BTreeMap<(Image, usize), Snapshot>,
    pub cold_misses: BTreeMap<Image, BTreeSet<OutputId>>,
    pub demand_sources: BTreeMap<Image, (sophia_protocol::SurfaceId, BTreeSet<OutputId>)>,
    pub frame_sources: BTreeMap<crate::LiveNativeFrameIdentity, sophia_protocol::SurfaceId>,
    // Retired imports can outlive the last queued frame. Busy workers keep
    // these requests until completion, without retaining new snapshot FDs.
    retired_imports: BTreeMap<u64, Vec<(Image, sophia_renderer_live::LiveRendererSnapshotWeak)>>,
    live_stores: BTreeMap<u64, usize>,
    epochs: BTreeMap<(u64, u64), Arc<Epoch>>,
    inventory_generation: Option<u64>,
    budget: Arc<Budget>,
}
impl Default for PreviewImages {
    fn default() -> Self {
        Self {
            reads: Default::default(),
            owners: BTreeMap::new(),
            snapshots: BTreeMap::new(),
            cold_misses: BTreeMap::new(),
            demand_sources: BTreeMap::new(),
            frame_sources: BTreeMap::new(),
            retired_imports: BTreeMap::new(),
            live_stores: BTreeMap::new(),
            epochs: BTreeMap::new(),
            inventory_generation: None,
            budget: Budget::new(128, 256 * 1024 * 1024),
        }
    }
}
impl Drop for PreviewImages {
    fn drop(&mut self) {
        self.invalidate();
    }
}
impl PreviewImages {
    pub(super) fn synchronize(&mut self, generation: u64, stores: &BTreeMap<u64, usize>) {
        if self
            .inventory_generation
            .is_some_and(|old| old != generation)
        {
            self.invalidate_foreign();
        }
        self.inventory_generation = Some(generation);
        self.live_stores = stores.clone();
        self.retired_imports
            .retain(|store, _| stores.contains_key(store));
        self.epochs.retain(|(store, old_generation), epoch| {
            let valid = stores.contains_key(store) && *old_generation == generation;
            if !valid {
                epoch.invalidate();
            }
            valid
        });
        self.owners.retain(|_, owners| {
            owners.retain(|store| stores.contains_key(store));
            !owners.is_empty()
        });
        self.cold_misses
            .retain(|image, _| self.owners.contains_key(image));
        let stale = self
            .snapshots
            .iter()
            .filter(|(_, snapshot)| !snapshot.is_current())
            .map(|(key, _)| *key)
            .collect::<Vec<_>>();
        for key in stale {
            self.retire_snapshot(key);
        }
    }

    fn preview_on_output(&self, image: Image, output: OutputId) -> bool {
        self.demand_sources
            .get(&image)
            .is_some_and(|(_, outputs)| outputs.contains(&output))
    }

    pub(super) fn invalidate(&mut self) {
        self.invalidate_foreign();
        self.reads.clear_evictions();
        self.owners.clear();
        self.cold_misses.clear();
        self.retired_imports.clear();
        self.live_stores.clear();
    }

    /// Inventory changes revoke foreign attachments, but surviving contexts
    /// still own their local images and must complete delayed evictions.
    pub(super) fn invalidate_foreign(&mut self) {
        for epoch in self.epochs.values() {
            epoch.invalidate();
        }
        for image in self.snapshots.keys().copied().collect::<Vec<_>>() {
            self.retire_snapshot(image);
        }
        self.demand_sources.clear();
        // Frame attribution survives invalidation: a queued old-epoch snapshot
        // must still revoke its own publication if the worker refuses it.
        self.epochs.clear();
        // The budget persists: queued frames, imports and submitted buffers
        // still charge old allocations until their actual last owner drops.
    }

    pub(super) fn retire_snapshot(&mut self, key: (Image, usize)) {
        if let Some(snapshot) = self.snapshots.remove(&key) {
            let (image, group) = key;
            snapshot.retire_import_cache();
            for (&store, &store_group) in &self.live_stores {
                if group != store_group {
                    continue;
                }
                let pending = self.retired_imports.entry(store).or_default();
                pending.retain(|(_, weak)| weak.is_alive());
                pending.push((image, snapshot.downgrade()));
            }
        }
    }
}

impl LiveProductionNativeScanout {
    pub(super) fn record_image_owner(&mut self, index: usize, image: Image) {
        if let Some(owner) = self.exporters[index].image_store_identity() {
            self.preview_images
                .owners
                .entry(image)
                .or_default()
                .insert(owner);
        }
    }

    pub(super) fn attach_preview_custody(
        &mut self,
        batches: &mut composition_admission::NativeHeadCompositionBatch,
        allow_foreign: bool,
    ) -> Result<(), LivePreviewImageRefusal> {
        self.synchronize_preview_epochs();
        for (output, frames) in batches.iter_mut() {
            for frame in frames {
                let index = self
                    .heads
                    .iter()
                    .position(|head| head.output.id == *output && head.head == frame.head)
                    .expect("validated native head");
                let local = self.exporters[index].image_store_identity();
                // A frame can capture its own source and sample it again in
                // the same display list, before promotion. That is local too.
                let captured = frame
                    .frame
                    .layers
                    .iter()
                    .filter_map(|layer| match layer {
                        Layer::DmaBuf { image_id, .. } => Some(*image_id),
                        _ => None,
                    })
                    .collect::<BTreeSet<_>>();
                for layer in &mut frame.frame.layers {
                    let Layer::RendererImage {
                        image_id,
                        placement,
                        ..
                    } = layer
                    else {
                        continue;
                    };
                    let image = *image_id;
                    let owners = self.preview_images.owners.get(&image);
                    if captured.contains(&image)
                        || local.is_some_and(|id| owners.is_some_and(|set| set.contains(&id)))
                    {
                        continue;
                    }
                    if !allow_foreign {
                        // Recovery freezes sources that were local at admission.
                        // Do not schedule a cold restore for a broken retry.
                        return Err(LivePreviewImageRefusal::Missing { image });
                    }
                    if !self.preview_images.preview_on_output(image, *output) && owners.is_some() {
                        // The ordinary surface's one-shot store migration is
                        // still waiting for a donor or recipient worker.
                        self.preview_images
                            .cold_misses
                            .entry(image)
                            .or_default()
                            .insert(*output);
                        return Err(LivePreviewImageRefusal::Pending { image });
                    }
                    let group = self.heads[index].group;
                    let snapshot = self
                        .preview_images
                        .snapshots
                        .get(&(image, group))
                        .filter(|snapshot| snapshot.is_current());
                    let snapshot =
                        match snapshot {
                            Some(snapshot) => snapshot,
                            None => {
                                // Classification only: never choose a donor here.
                                // Preparation binds each cached snapshot to its GPU.
                                if owners.is_none() {
                                    return Err(LivePreviewImageRefusal::Missing { image });
                                }
                                let same_group_owner = self.heads.iter().zip(&self.exporters).any(
                                    |(head, exporter)| {
                                        head.enabled
                                            && head.group == group
                                            && exporter.image_store_identity().is_some_and(
                                                |store| {
                                                    owners.is_some_and(|set| set.contains(&store))
                                                },
                                            )
                                    },
                                );
                                return Err(if same_group_owner {
                                    LivePreviewImageRefusal::Pending { image }
                                } else {
                                    LivePreviewImageRefusal::CrossDevice { image }
                                });
                            }
                        };
                    *layer = Layer::Snapshot {
                        snapshot: snapshot.clone(),
                        placement: *placement,
                    };
                }
            }
        }
        for (_, frames) in batches {
            for frame in frames {
                frame.frame.image_reads = frame
                    .frame
                    .layers
                    .iter()
                    .filter_map(|layer| match layer {
                        Layer::RendererImage { image_id, .. } | Layer::DmaBuf { image_id, .. } => {
                            Some(self.preview_images.reads.acquire(*image_id))
                        }
                        _ => None,
                    })
                    .collect();
            }
        }
        Ok(())
    }
}

include!("preview_preparation.rs");
