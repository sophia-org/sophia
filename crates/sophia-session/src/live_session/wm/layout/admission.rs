impl PersistentLiveLayout {
    fn retained_admission_candidate(
        &self,
        surface: SurfaceId,
    ) -> Option<sophia_protocol::SurfaceTransactionKey> {
        let extent = self.layout_epochs.recovery_extent(surface)?;
        if self.surface_awaits_visual_candidate(surface) {
            return self.selected_pre_admission_transaction(surface, extent)
                .map(SurfaceTransaction::key);
        }
        // Once released to the renderer, the exact retirement obligation owns
        // the pixels. An intervening policy answer must not demand a new size
        // before that frame can complete either.
        match self.admissions.state(surface) {
            sophia_engine::SurfacePresentationAdmissionState::AwaitingRetirement {
                visual_candidate, ..
            } if self.awaiting_visual_commits.exact_candidate(visual_candidate, extent) => {
                Some(visual_candidate)
            }
            _ => None,
        }
    }

    fn surface_requires_admission(&self, surface: SurfaceId) -> bool {
        !self.bypass_policy_admission
            && self.presentation_roles.get(&surface)
                == Some(&sophia_protocol::SurfacePresentationRole::PolicyManaged)
            && !matches!(
                self.admissions.state(surface),
                sophia_engine::SurfacePresentationAdmissionState::Inactive
                    | sophia_engine::SurfacePresentationAdmissionState::Managed
            )
    }

    fn synchronize_admission_extent(
        &mut self,
        surface: SurfaceId,
    ) -> AdmissionRecoveryExtentDecision {
        let selected = self.layout_epochs.safe_observation(surface);
        let selected_candidate_retained = selected.is_some_and(|observation| {
            self.selected_pre_admission_transaction(surface, observation.extent)
                .is_some()
        });
        let decision = decide_admission_recovery_extent(
            self.admissions.state(surface),
            selected,
            selected_candidate_retained,
            self.layout_epochs.recovery_extent(surface),
        );
        match decision {
            AdmissionRecoveryExtentDecision::Update { previous, selected } => {
                self.layout_epochs
                    .set_recovery_extent(surface, selected.extent);
                self.layout_epochs.set_admission(
                    surface,
                    sophia_engine::SurfaceAdmissionState::PendingLayout,
                );
                if let Some(previous) = previous {
                    crate::session_println!(
                        "sophia_live_resize_epoch schema=3 status=admission_extent_rebased surface={} previous_width={} previous_height={} width={} height={} evidence={:?}",
                        surface.index(),
                        previous.width,
                        previous.height,
                        selected.extent.width,
                        selected.extent.height,
                        selected.evidence,
                    );
                } else {
                    crate::session_println!(
                        "sophia_live_resize_epoch schema=3 status=admission_extent_primed surface={} width={} height={}",
                        surface.index(),
                        selected.extent.width,
                        selected.extent.height,
                    );
                }
            }
            AdmissionRecoveryExtentDecision::ClearStale { .. } => {
                self.release_recovery_extent(surface, "admission_candidate_unavailable");
            }
            AdmissionRecoveryExtentDecision::Ineligible
            | AdmissionRecoveryExtentDecision::AwaitingCandidate
            | AdmissionRecoveryExtentDecision::Unchanged { .. } => {}
        }
        decision
    }
}
