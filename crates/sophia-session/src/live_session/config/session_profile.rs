#[derive(Clone, Debug, PartialEq)]
pub(super) struct PreparedSessionProfile {
    slot: sophia_config::DesktopProfileCandidateSlot<sophia_config::DesktopSessionCandidate>,
}

impl PreparedSessionProfile {
    pub(super) fn new(
        candidate: sophia_config::DesktopSessionCandidate,
    ) -> Result<Self, sophia_config::DesktopProfileCandidateSlotError> {
        Ok(Self {
            slot: sophia_config::DesktopProfileCandidateSlot::with_candidate(candidate)?,
        })
    }

    /// The session profile startup is running: the prepared candidate until
    /// a window manager's startup activates it, the active profile after.
    /// Activation installs the candidate itself as active, so both name one
    /// payload; the phase only says which field holds it. Component prepare
    /// reads this after WM activation, which the former Prepared-only
    /// assertion turned into a panic in every debug session with a WM.
    pub(super) fn candidate(&self) -> &sophia_config::DesktopSessionCandidate {
        let slot = self.slot();
        match slot.participant().phase() {
            sophia_config::DesktopProfileParticipantPhase::Activated => slot.active(),
            _ => slot.candidate(),
        }
        .expect("trusted startup retains its session profile")
    }

    pub(super) const fn slot(
        &self,
    ) -> &sophia_config::DesktopProfileCandidateSlot<sophia_config::DesktopSessionCandidate> {
        &self.slot
    }

    pub(super) const fn slot_mut(
        &mut self,
    ) -> &mut sophia_config::DesktopProfileCandidateSlot<sophia_config::DesktopSessionCandidate>
    {
        &mut self.slot
    }
}
