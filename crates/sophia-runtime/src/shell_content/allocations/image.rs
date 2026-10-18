//! Descriptor admission still belongs to the live output/allocation grant.
use super::*;
use sophia_protocol::shell_files::{SHELL_IMAGE_SOURCE_SEALED_MEMORY, ShellImageMetadata};

impl ContentAllocationStore {
    pub(crate) fn isb_image_description(
        &self,
        metadata: &ShellImageMetadata,
    ) -> Result<ContentResourceBegin, ContentAllocationError> {
        self.check_grant(self.limits.grant)?;
        metadata
            .encode()
            .map_err(|_| ContentAllocationError::Malformed)?;
        if metadata.admission_epoch != self.limits.grant.content_grant_epoch
            || metadata.topology_generation != self.facts_generation
        {
            return Err(ContentAllocationError::Stale);
        }
        if metadata.source_kind != SHELL_IMAGE_SOURCE_SEALED_MEMORY {
            return Err(ContentAllocationError::Malformed);
        }
        let allocation = self
            .active
            .values()
            .find(|allocation| allocation.allocation.id == metadata.allocation_id)
            .ok_or(ContentAllocationError::AllocationLost)?;
        if allocation.output.id != metadata.output_id
            || !self
                .outputs
                .iter()
                .any(|output| output.output == allocation.output)
        {
            return Err(ContentAllocationError::OutputLost);
        }
        let row = metadata
            .width
            .checked_mul(4)
            .ok_or(ContentAllocationError::Malformed)?;
        let rows = self
            .limits
            .max_chunk_bytes
            .checked_div(row)
            .filter(|rows| *rows != 0)
            .ok_or(ContentAllocationError::Malformed)?;
        // Retained resources use the existing description/lease format.
        // Chunk count describes layout only; no upload fid is allocated.
        let description = ContentResourceBegin {
            grant: self.limits.grant,
            resource: ContentResourceId {
                id: metadata.resource,
                generation: metadata.resource_generation,
            },
            width_px: metadata.width,
            height_px: metadata.height,
            rendered_scale_numerator: allocation.scale_numerator,
            rendered_scale_denominator: allocation.scale_denominator,
            pixel_format: 1, // Isb metadata validation above requires tight BGRA8.
            chunk_count: metadata.height.div_ceil(rows),
            total_bytes: metadata.declared_bytes,
        };
        description
            .layout(&self.limits)
            .map_err(|_| ContentAllocationError::Malformed)?;
        Ok(description)
    }
}
