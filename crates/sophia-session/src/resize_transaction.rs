use std::collections::BTreeMap;

use sophia_engine::{SafeSurfaceObservation, SurfacePresentationAdmissionState};
use sophia_protocol::{LayerSnapshot, Size, SurfaceId, SurfaceTransactionKey, TransactionId};
use sophia_x_authority::XAuthorityObservedTransactionBatch;

pub use sophia_engine::{
    LayoutEpochCoordinator as ResizeRollbackCoordinator,
    LayoutRecoveryConfigure as ResizeRollbackRequest,
};

pub const RESIZE_VISUAL_COMMIT_CAPACITY: usize = 256;

/// Pure decision for keeping admission recovery constraints aligned with the
/// exact visual candidate that can cross the quarantine boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionRecoveryExtentDecision {
    Ineligible,
    AwaitingCandidate,
    Unchanged {
        selected: SafeSurfaceObservation,
    },
    Update {
        previous: Option<Size>,
        selected: SafeSurfaceObservation,
    },
    ClearStale {
        previous: Size,
    },
}

/// Reduces passive admission facts without mutating either owning state table.
///
/// A safe observation is not sufficient by itself: admission can use it only
/// while its exact transaction remains retained in the pre-admission queue.
/// Once native retirement owns a candidate, later observations cannot move the
/// recovery constraint underneath that in-flight frame.
pub fn decide_admission_recovery_extent(
    admission: SurfacePresentationAdmissionState,
    selected: Option<SafeSurfaceObservation>,
    selected_candidate_retained: bool,
    current: Option<Size>,
) -> AdmissionRecoveryExtentDecision {
    if !matches!(
        admission,
        SurfacePresentationAdmissionState::PolicyPending
            | SurfacePresentationAdmissionState::ControlPending { .. }
            | SurfacePresentationAdmissionState::AwaitingPixels { .. }
    ) {
        return AdmissionRecoveryExtentDecision::Ineligible;
    }

    let Some(selected) = selected
        .filter(|observation| observation.candidate.is_some() && selected_candidate_retained)
    else {
        return current.map_or(
            AdmissionRecoveryExtentDecision::AwaitingCandidate,
            |previous| AdmissionRecoveryExtentDecision::ClearStale { previous },
        );
    };

    if current == Some(selected.extent) {
        AdmissionRecoveryExtentDecision::Unchanged { selected }
    } else {
        AdmissionRecoveryExtentDecision::Update {
            previous: current,
            selected,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResizeVisualCommit {
    pub candidate: SurfaceTransactionKey,
    /// Exact buffer extent that native retirement must report.
    pub size: Size,
    /// Logical surface extent made durable by that retirement.
    pub layout_size: Size,
}

/// Bounded visual candidates selected by a layout epoch but not yet committed
/// by the asynchronous presentation backend.
#[derive(Debug, Default)]
pub struct ResizeVisualCommitTracker {
    awaiting: BTreeMap<(TransactionId, SurfaceId), ResizeVisualCommit>,
}

impl ResizeVisualCommitTracker {
    pub fn arm(&mut self, candidate: ResizeVisualCommit) -> Result<(), &'static str> {
        if !candidate.candidate.transaction.is_valid() || !candidate.candidate.surface.is_valid() {
            return Err("resize visual commit candidate has an invalid identity");
        }
        if candidate.size.width <= 0 || candidate.size.height <= 0 {
            return Err("resize visual commit candidate has an invalid size");
        }
        if candidate.layout_size.width <= 0 || candidate.layout_size.height <= 0 {
            return Err("resize visual commit candidate has an invalid layout size");
        }
        let key = (candidate.candidate.transaction, candidate.candidate.surface);
        if self.awaiting.contains_key(&key) {
            return Err("resize visual commit candidate identity is duplicated");
        }
        if self.awaiting.len() == RESIZE_VISUAL_COMMIT_CAPACITY {
            return Err("resize visual commit candidate capacity exceeded");
        }
        self.awaiting.insert(key, candidate);
        Ok(())
    }

    pub fn surface_awaiting(&self, surface: SurfaceId) -> bool {
        self.awaiting
            .values()
            .any(|candidate| candidate.candidate.surface == surface)
    }

    /// Returns whether this surface already has a bounded successor for the
    /// same logical layout target. A recovery frame and its successor may both
    /// retire, but repeated Presents for one target must not grow the tracker.
    pub fn surface_layout_awaiting(&self, surface: SurfaceId, layout_size: Size) -> bool {
        self.awaiting.values().any(|candidate| {
            candidate.candidate.surface == surface && candidate.layout_size == layout_size
        })
    }

    pub fn exact_candidate(&self, candidate: SurfaceTransactionKey, size: Size) -> bool {
        self.awaiting
            .get(&(candidate.transaction, candidate.surface))
            .is_some_and(|awaiting| awaiting.candidate == candidate && awaiting.size == size)
    }

    pub fn complete(
        &mut self,
        candidate: SurfaceTransactionKey,
        size: Size,
    ) -> Option<ResizeVisualCommit> {
        let key = (candidate.transaction, candidate.surface);
        let awaiting = self.awaiting.get(&key).copied()?;
        if awaiting.candidate != candidate || awaiting.size != size {
            return None;
        }
        self.awaiting.remove(&key)
    }

    pub fn remove_surface(&mut self, surface: SurfaceId) -> usize {
        let before = self.awaiting.len();
        self.awaiting
            .retain(|_, candidate| candidate.candidate.surface != surface);
        before.saturating_sub(self.awaiting.len())
    }

    pub fn reject(&mut self, candidate: SurfaceTransactionKey) -> bool {
        let key = (candidate.transaction, candidate.surface);
        if self
            .awaiting
            .get(&key)
            .is_none_or(|value| value.candidate != candidate)
        {
            return false;
        }
        self.awaiting.remove(&key);
        true
    }

    pub fn len(&self) -> usize {
        self.awaiting.len()
    }

    pub fn is_empty(&self) -> bool {
        self.awaiting.is_empty()
    }
}

/// Projects authority pixels onto the current layout without dropping any
/// generation-bearing transaction or associated buffer update.
///
/// Placement only. The authority's two extents -- what its raster spans and
/// what it was presented into -- are measurements it owns, and a layout that
/// moved underneath them does not change either one.
pub fn project_authority_batch_onto_layout(
    mut batch: XAuthorityObservedTransactionBatch,
    layers: &BTreeMap<SurfaceId, LayerSnapshot>,
) -> XAuthorityObservedTransactionBatch {
    for transaction in &mut batch.transactions {
        if let Some(layer) = layers.get(&transaction.surface) {
            transaction.target_geometry = layer.geometry;
        }
    }
    batch
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PendingLayoutObservationMerge {
    Inserted,
    Merged,
    ResizeOwned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PendingLayoutGeometryAuthority {
    Layout,
    Observation,
}

/// Keeps authority observations that are outside a pending resize proposal in
/// the proposal snapshot. Without this merge, committing an older proposal
/// can discard a surface admitted while its resize pixels were pending.
pub fn merge_unrequested_layout_observation(
    pending_layers: &mut Vec<LayerSnapshot>,
    requested_sizes: &BTreeMap<SurfaceId, Size>,
    observed: LayerSnapshot,
    geometry_authority: PendingLayoutGeometryAuthority,
) -> PendingLayoutObservationMerge {
    if requested_sizes.contains_key(&observed.surface) {
        return PendingLayoutObservationMerge::ResizeOwned;
    }
    match pending_layers
        .iter_mut()
        .find(|layer| layer.surface == observed.surface)
    {
        Some(layer) => {
            if geometry_authority == PendingLayoutGeometryAuthority::Observation {
                layer.geometry = observed.geometry;
            }
            layer.authority_local_id = observed.authority_local_id;
            layer.namespace = observed.namespace;
            layer.source = observed.source;
            layer.damage = observed.damage;
            layer.opacity = observed.opacity;
            layer.crop = observed.crop;
            layer.transform = observed.transform;
            layer.generation = observed.generation;
            layer.resize_sync = observed.resize_sync;
            PendingLayoutObservationMerge::Merged
        }
        None => {
            pending_layers.push(observed);
            PendingLayoutObservationMerge::Inserted
        }
    }
}
