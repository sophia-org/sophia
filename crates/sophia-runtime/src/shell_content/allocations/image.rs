//! Descriptor admission still belongs to the live output/allocation grant.
use super::*;
use sophia_protocol::shell_files::{SHELL_IMAGE_SOURCE_SEALED_MEMORY, ShellImageMetadata};

impl ContentAllocationStore {
    #[cfg(feature = "experimental-owned-copy")]
    pub(crate) fn owned_copy_description(
        &self,
        request: &crate::OwnedCopyImageRequest,
    ) -> Result<ContentResourceBegin, ContentAllocationError> {
        self.check_grant(self.limits.grant)?;
        if request.topology_generation != self.facts_generation {
            return Err(ContentAllocationError::Stale);
        }
        let allocation = self
            .active
            .values()
            .find(|a| a.allocation == request.allocation)
            .ok_or(ContentAllocationError::AllocationLost)?;
        if allocation.output != request.output
            || !self.outputs.iter().any(|o| o.output == request.output)
        {
            return Err(ContentAllocationError::OutputLost);
        }
        let row = request
            .width
            .checked_mul(4)
            .ok_or(ContentAllocationError::Malformed)?;
        let rows = self
            .limits
            .max_chunk_bytes
            .checked_div(row)
            .filter(|n| *n != 0)
            .ok_or(ContentAllocationError::Malformed)?;
        let description = ContentResourceBegin {
            grant: self.limits.grant,
            resource: request.resource,
            width_px: request.width,
            height_px: request.height,
            rendered_scale_numerator: allocation.scale_numerator,
            rendered_scale_denominator: allocation.scale_denominator,
            pixel_format: 1,
            chunk_count: request.height.div_ceil(rows),
            total_bytes: u64::from(row) * u64::from(request.height),
        };
        description
            .layout(&self.limits)
            .map_err(|_| ContentAllocationError::Malformed)?;
        Ok(description)
    }

    #[cfg(feature = "experimental-shell-capture")]
    pub(crate) fn capture_image_allocation(
        &self,
        metadata: &ShellImageMetadata,
    ) -> Result<ContentAllocationSnapshot, ContentAllocationError> {
        self.capture_image_description(metadata)?;
        self.active
            .values()
            .find(|entry| entry.allocation.id == metadata.allocation_id)
            .cloned()
            .ok_or(ContentAllocationError::AllocationLost)
    }
    pub(crate) fn isb_image_description(
        &self,
        metadata: &ShellImageMetadata,
    ) -> Result<ContentResourceBegin, ContentAllocationError> {
        self.image_description(metadata, SHELL_IMAGE_SOURCE_SEALED_MEMORY)
    }

    #[cfg(feature = "experimental-shell-capture")]
    pub(crate) fn capture_image_description(
        &self,
        metadata: &ShellImageMetadata,
    ) -> Result<ContentResourceBegin, ContentAllocationError> {
        if metadata.source_format != sophia_protocol::DRM_FORMAT_XRGB8888
            || metadata.plane_count != 1
            || metadata.descriptor_count != 1
        {
            return Err(ContentAllocationError::Malformed);
        }
        self.image_description(
            metadata,
            sophia_protocol::shell_files::SHELL_IMAGE_SOURCE_DMA_BUF,
        )
    }

    fn image_description(
        &self,
        metadata: &ShellImageMetadata,
        source_kind: u32,
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
        if metadata.source_kind != source_kind {
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
            // Logical four-byte pixel layout, not a source-read or import proof.
            pixel_format: 1,
            chunk_count: metadata.height.div_ceil(rows),
            total_bytes: metadata.declared_bytes,
        };
        description
            .layout(&self.limits)
            .map_err(|_| ContentAllocationError::Malformed)?;
        Ok(description)
    }
}
