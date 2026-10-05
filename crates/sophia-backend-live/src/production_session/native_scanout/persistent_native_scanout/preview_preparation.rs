// Demand preparation is separate from frame admission. Admission only reads
// immutable, budgeted handles; it never waits for a donor worker.
impl LiveProductionNativeScanout {
    fn renderer_store_busy(&self, index: usize) -> bool {
        let store = self.exporters[index].image_store_identity();
        self.exporters
            .iter()
            .any(|exporter| exporter.image_store_identity() == store && exporter.worker_in_flight())
    }

    /// Ordinary surfaces migrate once to their new head's existing store.
    /// This reuses topology's export/restore path, including import transfer
    /// fallback, without spending the repeating-preview snapshot budget.
    ///
    /// One cold-migration pass. Returns the (image, output) pairs whose
    /// import the output's stores refused; the runtime leaves those surfaces
    /// out on that output while the donor keeps its copy.
    pub(crate) fn prepare_retained_images(
        &mut self,
    ) -> Result<Vec<(Image, OutputId)>, crate::LiveRendererScanoutBufferExportDetail> {
        use crate::LiveRendererScanoutBufferExportDetail as D;
        self.synchronize_preview_epochs();
        if self.preview_images.cold_misses.is_empty() { return Ok(Vec::new()); }
        let stores = self.cold_image_stores();
        let progress = self.retirements;
        let exporters = &mut self.exporters;
        prepare_cold_images(
            &mut self.preview_images.cold_misses,
            &mut self.preview_images.owners,
            &stores,
            &mut self.preview_images.cold_gate,
            progress,
            |donor, target, image| {
                let snapshot = exporters[donor]
                    .try_export_promoted_renderer_image(image)?
                    .ok_or(D::InvalidRendererImageId)?;
                let restored = exporters[target].restore_promoted_renderer_image(snapshot)?;
                confirm_cold_restore(restored, || {
                    Ok(exporters[target].try_export_promoted_renderer_image(image)?.is_some())
                })
            },
        )
    }

    pub(crate) fn cold_preparation_ready(&self) -> bool {
        !self.preview_images.cold_misses.is_empty() && cold_preparation_ready(
            &self.preview_images.cold_misses,
            &self.preview_images.owners,
            &self.cold_image_stores(),
            &self.preview_images.cold_gate,
            self.retirements,
        )
    }

    fn cold_image_stores(&self) -> Vec<ColdImageStore> {
        self
            .heads
            .iter()
            .enumerate()
            .filter(|(_, head)| head.enabled)
            .filter_map(|(index, head)| {
                self.exporters[index]
                    .image_store_identity()
                    .map(|identity| ColdImageStore {
                        index,
                        output: head.output.id,
                        group: head.group,
                        identity,
                        busy: self.renderer_store_busy(index),
                    })
            })
            .collect()
    }

    fn service_retired_preview_imports(
        &mut self,
    ) -> Result<(), (Image, crate::LiveRendererScanoutBufferExportDetail)> {
        // At most the live budgeted snapshots per store, plus no strong
        // ownership of our own. Dead requests disappear even for busy stores.
        self.preview_images.retired_imports.retain(|_, pending| {
            pending.retain(|(_, weak)| weak.is_alive());
            !pending.is_empty()
        });
        let mut visited = BTreeSet::new();
        for index in 0..self.exporters.len() {
            let Some(store) = self.exporters[index].image_store_identity() else {
                continue;
            };
            if !visited.insert(store) || self.renderer_store_busy(index) {
                continue;
            }
            let Some(pending) = self.preview_images.retired_imports.get(&store) else {
                continue;
            };
            let images = pending
                .iter()
                .map(|(image, _)| *image)
                .collect::<BTreeSet<_>>();
            for image in images {
                self.exporters[index]
                    .evict_renderer_image_imports(image)
                    .map_err(|detail| (image, detail))?;
                self.preview_images
                    .retired_imports
                    .get_mut(&store)
                    .unwrap()
                    .retain(|(pending_image, _)| *pending_image != image);
            }
            self.preview_images.retired_imports.remove(&store);
        }
        Ok(())
    }

    pub(crate) fn renderer_image_reads(&self) -> sophia_renderer_live::LiveRendererImageReads {
        self.preview_images.reads.clone()
    }

    pub(crate) fn use_renderer_image_reads(
        &mut self,
        reads: sophia_renderer_live::LiveRendererImageReads,
    ) -> Result<(), &'static str> {
        if !self.preview_images.reads.same_registry(&reads)
            && (self.preview_images.reads.has_readers()
                || self.preview_images.reads.has_pending_evictions())
        {
            return Err("native renderer registry replaced with live readers or evictions");
        }
        self.preview_images.reads = reads;
        Ok(())
    }

    /// No new allocations: delayed removals stay inside each store's existing
    /// count and byte bounds. Guards drop with frames, including on coalescing,
    /// failed rendering, topology discard and shutdown.
    pub(crate) fn service_renderer_image_evictions(
        &mut self,
    ) -> Result<(), crate::LiveRendererScanoutBufferExportDetail> {
        self.preview_images.reads.prune();
        let ready = self.preview_images.reads.ready_evictions();
        for image in ready {
            self.evict_renderer_image(image)?;
        }
        Ok(())
    }

    /// Store replacement and inventory changes invalidate old attachments even
    /// if a caller missed an explicit clear. Head teardown drops donor identity.
    pub(super) fn synchronize_preview_epochs(&mut self) {
        let generation = self.render_devices.generation;
        let stores = self.heads.iter().zip(&self.exporters)
            .filter(|(head, _)| head.enabled)
            .filter_map(|(head, exporter)| exporter.image_store_identity().map(|store| (store, head.group)))
            .collect::<BTreeMap<_, _>>();
        self.preview_images.synchronize(generation, &stores);
    }

    fn retain_preview_snapshot(
        &mut self,
        index: usize,
        snapshot: sophia_renderer_live::LiveRendererImageSnapshot,
    ) -> Result<(), LivePreviewImageRefusal> {
        let image = snapshot.image_id();
        let store = self.exporters[index]
            .image_store_identity()
            .ok_or(LivePreviewImageRefusal::Missing { image })?;
        let card = self
            .render_devices
            .group_identity(self.heads[index].group)
            .ok_or(LivePreviewImageRefusal::Missing { image })?
            .device_number;
        let generation = self.render_devices.generation;
        let epoch = self
            .preview_images
            .epochs
            .entry((store, generation))
            .or_insert_with(|| Epoch::new(card, generation, store))
            .clone();
        let snapshot = snapshot
            .retain(&self.preview_images.budget, epoch)
            .map_err(|detail| LivePreviewImageRefusal::Renderer { image, detail })?;
        self.preview_images.snapshots.insert((image, self.heads[index].group), snapshot);
        Ok(())
    }

    /// Promotion is source retirement. Optional preview preparation cannot
    /// change that result, including when allocation accounting refuses it.
    pub fn promote_renderer_image_for_previews(
        &mut self,
        image: Image,
        source: Option<sophia_protocol::SurfaceId>,
        capturing_outputs: &BTreeSet<OutputId>,
        preview_outputs: &BTreeSet<OutputId>,
    ) -> Result<
        (usize, Result<(), LivePreviewImageRefusal>),
        crate::LiveRendererScanoutBufferExportDetail,
    > {
        self.synchronize_preview_epochs();
        let capturing_stores = self
            .heads
            .iter()
            .zip(&self.exporters)
            .filter(|(head, _)| head.enabled && capturing_outputs.contains(&head.output.id))
            .filter_map(|(_, exporter)| exporter.image_store_identity())
            .collect::<BTreeSet<_>>();
        let required_groups = self.heads.iter().zip(&self.exporters)
            .filter(|(head, exporter)| head.enabled && preview_outputs.contains(&head.output.id)
                && exporter.image_store_identity().is_some_and(|store| !capturing_stores.contains(&store)))
            .map(|(head, _)| head.group).collect::<BTreeSet<_>>();
        if !required_groups.is_empty() && let Some(source) = source {
            self.preview_images.demand_sources.insert(image, (source, preview_outputs.clone()));
        }
        let mut promoted = 0;
        let mut preview = Ok(());
        let mut exported = BTreeSet::new();
        let mut visited = BTreeSet::new();
        for index in 0..self.exporters.len() {
            if let Some(store) = self.exporters[index].image_store_identity()
                && !visited.insert(store)
            {
                continue;
            }
            // The export rides the existing promotion visit. A facade with no
            // image returns false without exporting anything.
            let group = self.heads[index].group;
            let changed = if self.heads[index].enabled && required_groups.contains(&group)
                && !exported.contains(&group) {
                let result = self.exporters[index].promote_and_export_renderer_image(image)?;
                if result.promoted {
                    exported.insert(group);
                    let retained = match result.snapshot {
                        Ok(Some(snapshot)) => self.retain_preview_snapshot(index, snapshot),
                        Ok(None) => Err(LivePreviewImageRefusal::Missing { image }),
                        Err(detail) => Err(LivePreviewImageRefusal::Renderer { image, detail }),
                    };
                    if preview.is_ok() { preview = retained; }
                }
                result.promoted
            } else {
                self.exporters[index].promote_renderer_image(image)?
            };
            if changed {
                self.record_image_owner(index, image);
            }
            promoted += usize::from(changed);
        }
        if preview.is_ok() && !required_groups.is_subset(&exported) {
            preview = Err(LivePreviewImageRefusal::CrossDevice { image });
        }
        Ok((promoted, preview))
    }

    /// Cold publication preparation. This is the only extra worker visit:
    /// export the *already displayed* revision once, after a WM begins using
    /// it. A busy worker defers without discarding its in-flight render.
    /// Later revisions arrive with promotion and never export at admission.
    pub fn prepare_preview_images(
        &mut self,
        demand: &BTreeMap<Image, (sophia_protocol::SurfaceId, BTreeSet<OutputId>)>,
    ) -> Result<(), LivePreviewImageRefusal> {
        self.synchronize_preview_epochs();
        self.preview_images.demand_sources = demand.clone();
        let stores = self.cold_image_stores();
        let mut needed = BTreeMap::new();
        for (&image, (_, outputs)) in demand {
            for (group, donor) in preview_group_donors(image, outputs,
                self.preview_images.owners.get(&image), &stores)? {
                needed.insert((image, group), donor);
            }
        }
        let obsolete = self.preview_images.snapshots.keys().copied()
            .filter(|key| !needed.contains_key(key)).collect::<Vec<_>>();
        for key in obsolete { self.preview_images.retire_snapshot(key); }
        self.service_retired_preview_imports()
            .map_err(|(image, detail)| LivePreviewImageRefusal::Renderer { image, detail })?;
        for ((image, group), donor) in needed {
            if self.preview_images.snapshots.contains_key(&(image, group))
                || self.renderer_store_busy(donor) { continue; }
            let snapshot = self.exporters[donor]
                .try_export_promoted_renderer_image(image)
                .map_err(|detail| LivePreviewImageRefusal::Renderer { image, detail })?
                .ok_or(LivePreviewImageRefusal::Missing { image })?;
            self.retain_preview_snapshot(donor, snapshot)?;
        }
        Ok(())
    }
}

/// A false restore result means the immutable id already exists. If it is
/// staged, wait for promotion instead of claiming it as a local retained image.
/// Rollback may remove it in the meantime; the next restore then captures anew.
fn confirm_cold_restore(
    restored: bool,
    confirm_promoted: impl FnOnce() -> Result<bool, crate::LiveRendererScanoutBufferExportDetail>,
) -> Result<(), crate::LiveRendererScanoutBufferExportDetail> {
    if restored || confirm_promoted()? {
        Ok(())
    } else {
        Err(crate::LiveRendererScanoutBufferExportDetail::WorkerPending)
    }
}

/// Bind each recipient GPU to a donor on that GPU, independent of head order.
fn preview_group_donors(
    image: Image,
    outputs: &BTreeSet<OutputId>,
    owners: Option<&BTreeSet<u64>>,
    stores: &[ColdImageStore],
) -> Result<BTreeMap<usize, usize>, LivePreviewImageRefusal> {
    let groups = stores.iter().filter(|s| outputs.contains(&s.output)
        && !owners.is_some_and(|set| set.contains(&s.identity)))
        .map(|s| s.group).collect::<BTreeSet<_>>();
    let mut result = BTreeMap::new();
    for group in groups {
        let owners = owners.ok_or(LivePreviewImageRefusal::Missing { image })?;
        let donor = stores.iter().filter(|s| s.group == group && owners.contains(&s.identity))
            .min_by_key(|s| s.busy).ok_or(LivePreviewImageRefusal::CrossDevice { image })?;
        result.insert(group, donor.index);
    }
    Ok(result)
}

/// One bounded miss set; the closure is the existing store export/restore.
/// Its worker completion controls wakeups. There is no retry timer here.
struct ColdImageStore {
    index: usize,
    output: OutputId,
    group: usize,
    identity: u64,
    busy: bool,
}

/// One pass of cold migration: for each (image, output) the output's stores
/// lack, export from a donor store and restore into the target. Returns the
/// (image, output) pairs whose import was refused outright; those leave the
/// demand and the donor keeps its copy. A full store may be waiting on GPU
/// completions rather than out of room (REVIEW-CODEX-05 R2): the pair is gated
/// at `progress`, the caller's native retirement count, and neither tried nor
/// reported ready again until that count moves, so nothing spins.
fn prepare_cold_images(
    demand: &mut BTreeMap<Image, BTreeSet<OutputId>>,
    owners: &mut BTreeMap<Image, BTreeSet<u64>>,
    stores: &[ColdImageStore],
    gate: &mut BTreeMap<(Image, OutputId), usize>,
    progress: usize,
    mut transfer: impl FnMut(
        usize,
        usize,
        Image,
    ) -> Result<(), crate::LiveRendererScanoutBufferExportDetail>,
) -> Result<Vec<(Image, OutputId)>, crate::LiveRendererScanoutBufferExportDetail> {
    use crate::LiveRendererScanoutBufferExportDetail as D;
    // One export/restore pair per owner pass (each worker visit is bounded).
    // Busy facts cost no visit; remaining ready misses keep the owner awake.
    let mut attempted = false;
    let mut refused = Vec::new();
    gate.retain(|_, at| *at == progress);
    for (image, outputs) in demand.clone() {
        for output in outputs {
            if gate.contains_key(&(image, output)) {
                continue;
            }
            let mut ready = true;
            let mut refused_here = false;
            for target in stores.iter().filter(|store| store.output == output) {
                if owners
                    .get(&image)
                    .is_some_and(|set| set.contains(&target.identity))
                {
                    continue;
                }
                let donor = stores
                    .iter()
                    .filter(|store| {
                        owners
                            .get(&image)
                            .is_some_and(|set| set.contains(&store.identity))
                    })
                    .min_by_key(|store| (store.group != target.group, store.busy))
                    .ok_or(D::InvalidRendererImageId)?;
                if attempted || target.busy || donor.busy {
                    ready = false;
                    continue;
                }
                attempted = true;
                match transfer(donor.index, target.index, image) {
                    Ok(()) => {
                        owners.entry(image).or_default().insert(target.identity);
                    }
                    Err(D::WorkerPending | D::WorkerQueueFull) => {
                        ready = false;
                    }
                    Err(D::RendererImageStoreFull) => {
                        gate.insert((image, output), progress);
                        ready = false;
                    }
                    Err(D::DmaBufImageCreateFailed | D::DmaBufImageBindFailed | D::DmaBufImportFailed) => {
                        refused_here = true;
                        break;
                    }
                    Err(detail) => return Err(detail),
                }
            }
            if refused_here {
                refused.push((image, output));
            }
            if (ready || refused_here) && let Some(outputs) = demand.get_mut(&image) {
                outputs.remove(&output);
            }
        }
    }
    demand.retain(|_, outputs| !outputs.is_empty());
    Ok(refused)
}

/// No frame is fabricated for preparation. The owner schedules another pass
/// only when a miss can progress; busy workers supply completion wakeups.
fn cold_preparation_ready(
    demand: &BTreeMap<Image, BTreeSet<OutputId>>,
    owners: &BTreeMap<Image, BTreeSet<u64>>,
    stores: &[ColdImageStore],
    gate: &BTreeMap<(Image, OutputId), usize>,
    progress: usize,
) -> bool {
    demand.iter().any(|(image, outputs)| {
        let owned = owners.get(image);
        outputs.iter().any(|output| {
            // A pair gated on a full store waits for progress, not a pass.
            if gate.get(&(*image, *output)) == Some(&progress) {
                return false;
            }
            stores.iter().filter(|s| s.output == *output).any(|target| {
                if owned.is_some_and(|set| set.contains(&target.identity)) {
                    return false;
                }
                let donor = stores.iter()
                    .filter(|s| owned.is_some_and(|set| set.contains(&s.identity)))
                    .min_by_key(|s| (s.group != target.group, s.busy));
                // A missing donor needs its classified invariant evaluated.
                donor.is_none_or(|donor| !target.busy && !donor.busy)
            })
        })
    })
}

#[cfg(test)]
#[path = "../../../../tests/support/cold_image_preparation.rs"]
mod cold_tests;
