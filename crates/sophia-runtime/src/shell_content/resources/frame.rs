//! A captured frame image is private until its exact Presented is transferred.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FrameImage {
    Held(ContentRetireOwner),
    Abandoned,
}

impl ContentResourceStore {
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
        let value = self.leaseable(owner.grant, resource)?;
        if value.frame.is_some() && value.frame != Some(FrameImage::Held(owner)) {
            return Err(ContentStoreError::Stale);
        }
        Ok(value.pixels.clone())
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
