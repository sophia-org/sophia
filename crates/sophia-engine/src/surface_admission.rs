use crate::prelude::*;
use sophia_protocol::{
    LayoutNodeKind, Rect, SurfaceConstraints, SurfaceId, SurfacePlacementPreference,
    SurfacePresentationIntent, SurfacePresentationIntentKind, SurfacePresentationRole,
    TransactionId,
};

/// Passive facts required to plan a surface before it has committed pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceLayoutFacts {
    pub surface: SurfaceId,
    pub role: SurfacePresentationRole,
    pub kind: LayoutNodeKind,
    pub placement_preference: SurfacePlacementPreference,
    pub presentation_owner: Option<SurfaceId>,
    pub stack_rank: u32,
    pub geometry: Rect,
    pub constraints: SurfaceConstraints,
    pub generation: u64,
}

impl From<SurfacePresentationIntent> for SurfaceLayoutFacts {
    fn from(intent: SurfacePresentationIntent) -> Self {
        Self {
            surface: intent.surface,
            role: intent.role,
            kind: intent.surface_kind,
            placement_preference: intent.placement_preference,
            presentation_owner: intent.presentation_owner,
            stack_rank: intent.stack_rank,
            geometry: intent.geometry,
            constraints: intent.constraints,
            generation: intent.generation,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SurfacePresentationAdmissionState {
    #[default]
    Inactive,
    PolicyPending,
    ControlPending {
        transaction: TransactionId,
        geometry: Rect,
    },
    AwaitingPixels {
        transaction: TransactionId,
        geometry: Rect,
    },
    AwaitingRetirement {
        admission_transaction: TransactionId,
        visual_candidate: SurfaceTransactionKey,
        geometry: Rect,
    },
    Managed,
}

/// Cold-path, protocol-neutral state for presentation admission.
///
/// The table owns lifecycle facts only. Buffer and fence ownership remains in
/// the presentation backend, and WM policy remains outside this reducer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SurfaceAdmissionTable {
    facts: BTreeMap<SurfaceId, SurfaceLayoutFacts>,
    states: BTreeMap<SurfaceId, SurfacePresentationAdmissionState>,
}

impl SurfaceAdmissionTable {
    pub fn observe_intent(&mut self, intent: SurfacePresentationIntent) -> bool {
        match intent.kind {
            SurfacePresentationIntentKind::Request => {
                let facts = SurfaceLayoutFacts::from(intent);
                let changed = self.facts.get(&intent.surface) != Some(&facts)
                    || self.state(intent.surface) == SurfacePresentationAdmissionState::Inactive;
                self.facts.insert(intent.surface, facts);
                self.states.insert(
                    intent.surface,
                    SurfacePresentationAdmissionState::PolicyPending,
                );
                changed
            }
            SurfacePresentationIntentKind::Withdraw => self.remove(intent.surface),
        }
    }

    pub fn begin_control(
        &mut self,
        surface: SurfaceId,
        transaction: TransactionId,
        geometry: Rect,
    ) -> bool {
        if !matches!(
            self.state(surface),
            SurfacePresentationAdmissionState::PolicyPending
        ) || geometry.is_empty()
        {
            return false;
        }
        self.states.insert(
            surface,
            SurfacePresentationAdmissionState::ControlPending {
                transaction,
                geometry,
            },
        );
        true
    }

    pub fn acknowledge_control(&mut self, surface: SurfaceId, transaction: TransactionId) -> bool {
        let SurfacePresentationAdmissionState::ControlPending {
            transaction: expected,
            geometry,
        } = self.state(surface)
        else {
            return false;
        };
        if transaction != expected {
            return false;
        }
        self.states.insert(
            surface,
            SurfacePresentationAdmissionState::AwaitingPixels {
                transaction,
                geometry,
            },
        );
        true
    }

    pub fn mark_managed(&mut self, surface: SurfaceId) -> bool {
        if !matches!(
            self.state(surface),
            SurfacePresentationAdmissionState::AwaitingPixels { .. }
        ) {
            return false;
        }
        self.states
            .insert(surface, SurfacePresentationAdmissionState::Managed);
        true
    }

    pub fn begin_retirement(
        &mut self,
        surface: SurfaceId,
        visual_candidate: SurfaceTransactionKey,
    ) -> bool {
        let SurfacePresentationAdmissionState::AwaitingPixels {
            transaction: admission_transaction,
            geometry,
        } = self.state(surface)
        else {
            return false;
        };
        if !visual_candidate.transaction.is_valid() || visual_candidate.surface != surface {
            return false;
        }
        self.states.insert(
            surface,
            SurfacePresentationAdmissionState::AwaitingRetirement {
                admission_transaction,
                visual_candidate,
                geometry,
            },
        );
        true
    }

    pub fn complete_retirement(&mut self, visual_candidate: SurfaceTransactionKey) -> bool {
        let SurfacePresentationAdmissionState::AwaitingRetirement {
            visual_candidate: expected,
            ..
        } = self.state(visual_candidate.surface)
        else {
            return false;
        };
        if visual_candidate != expected {
            return false;
        }
        self.states.insert(
            visual_candidate.surface,
            SurfacePresentationAdmissionState::Managed,
        );
        true
    }

    /// A terminally skipped frame cannot satisfy admission. Preserve the
    /// acknowledged policy placement while waiting for replacement pixels.
    pub fn reject_retirement(&mut self, candidate: SurfaceTransactionKey) -> bool {
        let SurfacePresentationAdmissionState::AwaitingRetirement {
            admission_transaction,
            visual_candidate,
            geometry,
        } = self.state(candidate.surface)
        else {
            return false;
        };
        if visual_candidate != candidate {
            return false;
        }
        self.states.insert(
            candidate.surface,
            SurfacePresentationAdmissionState::AwaitingPixels {
                transaction: admission_transaction,
                geometry,
            },
        );
        true
    }

    pub fn facts(&self, surface: SurfaceId) -> Option<SurfaceLayoutFacts> {
        self.facts.get(&surface).copied()
    }

    pub fn state(&self, surface: SurfaceId) -> SurfacePresentationAdmissionState {
        self.states.get(&surface).copied().unwrap_or_default()
    }

    pub fn pending_surfaces(&self) -> impl Iterator<Item = SurfaceId> + '_ {
        self.states.iter().filter_map(|(surface, state)| {
            (!matches!(
                state,
                SurfacePresentationAdmissionState::Inactive
                    | SurfacePresentationAdmissionState::Managed
            ))
            .then_some(*surface)
        })
    }

    pub fn remove(&mut self, surface: SurfaceId) -> bool {
        let facts = self.facts.remove(&surface).is_some();
        let state = self.states.remove(&surface).is_some();
        facts || state
    }
}
