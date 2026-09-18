//! Session placement for parentless native transient surfaces.
use super::*;
use sophia_protocol::{
    ContentAllocationId, ContentAllocationRequest, ContentLogicalRect, NativeLauncherOpening,
};
use sophia_runtime::ContentAllocationError as Error;

impl LiveContentSession {
    pub(in crate::live_session::metadata_shell::content) fn resolve_native_allocation(
        &mut self,
        request: &ContentAllocationRequest,
        opening: NativeLauncherOpening,
        outputs: &[HeadlessOutput],
    ) -> Result<ContentAllocationSnapshot, Error> {
        if request.grant != opening.grant
            || request.output != opening.output
            || request.role != 3
            || !matches!(request.operation, 1 | 2)
            || request.parent != ContentAllocationId::default()
            || request.anchor_parent_rect != sophia_protocol::ContentPixelRect::default()
        {
            return Err(Error::Stale);
        }
        let output = outputs
            .iter()
            .find(|v| v.id.raw() == opening.output.id)
            .copied()
            .ok_or(Error::OutputLost)?;
        let facts = output_facts_entry(output).map_err(|_| Error::Malformed)?;
        if facts.output != opening.output {
            return Err(Error::Stale);
        }
        let width = i32::try_from(request.desired_width).map_err(|_| Error::Malformed)?;
        let height = i32::try_from(request.desired_height).map_err(|_| Error::Malformed)?;
        let [left, top, right, bottom] = [
            request.margins.left,
            request.margins.top,
            request.margins.right,
            request.margins.bottom,
        ]
        .map(i32::from);
        if width <= 0 || height <= 0 || [left, top, right, bottom].iter().any(|v| *v < 0) {
            return Err(Error::Malformed);
        }
        let spare_x = i32::try_from(facts.local_width)
            .map_err(|_| Error::Malformed)?
            .checked_sub(left)
            .and_then(|v| v.checked_sub(right))
            .and_then(|v| v.checked_sub(width))
            .filter(|v| *v >= 0)
            .ok_or(Error::Malformed)?;
        let spare_y = i32::try_from(facts.local_height)
            .map_err(|_| Error::Malformed)?
            .checked_sub(top)
            .and_then(|v| v.checked_sub(bottom))
            .and_then(|v| v.checked_sub(height))
            .filter(|v| *v >= 0)
            .ok_or(Error::Malformed)?;
        let (x, y) = match request.edge {
            1 => (left + spare_x / 2, top),
            2 => (left + spare_x, top + spare_y / 2),
            3 => (left + spare_x / 2, top + spare_y),
            4 => (left, top + spare_y / 2),
            _ => return Err(Error::Malformed),
        };
        let logical = ContentLogicalRect {
            x,
            y,
            width: request.desired_width,
            height: request.desired_height,
        };
        let pixel = quantize(logical, output.scale).ok_or(Error::Malformed)?;
        let allocation = if request.operation == 1 {
            let id = self.next_allocation_id;
            self.next_allocation_id = id.checked_add(1).ok_or(Error::Budget)?;
            ContentAllocationId { id, generation: 1 }
        } else {
            ContentAllocationId {
                id: request.prior.id,
                generation: request
                    .prior
                    .generation
                    .checked_add(1)
                    .ok_or(Error::Budget)?,
            }
        };
        Ok(ContentAllocationSnapshot {
            native_opening: Some(opening.opening),
            output: opening.output,
            allocation,
            scale_generation: facts.scale_generation,
            scale_numerator: facts.scale_numerator,
            scale_denominator: facts.scale_denominator,
            role: 3,
            edge: request.edge,
            margins: request.margins,
            logical,
            pixel,
            parent: ContentAllocationId::default(),
            anchor_parent_rect: sophia_protocol::ContentPixelRect::default(),
            allowed_reservation_extent: 0,
        })
    }
}
