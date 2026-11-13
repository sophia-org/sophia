//! A captured frame image is private until its exact Presented is transferred.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FrameImage {
    Held(ContentRetireOwner),
    Abandoned,
}

/// What the store holds for one resource against one exact frame, for the
/// combined lane's controls (G3-5b F1). Reading it makes no lease and reads
/// no pixels; a private frame image stays unleasable until its Presented.
#[cfg(any(test, feature = "experimental-combined-frame-job"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameImageResidency {
    /// Accepted and privately held for exactly this frame.
    HeldForFrame,
    /// Accepted and privately held for another frame.
    HeldForOther,
    /// Accepted; its frame's failed terminal abandoned it.
    Abandoned,
    /// Accepted and public: its frame was presented, or it had none.
    Public,
    /// No accepted generation under this grant.
    Absent,
}

impl ContentResourceStore {
    /// The control read behind `FrameImageResidency`.
    #[cfg(any(test, feature = "experimental-combined-frame-job"))]
    pub fn frame_image_residency(
        &self,
        owner: ContentRetireOwner,
        resource: ContentResourceId,
    ) -> FrameImageResidency {
        let value = match self.accepted.get(&resource) {
            Some(value) if !self.revoked && owner.grant == self.limits.grant => value,
            _ => return FrameImageResidency::Absent,
        };
        match value.frame {
            Some(FrameImage::Held(held)) if held == owner => FrameImageResidency::HeldForFrame,
            Some(FrameImage::Held(_)) => FrameImageResidency::HeldForOther,
            Some(FrameImage::Abandoned) => FrameImageResidency::Abandoned,
            None => FrameImageResidency::Public,
        }
    }

    pub(crate) fn bind_frame_copy(
        &mut self,
        resource: ContentResourceId,
        owner: ContentRetireOwner,
    ) {
        let transfer = self
            .transfers
            .get_mut(&resource)
            .expect("reserved frame copy");
        assert_eq!(owner.grant, self.limits.grant, "copy owner grant");
        assert!(transfer.frame.is_none(), "copy already bound");
        transfer.frame = Some(owner);
    }

    pub(crate) fn lease_frame_image(
        &self,
        owner: ContentRetireOwner,
        resource: ContentResourceId,
    ) -> Result<ContentResourceLease, ContentStoreError> {
        Ok(self.frame_cpu_image(owner, resource)?.pixels.clone())
    }

    fn frame_cpu_image(
        &self,
        owner: ContentRetireOwner,
        resource: ContentResourceId,
    ) -> Result<&Accepted, ContentStoreError> {
        let value = self.leaseable(owner.grant, resource)?;
        if value.frame.is_some() && value.frame != Some(FrameImage::Held(owner)) {
            return Err(ContentStoreError::Stale);
        }
        if value.pixels.cpu_bytes().is_none() {
            return Err(ContentStoreError::Malformed);
        }
        Ok(value)
    }

    pub(crate) fn validate_frame_cpu_layers(
        &self,
        owner: ContentRetireOwner,
        placements: &[sophia_protocol::ContentPlacement],
        new_image: Option<ContentResourceId>,
        predecessor: Option<ContentResourceId>,
    ) -> Result<(), ContentStoreError> {
        for placement in placements {
            if Some(placement.resource) != new_image {
                self.frame_cpu_image(owner, placement.resource)?;
            }
        }
        if let Some(resource) = predecessor {
            self.frame_cpu_image(owner, resource)?;
        }
        Ok(())
    }

    pub(crate) fn validate_frame_images(
        &self,
        owner: ContentRetireOwner,
        used: &std::collections::BTreeSet<ContentResourceId>,
    ) -> Result<(), ContentStoreError> {
        if self.accepted.iter().any(|(resource, value)| {
            value.frame == Some(FrameImage::Held(owner)) && !used.contains(resource)
        }) {
            return Err(ContentStoreError::Malformed);
        }
        Ok(())
    }

    /// Only called by the terminal owner after its publication succeeds.
    pub(crate) fn finish_frame_images(&mut self, outcome: &ContentCandidateOutcome) {
        let owner = ContentRetireOwner {
            grant: outcome.grant,
            output: outcome.output,
            candidate_generation: outcome.candidate_generation,
        };
        for value in self.accepted.values_mut() {
            if value.frame == Some(FrameImage::Held(owner)) {
                value.frame = if outcome.kind == 2 {
                    None
                } else {
                    // Failed private storage stays resident-charged until its last lease.
                    Some(FrameImage::Abandoned)
                };
            }
        }
        self.collect();
    }
}
