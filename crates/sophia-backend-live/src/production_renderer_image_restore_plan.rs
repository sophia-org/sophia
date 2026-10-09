//! Where a renderer-image handoff goes when the heads changed (t306).
//!
//! A handoff holds one set of snapshots per retired head. When the
//! replacement has other heads, a snapshot cannot simply return to its own
//! head. This decides, without touching a device, which replacement store
//! imports which image and from which snapshot. Every image a store samples in
//! its first frames goes into that store. Every other image still owned goes
//! into exactly one store, from which the cold migration serves a later move;
//! copying every image into every store could exceed a store's bounds for a
//! scene each store holds comfortably. Stores are charged bytes and entries as
//! images are placed, and what fits nowhere is reported, never dropped.
//!
//! Device identities are the render devices the snapshots were drawn on and
//! the stores draw on. An unknown identity is never "the same device", and
//! card or group indices are never compared across owners.

use sophia_renderer_live::LiveRendererImageId;
use std::collections::{BTreeMap, BTreeSet};

/// One retired head's snapshots: each image with its size in bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveRendererImageRestoreSource<D> {
    pub device: Option<D>,
    pub images: Vec<(LiveRendererImageId, u64)>,
}

/// One replacement image store and what it can still take.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveRendererImageRestoreStore<D> {
    pub device: Option<D>,
    pub free_bytes: u64,
    pub free_entries: usize,
}

/// One import: `source` first, then `alternates` in order if the import is
/// refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveRendererImageRestoreImport {
    pub store: usize,
    pub image: LiveRendererImageId,
    pub source: usize,
    pub alternates: Vec<usize>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LiveRendererImageRestorePlan {
    pub imports: Vec<LiveRendererImageRestoreImport>,
    /// A store's first frames sample the image but the store cannot take it.
    pub refused_demand: Vec<(usize, LiveRendererImageId)>,
    /// A snapshot owns the image but no store can take it.
    pub unplaced: Vec<LiveRendererImageId>,
}

/// Plans the imports for a changed head set. `demand` names, by store index,
/// the images each store's first frames sample; duplicates are one demand.
///
/// The plan is advice. Sizes are the sources' and a destination may charge
/// more (pitch, pool, bridge); the store's own admission decides. An image
/// the plan leaves unplaced still owns its snapshots: the caller keeps them
/// rather than treating the plan as proof that the image cannot be held.
pub fn plan_live_renderer_image_restore_destinations<D: Copy + Eq>(
    sources: &[LiveRendererImageRestoreSource<D>],
    stores: &[LiveRendererImageRestoreStore<D>],
    demand: &[(usize, LiveRendererImageId)],
) -> Result<LiveRendererImageRestorePlan, &'static str> {
    // Each image's size, and the snapshots that hold it in source order.
    let mut images: BTreeMap<LiveRendererImageId, (u64, Vec<usize>)> = BTreeMap::new();
    for (index, source) in sources.iter().enumerate() {
        let mut seen = BTreeSet::new();
        for &(image, bytes) in &source.images {
            if !image.is_valid() {
                return Err("renderer-image restore names an invalid image");
            }
            if !seen.insert(image) {
                return Err("renderer-image restore source holds an image twice");
            }
            let entry = images.entry(image).or_insert((bytes, Vec::new()));
            // Copies of one image may differ in pitch; plan for the largest.
            entry.0 = entry.0.max(bytes);
            entry.1.push(index);
        }
    }
    let demand = demand.iter().copied().collect::<BTreeSet<_>>();
    for &(store, image) in &demand {
        if store >= stores.len() {
            return Err("renderer-image restore demand names an unknown store");
        }
        if !images.contains_key(&image) {
            return Err("renderer-image restore demand names an image no snapshot holds");
        }
    }

    let same_device = |a: Option<D>, b: Option<D>| matches!((a, b), (Some(a), Some(b)) if a == b);
    // A snapshot drawn on the store's own device first, then source order.
    let order_sources = |holders: &[usize], device: Option<D>| {
        let mut ordered = holders.to_vec();
        ordered.sort_by_key(|&source| (!same_device(sources[source].device, device), source));
        ordered
    };
    let mut free = stores
        .iter()
        .map(|store| (store.free_bytes, store.free_entries))
        .collect::<Vec<_>>();
    let mut plan = LiveRendererImageRestorePlan::default();
    let mut owned = BTreeSet::new();
    let place = |plan: &mut LiveRendererImageRestorePlan,
                 free: &mut [(u64, usize)],
                 store: usize,
                 image: LiveRendererImageId|
     -> bool {
        let (bytes, holders) = &images[&image];
        let (free_bytes, free_entries) = &mut free[store];
        if *free_entries == 0 || *free_bytes < *bytes {
            return false;
        }
        *free_bytes -= bytes;
        *free_entries -= 1;
        let mut ordered = order_sources(holders, stores[store].device);
        let source = ordered.remove(0);
        plan.imports.push(LiveRendererImageRestoreImport {
            store,
            image,
            source,
            alternates: ordered,
        });
        true
    };

    for &(store, image) in &demand {
        if place(&mut plan, &mut free, store, image) {
            owned.insert(image);
        } else {
            plan.refused_demand.push((store, image));
        }
    }
    // Largest first, then by id: placing small images first can strand a
    // large one a store had room for (two 200 MiB images spread over two
    // stores leave neither the 400 MiB the third needs). This is a planning
    // order, not a proof: an image left unplaced is not shown not to fit.
    let mut optional = images
        .iter()
        .filter(|(image, _)| !owned.contains(*image))
        .map(|(&image, &(bytes, _))| (std::cmp::Reverse(bytes), image))
        .collect::<Vec<_>>();
    optional.sort_unstable();
    for (_, image) in optional {
        let holders = &images[&image].1;
        // Same device as a snapshot of the image first, then the most room,
        // then the lowest index, so equal inputs give equal plans.
        let mut candidates = (0..stores.len()).collect::<Vec<_>>();
        candidates.sort_by_key(|&store| {
            let near = holders
                .iter()
                .any(|&source| same_device(sources[source].device, stores[store].device));
            (!near, std::cmp::Reverse(free[store].0), store)
        });
        if candidates
            .into_iter()
            .any(|store| place(&mut plan, &mut free, store, image))
        {
            owned.insert(image);
        } else {
            plan.unplaced.push(image);
        }
    }
    Ok(plan)
}

/// Whether retired heads and their replacements are the same render devices
/// pairwise. The exact-head restore relies on it; an unknown device on either
/// side means the general planner decides.
pub fn live_renderer_image_handoff_same_devices<D: Copy + Eq>(
    pairs: impl IntoIterator<Item = (Option<D>, Option<D>)>,
) -> bool {
    pairs
        .into_iter()
        .all(|pair| matches!(pair, (Some(retired), Some(replacement)) if retired == replacement))
}

/// How one import of a snapshot into a store ended (t306).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveRendererImageImport {
    Placed,
    /// The store already had the id, perhaps only staged; promotion must be
    /// confirmed before it counts.
    Existing,
    /// The store's device refused the snapshot; another snapshot may do.
    Refused,
    /// Not now: `busy` is GPU work still in flight, retried soon; otherwise
    /// the store had no room and waits for storage to change.
    Deferred {
        busy: bool,
    },
    /// Not an ordinary cost of a topology change.
    Failed(sophia_renderer_live::LiveRendererScanoutBufferExportDetail),
}

pub fn classify_live_renderer_image_import(
    result: Result<bool, sophia_renderer_live::LiveRendererScanoutBufferExportDetail>,
) -> LiveRendererImageImport {
    use sophia_renderer_live::LiveRendererScanoutBufferExportDetail as D;
    match result {
        Ok(true) => LiveRendererImageImport::Placed,
        Ok(false) => LiveRendererImageImport::Existing,
        Err(D::DmaBufImageCreateFailed | D::DmaBufImageBindFailed | D::DmaBufImportFailed) => {
            LiveRendererImageImport::Refused
        }
        // Store-full may also be pooled storage still charged behind GPU
        // completions the renderer reports as busy; what remains full waits.
        Err(D::RendererImageStoreFull) => LiveRendererImageImport::Deferred { busy: false },
        Err(D::RendererImageTransferBusy | D::WorkerPending | D::WorkerQueueFull) => {
            LiveRendererImageImport::Deferred { busy: true }
        }
        Err(detail) => LiveRendererImageImport::Failed(detail),
    }
}

/// How one attempt to import an image into one store ended, after every
/// snapshot the import names was offered to it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveRendererImageStoreAttempt {
    Placed,
    /// The store's device refused every snapshot offered.
    Refused,
    /// Not now: `busy` is GPU work still in flight; otherwise no room.
    Deferred {
        busy: bool,
    },
}

/// What carrying out a plan left behind.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LiveRendererImageRestoreExecution {
    /// (store, image) for every import a store kept.
    pub held: BTreeSet<(usize, LiveRendererImageId)>,
    /// Demanded or planned destinations that do not hold their image.
    pub missing: BTreeSet<(usize, LiveRendererImageId)>,
    /// Some image no store kept was deferred behind GPU work in flight at
    /// one of the stores tried for it, so it is retried when that work
    /// settles rather than only when storage changes. An image a store kept
    /// leaves no such obligation.
    pub busy: bool,
}

impl LiveRendererImageRestoreExecution {
    pub fn placed(&self, image: LiveRendererImageId) -> bool {
        self.held.iter().any(|(_, held)| *held == image)
    }
}

/// Carries out `plan`. `attempt(import, store)` imports `import.image` into
/// `store`. A demanded destination is fixed: it is where the image is
/// sampled. A destination the plan only chose to keep the image in may be
/// fuller than planned, as the plan assumes free stores, so the other stores
/// are tried in turn, same device first, until one keeps it
/// (REVIEW-CODEX-06 R2). Whether any store tried was busy is kept for every
/// image that ends unplaced (REVIEW-CODEX-08).
pub fn execute_live_renderer_image_restore_plan<D: Copy + Eq, E>(
    plan: &LiveRendererImageRestorePlan,
    stores: &[LiveRendererImageRestoreStore<D>],
    demand: &BTreeSet<(usize, LiveRendererImageId)>,
    mut attempt: impl FnMut(
        &LiveRendererImageRestoreImport,
        usize,
    ) -> Result<LiveRendererImageStoreAttempt, E>,
) -> Result<LiveRendererImageRestoreExecution, E> {
    let mut execution = LiveRendererImageRestoreExecution {
        missing: plan.refused_demand.iter().copied().collect(),
        ..LiveRendererImageRestoreExecution::default()
    };
    let mut busy = BTreeSet::new();
    for import in &plan.imports {
        let mut order = vec![import.store];
        if !demand.contains(&(import.store, import.image)) {
            let device = stores[import.store].device;
            let mut others = (0..stores.len())
                .filter(|other| {
                    *other != import.store && !execution.held.contains(&(*other, import.image))
                })
                .collect::<Vec<_>>();
            others.sort_by_key(|other| {
                let same = matches!((stores[*other].device, device), (Some(a), Some(b)) if a == b);
                (!same, *other)
            });
            order.extend(others);
        }
        let mut kept = None;
        for store in order {
            match attempt(import, store)? {
                LiveRendererImageStoreAttempt::Placed => {
                    kept = Some(store);
                    break;
                }
                LiveRendererImageStoreAttempt::Refused => {}
                LiveRendererImageStoreAttempt::Deferred { busy: true } => {
                    busy.insert(import.image);
                }
                LiveRendererImageStoreAttempt::Deferred { busy: false } => {}
            }
        }
        match kept {
            Some(store) => {
                execution.held.insert((store, import.image));
            }
            None => {
                execution.missing.insert((import.store, import.image));
            }
        }
    }
    execution.busy = busy.iter().any(|image| !execution.placed(*image));
    Ok(execution)
}
