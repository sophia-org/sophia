//! DMA-BUF capture offers beside the owned ring. The first phase admits one
//! LINEAR XRGB8888 plane on an advertised output row. This module reserves
//! logical and journal credit and keeps descriptor custody until the capture
//! owner takes it; it never decides source-read completion.
use super::image_offers::Offer;
use super::*;
use crate::{
    ContentCaptureAdmissionError, ContentCaptureReservation, ContentEpochRegistry,
    ShellCaptureImageSource,
};
use std::os::fd::OwnedFd;

const STATUS_BYTES: usize = SHELL_FILE_HEADER_BYTES + 56;
const RELEASE_BYTES: usize = SHELL_FILE_HEADER_BYTES + 32;

/// Per-offer charges against the published DMA-BUF limits group.
#[derive(Clone, Copy, Debug)]
pub(super) struct CaptureCharge {
    /// The destination bound the caller reserved before admission.
    allocation_bound: u64,
    /// Declared bytes until the Source arrives, then its exact `lseek` size.
    pub(super) source_bytes: u64,
    plane: Option<ShellImagePlane>,
}

impl CaptureCharge {
    /// `available` is the source budget left after every other live offer.
    pub(super) fn receive(
        &mut self,
        metadata: &ShellImageMetadata,
        plane: ShellImagePlane,
        source: &OwnedFd,
        available: u64,
    ) -> Result<(), Errno> {
        let row = u64::from(metadata.width) * 4;
        if plane.fd_index != 0 || u64::from(plane.stride) < row {
            return Err(Errno::EPROTO);
        }
        // LINEAR only: the last row ends at offset + stride*(h-1) + width*4.
        // Nonlinear sizes are never guessed.
        let span = u64::from(plane.stride)
            .checked_mul(u64::from(metadata.height) - 1)
            .and_then(|bytes| bytes.checked_add(row))
            .and_then(|bytes| bytes.checked_add(plane.offset))
            .ok_or(Errno::EPROTO)?;
        let size =
            rustix::fs::seek(source, rustix::fs::SeekFrom::End(0)).map_err(|_| Errno::EPROTO)?;
        if span > size || size > available {
            return Err(Errno::EPROTO);
        }
        self.source_bytes = size;
        self.plane = Some(plane);
        Ok(())
    }
}

impl ShellFiles {
    fn capture_ready(&self) -> bool {
        matches!(
            self.image_readiness(),
            ShellImageReadiness::Ready { granted_source_kinds }
                if granted_source_kinds & SHELL_IMAGE_SOURCE_DMA_BUF != 0
        )
    }

    pub(in crate::shell_transport) fn enable_capture_image_offers(&mut self) -> Result<(), Errno> {
        if !self.capture_ready() {
            return Err(Errno::EACCES);
        }
        self.image.as_mut().ok_or(Errno::EACCES)?.offers.enabled = true;
        Ok(())
    }

    /// The oldest queued offer when it is DMA-BUF, with the deadline fixed
    /// when its ImageBegin was accepted.
    pub(in crate::shell_transport) fn pending_capture_image_offer(
        &self,
    ) -> Option<(ShellImageMetadata, Instant)> {
        let metadata = self
            .pending_image_offer()
            .filter(|metadata| metadata.source_kind == SHELL_IMAGE_SOURCE_DMA_BUF)?;
        let deadline = self
            .image
            .as_ref()?
            .offers
            .queued_deadline(metadata.offer_id)?;
        Some((metadata, deadline))
    }

    /// Every check and journal hold precedes the logical reservation, which
    /// is the last fallible step before ImageAdmitted commits. Any failure
    /// leaves the ImageBegin queued with nothing reserved.
    pub(in crate::shell_transport) fn admit_capture_image_offer(
        &mut self,
        grant: ContentGrant,
        epochs: &mut ContentEpochRegistry,
        allocation_bound: u64,
        now: u64,
    ) -> Result<(ShellImageMetadata, ContentCaptureReservation, Instant), Errno> {
        if !self.capture_ready() {
            return Err(Errno::EACCES);
        }
        let (metadata, deadline) = self.pending_capture_image_offer().ok_or(Errno::EAGAIN)?;
        let granted = match self.image_readiness() {
            ShellImageReadiness::Ready {
                granted_source_kinds,
            } => granted_source_kinds,
            _ => unreachable!("capture readiness checked"),
        };
        let state = self.image.as_mut().ok_or(Errno::EACCES)?;
        if grant.connection_epoch != self.epoch
            || metadata.admission_epoch != grant.content_grant_epoch
            || metadata.offer_id <= state.offers.high_water
        {
            return Err(Errno::ESTALE);
        }
        let limits = state.config.limits;
        metadata
            .admissible(&ShellImageExpect {
                role: 1,
                connection_epoch: self.epoch,
                channel_generation: state.config.discovery.channel_generation,
                granted_source_kinds: granted,
                max_width: limits.max_width,
                max_height: limits.max_height,
                max_resource_bytes: limits.max_resource_bytes,
            })
            .map_err(|_| Errno::EINVAL)?;
        if metadata.source_format != sophia_protocol::DRM_FORMAT_XRGB8888
            || metadata.modifier != 0
            || metadata.plane_count != 1
            || !state.config.formats.rows.iter().any(|row| {
                row.output == metadata.output_id
                    && row.fourcc == metadata.source_format
                    && row.modifier == metadata.modifier
                    && metadata.width <= row.max_width
                    && metadata.height <= row.max_height
            })
            // declared_bytes is the codec-fixed logical w*h*4, not the source
            // layout; the descriptor is charged separately at its real size.
            || allocation_bound < metadata.declared_bytes
            || allocation_bound > limits.max_renderer_allocation_bytes
            || metadata.declared_bytes > limits.max_dma_source_bytes
        {
            return Err(Errno::EINVAL);
        }
        let live = || state.offers.live.iter().flatten().filter_map(|o| o.capture);
        let entries = live().count() as u64;
        let allocated = live().map(|c| c.allocation_bound).sum::<u64>();
        let sources = live().map(|c| c.source_bytes).sum::<u64>();
        if entries >= u64::from(limits.max_capture_jobs.min(limits.max_renderer_entries))
            || allocated + allocation_bound > limits.max_renderer_allocation_bytes
            || sources + metadata.declared_bytes > limits.max_dma_source_bytes
        {
            return Err(Errno::EAGAIN);
        }
        let slot = state.offers.live[..limits.max_offers as usize]
            .iter()
            .position(Option::is_none)
            .ok_or(Errno::EAGAIN)?;
        let body = metadata.encode().map_err(|_| Errno::EINVAL)?;
        let status = self
            .journal
            .hold_reservation(STATUS_BYTES)
            .ok_or(Errno::EAGAIN)?;
        let Some(release) = self.journal.hold_reservation(RELEASE_BYTES) else {
            self.journal.release_reservation(status);
            return Err(Errno::EAGAIN);
        };
        let admitted = (|| {
            let prepared =
                self.journal
                    .prepare_append(ShellFileKind::ImageAdmitted, &body, true)?;
            let reservation = epochs
                .reserve_capture_image(grant, &metadata, now)
                .map_err(|error| match error {
                    ContentCaptureAdmissionError::Resource(crate::ContentStoreError::Budget) => {
                        Errno::EAGAIN
                    }
                    ContentCaptureAdmissionError::Resource(
                        crate::ContentStoreError::Stale | crate::ContentStoreError::Revoked,
                    )
                    | ContentCaptureAdmissionError::Allocation(
                        crate::ContentAllocationError::AllocationLost,
                    ) => Errno::ESTALE,
                    _ => Errno::EINVAL,
                })?;
            prepared.commit();
            Ok(reservation)
        })();
        let reservation = match admitted {
            Ok(reservation) => reservation,
            Err(error) => {
                self.journal.release_reservation(status);
                self.journal.release_reservation(release);
                return Err(error);
            }
        };
        let state = self.image.as_mut().expect("image state checked");
        state.offers.live[slot] = Some(Offer {
            metadata,
            job: None,
            capture: Some(CaptureCharge {
                allocation_bound,
                source_bytes: metadata.declared_bytes,
                plane: None,
            }),
            status: Some(status),
            release: Some(release),
            received: false,
            source: None,
        });
        state.offers.high_water = metadata.offer_id;
        state.offers.dequeue(metadata.offer_id);
        self.inbound.pop_front();
        Ok((metadata, reservation, deadline))
    }

    /// A received source leaves export custody exactly once. Its offer slot
    /// and charges remain until SourceReleased is journaled.
    pub(in crate::shell_transport) fn take_capture_image_source(
        &mut self,
    ) -> Option<ShellCaptureImageSource> {
        self.image
            .as_mut()?
            .offers
            .live
            .iter_mut()
            .flatten()
            .filter(|offer| offer.job.is_none())
            .find_map(|offer| {
                let charge = offer.capture?;
                let plane = charge.plane?;
                Some(ShellCaptureImageSource {
                    metadata: offer.metadata,
                    plane,
                    source_bytes: charge.source_bytes,
                    fd: offer.source.take()?,
                })
            })
    }
}
