use super::*;

type Image = sophia_renderer_live::LiveRendererImageId;

#[derive(Debug)]
pub struct LiveProductionRendererImageHandoff {
    expected: Vec<Image>,
    heads: Vec<LiveProductionRendererImageHeadHandoff>,
}

#[derive(Debug)]
struct LiveProductionRendererImageHeadHandoff {
    output: OutputId,
    card_index: usize,
    connector_id: u32,
    /// The render device the head's store drew on, when it was known. Card
    /// indices are positions within one owner and identify no device.
    device: Option<LiveRenderDeviceNodeIdentity>,
    snapshots: Vec<sophia_renderer_live::LiveRendererImageSnapshot>,
}

/// What a restore left where. Every image held by a replacement store is in
/// `restored`; an image no store holds yet is `pending`, and its snapshots
/// stay in the handoff. `unavailable` names the (image, output) pairs whose
/// first frames sample an image that output's stores do not hold.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LiveProductionRendererImageRestore {
    pub restored: BTreeSet<Image>,
    pub pending: BTreeSet<Image>,
    pub unavailable: BTreeSet<(Image, OutputId)>,
    /// Some import waited on GPU work still in flight (a busy bridge or
    /// worker), which may finish without any store changing; a retry is due
    /// soon rather than on the next storage change.
    pub busy: bool,
}

impl LiveProductionRendererImageHandoff {
    pub const fn len(&self) -> usize {
        self.expected.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.expected.is_empty()
    }

    pub fn image_ids(&self) -> &[Image] {
        &self.expected
    }

    /// Keeps only the snapshots of `images`, which no store holds yet, once a
    /// replacement has published with every other image in its stores.
    pub fn retain_only(&mut self, images: &BTreeSet<Image>) {
        self.expected.retain(|image| images.contains(image));
        for head in &mut self.heads {
            head.snapshots
                .retain(|snapshot| images.contains(&snapshot.image_id()));
        }
        self.heads.retain(|head| !head.snapshots.is_empty());
    }

    /// Adds a residual's snapshots, of images no store holds, to a handoff
    /// exported from the stores; the two cover disjoint images.
    pub fn absorb(&mut self, residual: Self) {
        self.expected.extend(residual.expected);
        self.heads.extend(residual.heads);
    }
}

fn validate_renderer_image_handoff_ids(
    expected: &[Image],
    actual: &[Image],
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

/// How one import attempt into one store ended. Only a refused import moves
/// on to another snapshot; a store that is full or busy may take the image
/// later and is not asked again in this restore.
enum ImportAttempt {
    Placed,
    Refused,
    /// `busy`: behind GPU work in flight; otherwise the store had no room.
    Deferred {
        busy: bool,
    },
}

fn import_attempt(
    import: crate::LiveRendererImageImport,
) -> Result<ImportAttempt, Box<dyn std::error::Error>> {
    match import {
        crate::LiveRendererImageImport::Placed => Ok(ImportAttempt::Placed),
        crate::LiveRendererImageImport::Refused => Ok(ImportAttempt::Refused),
        crate::LiveRendererImageImport::Deferred { busy } => Ok(ImportAttempt::Deferred { busy }),
        crate::LiveRendererImageImport::Failed(detail) => Err(detail.into()),
        crate::LiveRendererImageImport::Existing => {
            Err("renderer-image import left an existing id unconfirmed".into())
        }
    }
}

impl LiveProductionNativeScanout {
    /// Capture the session's retained scene across every enabled head. An
    /// image is only required in the stores that actually rendered it.
    pub fn export_renderer_image_handoff(
        &mut self,
        expected: &[Image],
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
                    device: self.render_devices.group_identity(self.heads[index].group),
                    snapshots,
                },
            )
            .collect();
        Ok(LiveProductionRendererImageHandoff {
            expected: expected.to_vec(),
            heads,
        })
    }

    /// The enabled head each handoff entry restores into, in handoff order,
    /// when the replacement has exactly the retired heads on the same render
    /// devices. Any difference, including an unknown device, is `None`.
    fn exact_renderer_image_handoff_targets(
        &self,
        handoff: &LiveProductionRendererImageHandoff,
    ) -> Option<Vec<usize>> {
        let active = self
            .heads
            .iter()
            .enumerate()
            .filter(|(_, head)| head.enabled)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if active.len() != handoff.heads.len() {
            return None;
        }
        // Connector numbers are card-local; neither vector order nor a
        // connector alone identifies a head.
        let mut mapped = BTreeSet::new();
        let mut indices = Vec::with_capacity(active.len());
        for source in &handoff.heads {
            let index = active.iter().copied().find(|index| {
                let head = &self.heads[*index];
                head.output.id == source.output
                    && head.group == source.card_index
                    && head.selection.connector_id() == source.connector_id
            })?;
            if !mapped.insert(index) {
                return None;
            }
            indices.push(index);
        }
        let same_devices = crate::live_renderer_image_handoff_same_devices(
            indices.iter().zip(&handoff.heads).map(|(&index, source)| {
                (
                    source.device,
                    self.render_devices.group_identity(self.heads[index].group),
                )
            }),
        );
        same_devices.then_some(indices)
    }

    /// The store an image restored at `index` lands in: shared workers have
    /// one per card, private workers one per head.
    fn renderer_image_store_key(&self, index: usize) -> usize {
        let group = self.heads[index].group;
        if self.groups[group].renderer_core.is_some() {
            self.heads.len() + group
        } else {
            index
        }
    }

    fn import_renderer_image(
        &mut self,
        index: usize,
        snapshot: &sophia_renderer_live::LiveRendererImageSnapshot,
    ) -> Result<ImportAttempt, Box<dyn std::error::Error>> {
        if !self.exporters[index].renderer_image_owner_initialized() {
            return Err("replacement renderer image owner is not initialized".into());
        }
        let import = crate::classify_live_renderer_image_import(
            self.exporters[index].restore_promoted_renderer_image(snapshot.try_clone()?),
        );
        // As the cold path does: an existing id counts only once promoted; a
        // staged one is waited for, never claimed (REVIEW-CODEX-06 R4).
        let attempt = match import {
            crate::LiveRendererImageImport::Existing => {
                match self.exporters[index].try_export_promoted_renderer_image(snapshot.image_id())
                {
                    Ok(Some(_)) => ImportAttempt::Placed,
                    Ok(None) => ImportAttempt::Deferred { busy: true },
                    Err(detail) => {
                        import_attempt(crate::classify_live_renderer_image_import(Err(detail)))?
                    }
                }
            }
            import => import_attempt(import)?,
        };
        if matches!(attempt, ImportAttempt::Placed) {
            self.record_image_owner(index, snapshot.image_id());
        }
        Ok(attempt)
    }

    /// Changes when a store may have room again or deferred GPU work may have
    /// finished, page flip or not: native retirements, and each store's
    /// snapshot promotions, rollbacks and evictions and live entries and
    /// bytes. A retry gated on it waits for real progress instead of
    /// repeating, and is not held back by a still screen (REVIEW-CODEX-06 R1).
    pub fn renderer_storage_progress(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.retirements.hash(&mut hasher);
        for exporter in &self.exporters {
            let stats = exporter.persistent_render_stats();
            // A replaced store is new storage even with equal counters.
            exporter.image_store_identity().hash(&mut hasher);
            (
                stats.snapshot_promotions,
                stats.snapshot_rollbacks,
                stats.snapshot_evictions,
                stats.snapshot_live_entries,
                stats.snapshot_live_bytes,
            )
                .hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Imports `image` into the store at head `index`, from each snapshot in
    /// `sources` in turn until one is not refused.
    fn import_into_store(
        &mut self,
        handoff: &LiveProductionRendererImageHandoff,
        index: usize,
        image: Image,
        sources: &[usize],
    ) -> Result<ImportAttempt, Box<dyn std::error::Error>> {
        let mut outcome = ImportAttempt::Refused;
        for &source in sources {
            let snapshot = handoff.heads[source]
                .snapshots
                .iter()
                .find(|snapshot| snapshot.image_id() == image)
                .ok_or("renderer-image restore plan names a snapshot the handoff lacks")?;
            outcome = self.import_renderer_image(index, snapshot)?;
            if !matches!(outcome, ImportAttempt::Refused) {
                break;
            }
        }
        Ok(outcome)
    }

    /// Restores a retired owner's renderer images into this replacement.
    /// `demand` names, per output, the images its first frames sample.
    ///
    /// With the same heads on the same render devices every image returns to
    /// the store it came from, as before. Otherwise the destination planner
    /// decides: each sampled image goes to the stores of the outputs that
    /// sample it, every other image to one store. Imports that succeed are
    /// kept whatever else happens. The handoff itself is not consumed: its
    /// caller drops it only after the replacement published, and then keeps
    /// the snapshots of the pending images.
    pub fn restore_renderer_image_handoff(
        &mut self,
        handoff: &LiveProductionRendererImageHandoff,
        demand: &BTreeMap<OutputId, BTreeSet<Image>>,
    ) -> Result<LiveProductionRendererImageRestore, Box<dyn std::error::Error>> {
        let all = handoff
            .heads
            .iter()
            .flat_map(|head| head.snapshots.iter().map(|snapshot| snapshot.image_id()))
            .collect::<BTreeSet<_>>();
        validate_renderer_image_handoff_ids(
            &handoff.expected,
            &all.iter().copied().collect::<Vec<_>>(),
        )?;
        if demand.values().flatten().any(|image| !all.contains(image)) {
            return Err("renderer-image restore demand names an image the handoff lacks".into());
        }
        match self.exact_renderer_image_handoff_targets(handoff) {
            Some(indices) => self.restore_renderer_image_handoff_exactly(handoff, &indices, demand),
            None => self.restore_renderer_image_handoff_planned(handoff, demand),
        }
    }

    /// Offers pending snapshots, of images no store held at the last restore,
    /// to this owner's stores again. Always planned: a store that is still
    /// full or busy leaves the image pending rather than failing.
    pub fn place_pending_renderer_images(
        &mut self,
        handoff: &LiveProductionRendererImageHandoff,
    ) -> Result<LiveProductionRendererImageRestore, Box<dyn std::error::Error>> {
        self.restore_renderer_image_handoff_planned(handoff, &BTreeMap::new())
    }

    fn restore_renderer_image_handoff_exactly(
        &mut self,
        handoff: &LiveProductionRendererImageHandoff,
        indices: &[usize],
        demand: &BTreeMap<OutputId, BTreeSet<Image>>,
    ) -> Result<LiveProductionRendererImageRestore, Box<dyn std::error::Error>> {
        let owners = indices
            .iter()
            .zip(&handoff.heads)
            .map(|(&index, source)| {
                (
                    self.renderer_image_store_key(index),
                    source
                        .snapshots
                        .iter()
                        .map(sophia_renderer_live::LiveRendererImageSnapshot::image_id)
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        let plan = crate::plan_live_renderer_image_restore(&owners)
            .map_err(|_| "renderer-image handoff contains invalid owner coverage")?;
        let mut restore = LiveProductionRendererImageRestore::default();
        for ((&index, source), selected) in indices.iter().zip(&handoff.heads).zip(plan) {
            for position in selected {
                let snapshot = &source.snapshots[position];
                let image = snapshot.image_id();
                match self.import_renderer_image(index, snapshot)? {
                    ImportAttempt::Placed => {
                        restore.restored.insert(image);
                    }
                    // A full or busy store may take it later: the image stays
                    // pending, and the outputs that sample it wait for it.
                    ImportAttempt::Deferred { busy } => {
                        restore.busy |= busy;
                        let output = self.heads[index].output.id;
                        if demand
                            .get(&output)
                            .is_some_and(|images| images.contains(&image))
                        {
                            restore.unavailable.insert((image, output));
                        }
                    }
                    // The same head on the same device took this image before;
                    // a refusal now is not the ordinary cost of a topology change.
                    ImportAttempt::Refused => {
                        return Err(
                            "replacement renderer rejected a retained image snapshot".into()
                        );
                    }
                }
            }
        }
        for image in all_images(handoff) {
            if !restore.restored.contains(&image) {
                restore.pending.insert(image);
            }
        }
        Ok(restore)
    }

    fn restore_renderer_image_handoff_planned(
        &mut self,
        handoff: &LiveProductionRendererImageHandoff,
        demand: &BTreeMap<OutputId, BTreeSet<Image>>,
    ) -> Result<LiveProductionRendererImageRestore, Box<dyn std::error::Error>> {
        // One entry per replacement store, imported through its first head.
        let mut store_heads: BTreeMap<usize, usize> = BTreeMap::new();
        for (index, head) in self.heads.iter().enumerate() {
            if head.enabled {
                store_heads
                    .entry(self.renderer_image_store_key(index))
                    .or_insert(index);
            }
        }
        let store_keys = store_heads.keys().copied().collect::<Vec<_>>();
        let stores = store_keys
            .iter()
            .map(|key| crate::LiveRendererImageRestoreStore {
                device: self
                    .render_devices
                    .group_identity(self.heads[store_heads[key]].group),
                free_bytes: sophia_renderer_live::LIVE_RENDERER_IMAGE_STORE_BYTE_BUDGET,
                free_entries: sophia_renderer_live::LIVE_RENDERER_IMAGE_STORE_CAPACITY,
            })
            .collect::<Vec<_>>();
        let sources = handoff
            .heads
            .iter()
            .map(|head| crate::LiveRendererImageRestoreSource {
                device: head.device,
                images: head
                    .snapshots
                    .iter()
                    .map(|snapshot| (snapshot.image_id(), snapshot.byte_estimate()))
                    .collect(),
            })
            .collect::<Vec<_>>();
        // Which outputs each store serves, so demand lands in every store an
        // output's heads draw from.
        let store_outputs = |key: usize| -> BTreeSet<OutputId> {
            self.heads
                .iter()
                .enumerate()
                .filter(|(index, head)| {
                    head.enabled && self.renderer_image_store_key(*index) == key
                })
                .map(|(_, head)| head.output.id)
                .collect()
        };
        let outputs_by_store = store_keys
            .iter()
            .map(|&key| store_outputs(key))
            .collect::<Vec<_>>();
        let planned_demand = demand
            .iter()
            .flat_map(|(output, images)| {
                outputs_by_store
                    .iter()
                    .enumerate()
                    .filter(|(_, outputs)| outputs.contains(output))
                    .flat_map(|(store, _)| images.iter().map(move |image| (store, *image)))
            })
            .collect::<Vec<_>>();
        let plan = crate::plan_live_renderer_image_restore_destinations(
            &sources,
            &stores,
            &planned_demand,
        )?;

        let mut restore = LiveProductionRendererImageRestore::default();
        let mut missing: BTreeSet<(usize, Image)> = plan.refused_demand.iter().copied().collect();
        let demanded = planned_demand.iter().copied().collect::<BTreeSet<_>>();
        let mut held: BTreeSet<(usize, Image)> = BTreeSet::new();
        for import in &plan.imports {
            let sources = std::iter::once(import.source)
                .chain(import.alternates.iter().copied())
                .collect::<Vec<_>>();
            let mut outcome = self.import_into_store(
                handoff,
                store_heads[&store_keys[import.store]],
                import.image,
                &sources,
            )?;
            let mut store = import.store;
            // A store the plan only chose to keep the image in may be fuller
            // than planned (the plan assumes free stores). The store's own
            // admission decides: try the others, same device first, until
            // one keeps it (REVIEW-CODEX-06 R2). Demanded destinations are
            // fixed; they are where the image is sampled.
            if !matches!(outcome, ImportAttempt::Placed)
                && !demanded.contains(&(import.store, import.image))
            {
                let device = stores[import.store].device;
                let mut others = (0..stores.len())
                    .filter(|other| {
                        *other != import.store && !held.contains(&(*other, import.image))
                    })
                    .collect::<Vec<_>>();
                others.sort_by_key(|other| {
                    let same =
                        matches!((stores[*other].device, device), (Some(a), Some(b)) if a == b);
                    (!same, *other)
                });
                for other in others {
                    outcome = self.import_into_store(
                        handoff,
                        store_heads[&store_keys[other]],
                        import.image,
                        &sources,
                    )?;
                    if matches!(outcome, ImportAttempt::Placed) {
                        store = other;
                        break;
                    }
                }
            }
            match outcome {
                ImportAttempt::Placed => {
                    held.insert((store, import.image));
                    restore.restored.insert(import.image);
                }
                ImportAttempt::Refused => {
                    missing.insert((import.store, import.image));
                }
                ImportAttempt::Deferred { busy } => {
                    restore.busy |= busy;
                    missing.insert((import.store, import.image));
                }
            }
        }
        for image in &all_images(handoff) {
            if !restore.restored.contains(image) {
                restore.pending.insert(*image);
            }
        }
        for (store, image) in missing {
            for output in &outputs_by_store[store] {
                if demand
                    .get(output)
                    .is_some_and(|images| images.contains(&image))
                {
                    restore.unavailable.insert((image, *output));
                }
            }
        }
        Ok(restore)
    }
}

fn all_images(handoff: &LiveProductionRendererImageHandoff) -> BTreeSet<Image> {
    handoff
        .heads
        .iter()
        .flat_map(|head| head.snapshots.iter().map(|snapshot| snapshot.image_id()))
        .collect()
}
