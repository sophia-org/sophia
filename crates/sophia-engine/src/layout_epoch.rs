use crate::prelude::*;
use sophia_protocol::SurfaceConstraints;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutConstraintError {
    InvalidBounds,
    InvalidConstraints { surface: SurfaceId },
    ExtentExceedsBounds { surface: SurfaceId, size: Size },
    GeometryOverflow { surface: SurfaceId },
}

impl fmt::Display for LayoutConstraintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBounds => formatter.write_str("layout constraint bounds are empty"),
            Self::InvalidConstraints { surface } => write!(
                formatter,
                "surface {}:{} declared invalid layout constraints",
                surface.index(),
                surface.generation(),
            ),
            Self::ExtentExceedsBounds { surface, size } => write!(
                formatter,
                "surface {}:{} constrained extent {}x{} exceeds output bounds",
                surface.index(),
                surface.generation(),
                size.width,
                size.height,
            ),
            Self::GeometryOverflow { surface } => write!(
                formatter,
                "surface {}:{} constrained geometry overflowed",
                surface.index(),
                surface.generation(),
            ),
        }
    }
}

impl std::error::Error for LayoutConstraintError {}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutConstraintReconciliation {
    pub transaction: LayoutTransaction,
    pub adjusted_surfaces: Vec<SurfaceId>,
}

/// Protocol-neutral configure request produced while recovering an abandoned
/// layout epoch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LayoutRecoveryConfigure {
    pub transaction: TransactionId,
    pub surface: SurfaceId,
    pub size: Size,
}

/// Passive admission state for a policy-managed surface.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SurfaceAdmissionState {
    #[default]
    Unmanaged,
    PendingLayout,
    Managed,
}

/// Protocol-neutral quality of a complete authority-side visual observation.
///
/// A backing snapshot is sufficient for software-only clients. A presented
/// buffer is stronger admission evidence because it is the client's complete
/// frame rather than an authority-maintained backing image.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SurfaceVisualEvidence {
    BackingSnapshot,
    PresentedBuffer,
}

/// Protocol-neutral relationship between a complete pixel extent and the
/// geometry an outstanding layout transition is trying to establish.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceVisualExtentDisposition {
    Unconstrained,
    Expected,
    RetainedRecovery,
    Mismatch,
}

/// Classifies pixels without mutating either committed geometry or the
/// standing layout target. A retained recovery extent is coherent content
/// that may continue changing while a client converges on `expected`.
pub const fn classify_surface_visual_extent(
    actual: Option<Size>,
    expected: Option<Size>,
    retained_recovery: Option<Size>,
) -> SurfaceVisualExtentDisposition {
    let Some(expected) = expected else {
        return SurfaceVisualExtentDisposition::Unconstrained;
    };
    match actual {
        Some(actual) if actual.width == expected.width && actual.height == expected.height => {
            SurfaceVisualExtentDisposition::Expected
        }
        Some(actual)
            if matches!(
                retained_recovery,
                Some(retained)
                    if actual.width == retained.width && actual.height == retained.height
            ) =>
        {
            SurfaceVisualExtentDisposition::RetainedRecovery
        }
        Some(_) => SurfaceVisualExtentDisposition::Mismatch,
        None => SurfaceVisualExtentDisposition::Unconstrained,
    }
}

/// Latest safe visual extent selected by the Engine's evidence reducer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SafeSurfaceObservation {
    pub candidate: Option<SurfaceTransactionKey>,
    pub extent: Size,
    pub evidence: SurfaceVisualEvidence,
    pub sequence: u64,
}

/// Declared and temporary constraints are stored separately so recovery never
/// mutates application truth.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceConstraintState {
    pub declared: SurfaceConstraints,
    pub recovery_extent: Option<Size>,
}

impl SurfaceConstraintState {
    pub fn effective(self) -> SurfaceConstraints {
        self.recovery_extent
            .map_or(self.declared, |size| SurfaceConstraints {
                min_size: Some(size),
                max_size: Some(size),
            })
    }

    pub const fn resizable(self) -> bool {
        self.recovery_extent.is_none() && self.declared_resizable()
    }

    pub const fn declared_resizable(self) -> bool {
        !matches!(
            (self.declared.min_size, self.declared.max_size),
            (Some(minimum), Some(maximum)) if minimum.width == maximum.width
                && minimum.height == maximum.height
        )
    }
}

/// Engine-owned state for joining authority content sizes to blind-WM layout
/// epochs. The record contains no X11 identifiers or application metadata.
#[derive(Debug)]
pub struct LayoutEpochCoordinator {
    committed_sizes: BTreeMap<SurfaceId, Size>,
    safe_observations: BTreeMap<SurfaceId, SafeSurfaceObservation>,
    // Strongest complete visual path observed for each surface. Once a client
    // has explicitly presented complete buffers, a passive backing snapshot
    // may remain useful recovery evidence but cannot complete a later resize.
    visual_evidence_requirements: BTreeMap<SurfaceId, SurfaceVisualEvidence>,
    rollback_sizes: BTreeMap<SurfaceId, Size>,
    rollback_transactions: BTreeMap<SurfaceId, TransactionId>,
    rejected_sizes: BTreeMap<SurfaceId, Size>,
    constraints: BTreeMap<SurfaceId, SurfaceConstraintState>,
    admission: BTreeMap<SurfaceId, SurfaceAdmissionState>,
    // Standing size a surface must still converge on after an aborted layout
    // epoch admitted it at a different extent. A first-launch client can be
    // shown at whatever it first rendered while this obligation drives it
    // toward the blind-WM target; it clears once the surface commits a
    // buffer at that exact target.
    pending_targets: BTreeMap<SurfaceId, Size>,
    /// How many times each standing target has been re-driven without the
    /// surface converging on it. Cleared with the target itself, so a live entry
    /// always counts attempts against an obligation that is still outstanding.
    standing_redrives: BTreeMap<SurfaceId, u32>,
    next_observation_sequence: u64,
    next_transaction: u64,
}

impl Default for LayoutEpochCoordinator {
    fn default() -> Self {
        Self {
            committed_sizes: BTreeMap::new(),
            safe_observations: BTreeMap::new(),
            visual_evidence_requirements: BTreeMap::new(),
            rollback_sizes: BTreeMap::new(),
            rollback_transactions: BTreeMap::new(),
            rejected_sizes: BTreeMap::new(),
            constraints: BTreeMap::new(),
            admission: BTreeMap::new(),
            pending_targets: BTreeMap::new(),
            standing_redrives: BTreeMap::new(),
            next_observation_sequence: 1,
            next_transaction: 1 << 63,
        }
    }
}

impl LayoutEpochCoordinator {
    /// Reconciles a blind-WM proposal with Engine-owned content constraints
    /// before any client configure is emitted.
    pub fn reconcile_transaction(
        &self,
        transaction: &LayoutTransaction,
        bounds: Rect,
    ) -> Result<LayoutConstraintReconciliation, LayoutConstraintError> {
        if bounds.is_empty() {
            return Err(LayoutConstraintError::InvalidBounds);
        }
        let bounds_right = bounds
            .x
            .checked_add(bounds.width)
            .ok_or(LayoutConstraintError::InvalidBounds)?;
        let bounds_bottom = bounds
            .y
            .checked_add(bounds.height)
            .ok_or(LayoutConstraintError::InvalidBounds)?;
        let mut transaction = transaction.clone();
        let mut adjusted = BTreeSet::new();

        for request in &mut transaction.requested_sizes {
            let reconciled = self.constrained_size(request.surface, request.size, bounds)?;
            if reconciled != request.size {
                request.size = reconciled;
                adjusted.insert(request.surface);
            }
        }

        for placement in &mut transaction.render_positions {
            let proposed = Size {
                width: placement.geometry.width,
                height: placement.geometry.height,
            };
            let reconciled = self.constrained_size(placement.surface, proposed, bounds)?;
            let max_x = bounds_right.checked_sub(reconciled.width).ok_or(
                LayoutConstraintError::GeometryOverflow {
                    surface: placement.surface,
                },
            )?;
            let max_y = bounds_bottom.checked_sub(reconciled.height).ok_or(
                LayoutConstraintError::GeometryOverflow {
                    surface: placement.surface,
                },
            )?;
            // Reposition only a surface we resized. Growing one to its
            // minimum can push it past the edge, and moving it back is the
            // only way to honour both the minimum and the bounds -- that is
            // what this clamp is for.
            //
            // A surface whose size was accepted keeps the position it was
            // given, outside these bounds included. A scrolling layout puts
            // columns past the edge on purpose, and clamping them back does
            // not keep anything safe: it silently rewrites the layout and
            // lands a new column on top of one already on screen.
            let resized =
                reconciled.width != proposed.width || reconciled.height != proposed.height;
            let geometry = Rect {
                x: if resized {
                    placement.geometry.x.clamp(bounds.x, max_x)
                } else {
                    placement.geometry.x
                },
                y: if resized {
                    placement.geometry.y.clamp(bounds.y, max_y)
                } else {
                    placement.geometry.y
                },
                width: reconciled.width,
                height: reconciled.height,
            };
            if geometry != placement.geometry {
                placement.geometry = geometry;
                adjusted.insert(placement.surface);
            }

            if let Some(request) = transaction
                .requested_sizes
                .iter_mut()
                .find(|request| request.surface == placement.surface)
            {
                if request.size != reconciled {
                    request.size = reconciled;
                    adjusted.insert(placement.surface);
                }
            } else if self.committed_size(placement.surface) != Some(reconciled) {
                transaction
                    .requested_sizes
                    .push(sophia_protocol::SurfaceSizeRequest {
                        surface: placement.surface,
                        size: reconciled,
                    });
                adjusted.insert(placement.surface);
            }
        }

        Ok(LayoutConstraintReconciliation {
            transaction,
            adjusted_surfaces: adjusted.into_iter().collect(),
        })
    }

    pub fn committed_size(&self, surface: SurfaceId) -> Option<Size> {
        self.committed_sizes.get(&surface).copied()
    }

    pub fn record_committed(&mut self, surface: SurfaceId, size: Size) {
        self.committed_sizes.insert(surface, size);
        let _ = self.reduce_safe_observation(
            surface,
            None,
            size,
            SurfaceVisualEvidence::BackingSnapshot,
        );
        if self.rejected_sizes.get(&surface) == Some(&size) {
            self.rejected_sizes.remove(&surface);
        }
        // A surface that commits its outstanding target has converged; the
        // standing re-drive obligation is discharged.
        if self.pending_targets.get(&surface) == Some(&size) {
            self.pending_targets.remove(&surface);
            self.standing_redrives.remove(&surface);
        }
    }

    /// Records a size a surface must still converge on after an aborted layout
    /// epoch admitted it at a different extent. No-op when it already matches
    /// the committed size (nothing to drive).
    pub fn set_pending_target(&mut self, surface: SurfaceId, target: Size) {
        if self.committed_sizes.get(&surface) == Some(&target) {
            self.pending_targets.remove(&surface);
            self.standing_redrives.remove(&surface);
        } else {
            // A different target is a new obligation, so its attempts start over
            // rather than inheriting the previous one's.
            if self.pending_targets.insert(surface, target) != Some(target) {
                self.standing_redrives.remove(&surface);
            }
        }
    }

    /// Counts one attempt at driving a surface to its standing target.
    ///
    /// A standing target is re-injected into every proposal until the surface
    /// commits at exactly that size, and a surface that commits at some other
    /// size never clears it. That loop is idempotent and was therefore invisible:
    /// a browser was reconfigured five times at identical geometry with nothing
    /// saying so. Counting the attempts is what separates "converging" from
    /// "repeating", which the geometry alone cannot.
    pub fn note_standing_redrive(&mut self, surface: SurfaceId) -> u32 {
        let attempts = self.standing_redrives.entry(surface).or_insert(0);
        *attempts = attempts.saturating_add(1);
        *attempts
    }

    /// Attempts made against a surface's outstanding target, zero if none.
    pub fn standing_redrives(&self, surface: SurfaceId) -> u32 {
        self.standing_redrives.get(&surface).copied().unwrap_or(0)
    }

    pub fn pending_target(&self, surface: SurfaceId) -> Option<Size> {
        self.pending_targets.get(&surface).copied()
    }

    pub fn clear_pending_target(&mut self, surface: SurfaceId) -> bool {
        self.standing_redrives.remove(&surface);
        self.pending_targets.remove(&surface).is_some()
    }

    pub fn pending_target_surfaces(&self) -> impl Iterator<Item = (SurfaceId, Size)> + '_ {
        self.pending_targets
            .iter()
            .map(|(surface, size)| (*surface, *size))
    }

    pub fn pending_target_count(&self) -> usize {
        self.pending_targets.len()
    }

    /// Records a complete authority buffer extent without claiming that its
    /// pixels have entered committed visual state.
    pub fn record_safe_observation(
        &mut self,
        candidate: SurfaceTransactionKey,
        extent: Size,
        evidence: SurfaceVisualEvidence,
    ) -> bool {
        self.reduce_safe_observation(candidate.surface, Some(candidate), extent, evidence)
    }

    fn reduce_safe_observation(
        &mut self,
        surface: SurfaceId,
        candidate: Option<SurfaceTransactionKey>,
        extent: Size,
        evidence: SurfaceVisualEvidence,
    ) -> bool {
        self.visual_evidence_requirements
            .entry(surface)
            .and_modify(|required| *required = (*required).max(evidence))
            .or_insert(evidence);
        let sequence = self.next_observation_sequence;
        self.next_observation_sequence = self.next_observation_sequence.saturating_add(1);
        let candidate = SafeSurfaceObservation {
            candidate,
            extent,
            evidence,
            sequence,
        };
        let admission_pending = self.admission(surface) != SurfaceAdmissionState::Managed;
        let replace = self.safe_observations.get(&surface).is_none_or(|current| {
            if admission_pending {
                candidate.evidence >= current.evidence
            } else {
                candidate.sequence > current.sequence
            }
        });
        if replace {
            self.safe_observations.insert(surface, candidate);
        }
        replace
    }

    pub fn safe_size(&self, surface: SurfaceId) -> Option<Size> {
        self.safe_observation(surface)
            .map(|observation| observation.extent)
    }

    pub fn safe_observation(&self, surface: SurfaceId) -> Option<SafeSurfaceObservation> {
        self.safe_observations.get(&surface).copied()
    }

    /// Invalidates only the terminal candidate, never a newer observation.
    pub fn reject_safe_observation(&mut self, candidate: SurfaceTransactionKey) -> bool {
        if self
            .safe_observation(candidate.surface)
            .is_none_or(|observed| observed.candidate != Some(candidate))
        {
            return false;
        }
        self.safe_observations.remove(&candidate.surface);
        true
    }

    pub fn required_visual_evidence(&self, surface: SurfaceId) -> SurfaceVisualEvidence {
        self.visual_evidence_requirements
            .get(&surface)
            .copied()
            .unwrap_or(SurfaceVisualEvidence::BackingSnapshot)
    }

    /// Returns whether an exact-size observation is strong enough to complete
    /// a resize for this surface. The requirement is monotonic for the
    /// surface lifetime so passive backing maintenance cannot demote an active
    /// presentation path.
    pub fn resize_evidence_allowed(
        &self,
        surface: SurfaceId,
        evidence: SurfaceVisualEvidence,
    ) -> bool {
        evidence >= self.required_visual_evidence(surface)
    }

    pub fn request_allowed(&self, surface: SurfaceId, size: Size) -> bool {
        self.rejected_sizes.get(&surface) != Some(&size)
    }

    pub fn accept_observation(&mut self, surface: SurfaceId, size: Size) -> bool {
        let Some(expected) = self.rollback_sizes.get(&surface).copied() else {
            return true;
        };
        if size != expected {
            return false;
        }
        self.rollback_sizes.remove(&surface);
        self.rollback_transactions.remove(&surface);
        self.rejected_sizes.remove(&surface);
        true
    }

    pub fn begin_recovery(
        &mut self,
        requests: impl IntoIterator<Item = (SurfaceId, Size)>,
        fixed_surfaces: impl IntoIterator<Item = SurfaceId>,
    ) -> Result<Vec<LayoutRecoveryConfigure>, &'static str> {
        let fixed = fixed_surfaces
            .into_iter()
            .map(|surface| {
                self.safe_size(surface)
                    .map(|extent| (surface, extent))
                    .ok_or("layout recovery surface has no safe content extent")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let sizes = requests
            .into_iter()
            .map(|(surface, rejected)| {
                self.safe_size(surface)
                    .map(|size| (surface, rejected, size))
                    .ok_or("layout recovery surface has no committed authority size")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let transaction = TransactionId::from_raw(self.next_transaction);
        self.next_transaction = self
            .next_transaction
            .checked_add(1)
            .ok_or("layout recovery transaction ID exhausted")?;
        for (surface, extent) in fixed {
            self.set_recovery_extent(surface, extent);
            self.admission
                .insert(surface, SurfaceAdmissionState::PendingLayout);
        }
        Ok(sizes
            .into_iter()
            .map(|(surface, rejected, size)| {
                self.rejected_sizes.insert(surface, rejected);
                self.rollback_sizes.insert(surface, size);
                self.rollback_transactions.insert(surface, transaction);
                LayoutRecoveryConfigure {
                    transaction,
                    surface,
                    size,
                }
            })
            .collect())
    }

    pub fn begin_rollback(
        &mut self,
        requests: impl IntoIterator<Item = (SurfaceId, Size)>,
    ) -> Result<Vec<LayoutRecoveryConfigure>, &'static str> {
        self.begin_recovery(requests, [])
    }

    pub fn set_declared_constraints(&mut self, surface: SurfaceId, declared: SurfaceConstraints) {
        self.constraints
            .entry(surface)
            .and_modify(|state| state.declared = declared)
            .or_insert(SurfaceConstraintState {
                declared,
                recovery_extent: None,
            });
    }

    /// A successful authority configure acknowledgement completes the
    /// compensating-control fence when safe pixels at that extent are already
    /// retained. A redraw is not required merely to unblock the blind-WM
    /// recovery replan.
    pub fn acknowledge_recovery_configure(
        &mut self,
        transaction: TransactionId,
        surface: SurfaceId,
    ) -> bool {
        if self.rollback_transactions.get(&surface) != Some(&transaction) {
            return false;
        }
        self.rollback_transactions.remove(&surface);
        self.rollback_sizes.remove(&surface);
        self.rejected_sizes.remove(&surface);
        true
    }

    pub fn set_recovery_extent(&mut self, surface: SurfaceId, extent: Size) {
        self.constraints
            .entry(surface)
            .and_modify(|state| state.recovery_extent = Some(extent))
            .or_insert(SurfaceConstraintState {
                declared: SurfaceConstraints {
                    min_size: None,
                    max_size: None,
                },
                recovery_extent: Some(extent),
            });
    }

    pub fn effective_constraints(&self, surface: SurfaceId) -> SurfaceConstraints {
        self.constraints.get(&surface).map_or(
            SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            |state| state.effective(),
        )
    }

    /// Returns application-declared constraints without Engine-owned recovery
    /// extents. External policy peers consume this view so temporary visual
    /// recovery never changes the client's policy identity.
    pub fn declared_constraints(&self, surface: SurfaceId) -> SurfaceConstraints {
        self.constraints.get(&surface).map_or(
            SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            |state| state.declared,
        )
    }

    pub fn surface_resizable(&self, surface: SurfaceId) -> bool {
        self.constraints
            .get(&surface)
            .is_none_or(|state| state.resizable())
    }

    pub fn surface_declared_resizable(&self, surface: SurfaceId) -> bool {
        self.constraints
            .get(&surface)
            .is_none_or(|state| state.declared_resizable())
    }

    pub fn recovery_extent(&self, surface: SurfaceId) -> Option<Size> {
        self.constraints
            .get(&surface)
            .and_then(|state| state.recovery_extent)
    }

    pub fn clear_recovery_extent(&mut self, surface: SurfaceId) -> bool {
        self.constraints.get_mut(&surface).is_some_and(|state| {
            let changed = state.recovery_extent.is_some();
            state.recovery_extent = None;
            changed
        })
    }

    /// Every surface currently holding a recovery extent.
    pub fn recovery_extent_surfaces(&self) -> impl Iterator<Item = SurfaceId> + '_ {
        self.constraints
            .iter()
            .filter(|(_, state)| state.recovery_extent.is_some())
            .map(|(surface, _)| *surface)
    }

    pub fn recovery_extent_count(&self) -> usize {
        self.constraints
            .values()
            .filter(|state| state.recovery_extent.is_some())
            .count()
    }

    pub fn set_admission(&mut self, surface: SurfaceId, state: SurfaceAdmissionState) {
        self.admission.insert(surface, state);
    }

    pub fn admission(&self, surface: SurfaceId) -> SurfaceAdmissionState {
        self.admission.get(&surface).copied().unwrap_or_default()
    }

    pub fn remove(&mut self, surface: SurfaceId) {
        self.committed_sizes.remove(&surface);
        self.safe_observations.remove(&surface);
        self.visual_evidence_requirements.remove(&surface);
        self.rollback_sizes.remove(&surface);
        self.rollback_transactions.remove(&surface);
        self.rejected_sizes.remove(&surface);
        self.constraints.remove(&surface);
        self.admission.remove(&surface);
        self.pending_targets.remove(&surface);
        self.standing_redrives.remove(&surface);
    }

    pub fn rollback_pending(&self, surface: SurfaceId) -> bool {
        self.rollback_sizes.contains_key(&surface)
    }

    pub fn rollback_surfaces(&self) -> impl Iterator<Item = SurfaceId> + '_ {
        self.rollback_sizes.keys().copied()
    }

    fn constrained_size(
        &self,
        surface: SurfaceId,
        proposed: Size,
        bounds: Rect,
    ) -> Result<Size, LayoutConstraintError> {
        // A recovery extent pins the surface to exactly the pixels the client
        // has already produced, so admission can show real content before the
        // blind WM drives final geometry. It is a best-effort aid, not
        // something the client asked for, and an output change can leave it
        // larger than the output the surface now lands on. Yield to the
        // client's declared constraints there: holding the pin instead makes
        // every proposal unsatisfiable and fails the session over pixels that
        // were only ever a courtesy.
        let constraints = self.constraints.get(&surface).map_or(
            SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            |state| {
                if state.recovery_extent.is_some_and(|extent| {
                    extent.width > bounds.width || extent.height > bounds.height
                }) {
                    return state.declared;
                }
                state.effective()
            },
        );
        let minimum = constraints.min_size.unwrap_or(Size {
            width: 1,
            height: 1,
        });
        let maximum = constraints.max_size.unwrap_or(Size {
            width: bounds.width,
            height: bounds.height,
        });
        if minimum.width <= 0
            || minimum.height <= 0
            || maximum.width <= 0
            || maximum.height <= 0
            || minimum.width > maximum.width
            || minimum.height > maximum.height
        {
            return Err(LayoutConstraintError::InvalidConstraints { surface });
        }
        let size = Size {
            width: proposed.width.clamp(minimum.width, maximum.width),
            height: proposed.height.clamp(minimum.height, maximum.height),
        };
        if size.width > bounds.width || size.height > bounds.height {
            return Err(LayoutConstraintError::ExtentExceedsBounds { surface, size });
        }
        Ok(size)
    }
}
