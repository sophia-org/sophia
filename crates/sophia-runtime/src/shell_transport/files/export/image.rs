//! Image-channel state for one attach: owner-configured immutable
//! snapshots, readiness published only by the owner, and the bounded
//! pending refusal an accepted `ImageBegin` leaves behind. This layer
//! defaults to refusing offers. The explicit resource-owner hooks in
//! `image_offers` can enable sealed admission after readiness. Neither path
//! uses an upload slot. Readiness supplied through
//! [`ShellFiles::image_channel_ready`] is the owner's statement (in
//! production, after its authenticated Hello); this file does not prove
//! that Hello happened.
use super::*;
use crate::shell_transport::files::journal::JournalReservation;

/// Owner-supplied image configuration. All three objects are validated
/// and cross-checked at configuration time; the export encodes the
/// immutable snapshot objects itself.
pub(in crate::shell_transport) struct ShellImageChannelConfig {
    pub discovery: ShellImageDiscovery,
    pub limits: ShellImageLimits,
    pub formats: ShellImageFormats,
    pub role_bounds: ShellImageRoleBounds,
}

/// Where the image channel stands for this attach. Configuration alone
/// never advances this; only negotiation disclosure plus the owner's
/// explicit ready/closed publications do.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::shell_transport) enum ShellImageReadiness {
    Disabled,
    AwaitingHello,
    Ready { granted_source_kinds: u32 },
    Closed { reason: ShellChannelClosedReason },
}

/// Borrowed image-channel facts for the owner's socket service.
pub(in crate::shell_transport) struct ShellImageAccess {
    pub connection_epoch: u64,
    pub channel_generation: u64,
    pub hello_timeout_ms: u32,
    pub granted_source_kinds: u32,
    pub readiness: ShellImageReadiness,
}

/// One refused offer whose `ResourceStatus` could not yet be journalled.
/// Bounded at one: while it is pending, further submissions are blocked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PendingImageRefusal {
    pub offer_id: u64,
    pub admission_epoch: u64,
    pub resource: ContentResourceId,
}

pub(super) struct ImageChannelState {
    pub config: ShellImageChannelConfig,
    pub discovery: Arc<super::image_topology::DiscoverySnapshot>,
    pub limits_body: Vec<u8>,
    pub formats_body: Vec<u8>,
    pub readiness: ShellImageReadiness,
    close_reservation: Option<JournalReservation>,
    pub offers: super::image_offers::ImageOffers,
}

/// Shell r9 capability bit 14, reused at r10 for the additive DMA-BUF opt-in.
const SHELL_CAPABILITY_DMA_BUF: u64 = 1 << 14;

/// Total bytes of one journalled `ChannelClosed` record (header + body).
const CHANNEL_CLOSED_RECORD_BYTES: usize = SHELL_FILE_HEADER_BYTES + 16;

impl ShellFiles {
    /// Recheck publication against the actual admitted content profile, not
    /// only the owner's provisional bounds supplied before negotiation.
    pub(super) fn validate_image_content_limits(
        &self,
        limits: &ContentLimits,
    ) -> Result<(), Errno> {
        let config = &self.image.as_ref().ok_or(Errno::EACCES)?.config;
        let bounds = ShellImageRoleBounds {
            max_resource_bytes: limits.max_resource_bytes,
            max_width: limits.max_width_px,
            max_height: limits.max_height_px,
            max_admitted_outputs: limits.max_outputs,
            transfer_slots: limits.max_open_transfers,
        };
        config
            .discovery
            .coherent(
                &config.limits,
                &config.formats,
                self.epoch,
                self.epoch,
                &bounds,
            )
            .map_err(|_| Errno::EINVAL)?;
        Ok(())
    }

    /// Prepares both snapshot records for disclosure without publishing
    /// anything: every encoding and both qids are settled first, so a
    /// failure leaves the export untouched.
    pub(super) fn prepare_image_objects(
        &mut self,
    ) -> Result<Vec<(ShellFileKind, u64, Vec<u8>)>, Errno> {
        let epoch = self.epoch;
        let (limits_body, formats_body) = {
            let state = self.image.as_ref().ok_or(Errno::EINVAL)?;
            (state.limits_body.clone(), state.formats_body.clone())
        };
        let mut prepared = Vec::with_capacity(2);
        for (kind, body) in [
            (ShellFileKind::ImageLimits, limits_body),
            (ShellFileKind::ImageFormats, formats_body),
        ] {
            let bytes = encode_shell_file_record_at_revision(
                ShellFileHeader {
                    kind,
                    connection_epoch: epoch,
                    submission_id: 0,
                    sequence: 0,
                },
                &body,
                crate::owned_ring_protocol::REVISION,
            )
            .map_err(|_| Errno::EINVAL)?;
            prepared.push((kind, bytes));
        }
        let mut with_qids = Vec::with_capacity(2);
        for (kind, bytes) in prepared {
            with_qids.push((kind, self.allocate_qid()?, bytes));
        }
        Ok(with_qids)
    }

    /// Installs prepared snapshot objects at disclosure. They are
    /// configured before negotiation. No `ObjectPublished` event is
    /// journalled: the root gains them
    /// atomically with negotiation, and each open pins its bytes.
    pub(super) fn install_image_objects(&mut self, prepared: Vec<(ShellFileKind, u64, Vec<u8>)>) {
        for (kind, qid, bytes) in prepared {
            let slot = self.object_slot_mut(kind).expect("image object kind");
            slot.current = Some(Arc::new(Object { kind, qid, bytes }));
        }
    }
}

impl ShellFiles {
    /// Configures (or disables, with `None`) the image channel for this
    /// attach. Pre-negotiation only. Configuration validates every object
    /// and the cross-object coherence, but discloses nothing by itself.
    pub(in crate::shell_transport) fn configure_image_channel(
        &mut self,
        config: Option<ShellImageChannelConfig>,
    ) -> Result<(), Errno> {
        if self.revoked {
            return Err(Errno::ESTALE);
        }
        if self.negotiated {
            return Err(EALREADY);
        }
        let Some(config) = config else {
            self.image = None;
            return Ok(());
        };
        let discovery = &config.discovery;
        if discovery.connection_epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        // Cross-object coherence from the vendored SDK: generations and
        // the five discovery/limits equalities must agree, source kinds
        // must match the capacity groups, and the limits must fit the
        // role bounds. The header epochs are this attach's epoch.
        discovery
            .coherent(
                &config.limits,
                &config.formats,
                self.epoch,
                self.epoch,
                &config.role_bounds,
            )
            .map_err(|_| Errno::EINVAL)?;
        let discovery_text = discovery.encode().map_err(|_| Errno::EINVAL)?;
        let limits_body = config.limits.encode().map_err(|_| Errno::EINVAL)?;
        let formats_body = config.formats.encode().map_err(|_| Errno::EINVAL)?;
        let discovery = Arc::new(super::image_topology::DiscoverySnapshot {
            qid: self.allocate_qid()?,
            text: discovery_text,
        });
        self.image = Some(ImageChannelState {
            config,
            discovery,
            limits_body,
            formats_body,
            readiness: ShellImageReadiness::AwaitingHello,
            close_reservation: None,
            offers: super::image_offers::ImageOffers::default(),
        });
        Ok(())
    }

    /// Whether the negotiated outcome discloses the image objects:
    /// configured, shell r10 selected, and owned-ring capability granted.
    pub(super) fn image_disclosed(&self) -> bool {
        self.image.is_some()
            && self.negotiated
            && self.image_revision == crate::owned_ring_protocol::REVISION
            && self.capabilities & crate::owned_ring_protocol::CAPABILITY != 0
    }

    /// Source kinds the negotiated capabilities name: always the ring, plus
    /// DMA-BUF only when capture was configured and its bit 14 was granted.
    pub(super) fn negotiated_source_kinds(&self) -> u32 {
        let owned = if self.capabilities & crate::owned_ring_protocol::CAPABILITY != 0 {
            crate::owned_ring_protocol::SOURCE_KIND
        } else {
            0
        };
        let dma = if owned != 0 && self.capabilities & SHELL_CAPABILITY_DMA_BUF != 0 {
            SHELL_IMAGE_SOURCE_DMA_BUF
        } else {
            0
        };
        owned | dma
    }

    pub(in crate::shell_transport) fn image_readiness(&self) -> ShellImageReadiness {
        match &self.image {
            Some(state) if self.image_disclosed() => state.readiness,
            // Configured but not disclosed is indistinguishable from
            // disabled to every caller: readiness is never inferred from
            // configuration alone.
            _ => ShellImageReadiness::Disabled,
        }
    }

    /// Borrowed channel facts for the owner's socket service: the epochs
    /// and generation a Hello must name, the Hello deadline, and the
    /// granted source-kind mask once ready. `None` until disclosure.
    pub(in crate::shell_transport) fn image_channel_access(&self) -> Option<ShellImageAccess> {
        let state = self.image.as_ref()?;
        if !self.image_disclosed() {
            return None;
        }
        Some(ShellImageAccess {
            connection_epoch: state.config.discovery.connection_epoch,
            channel_generation: state.config.discovery.channel_generation,
            hello_timeout_ms: state.config.discovery.hello_timeout_ms,
            granted_source_kinds: match state.readiness {
                ShellImageReadiness::Ready {
                    granted_source_kinds,
                } => granted_source_kinds,
                _ => state.config.discovery.source_kinds & self.negotiated_source_kinds(),
            },
            readiness: state.readiness,
        })
    }

    /// Publishes `ChannelReady` for the current channel generation. The
    /// owner calls this only after its authenticated Hello; the supplied
    /// generation fences stale callbacks from a successor channel. The
    /// granted mask must be nonzero and within the kinds the negotiated
    /// capabilities and the configuration both name. Before journaling,
    /// room for one future `ChannelClosed` record is held in the journal;
    /// readiness is refused while that room cannot be held.
    pub(in crate::shell_transport) fn image_channel_ready(
        &mut self,
        channel_generation: u64,
        granted_source_kinds: u32,
    ) -> Result<u64, Errno> {
        let state = self.image.as_ref().ok_or(Errno::EACCES)?;
        if !self.image_disclosed() || state.readiness != ShellImageReadiness::AwaitingHello {
            return Err(Errno::EACCES);
        }
        if channel_generation != state.config.discovery.channel_generation {
            return Err(Errno::ESTALE);
        }
        let configured = state.config.discovery.source_kinds;
        let negotiated = self.negotiated_source_kinds();
        if granted_source_kinds == 0 || granted_source_kinds & !(configured & negotiated) != 0 {
            return Err(Errno::EINVAL);
        }
        let reservation = self
            .journal
            .hold_reservation(CHANNEL_CLOSED_RECORD_BYTES)
            .ok_or(Errno::EAGAIN)?;
        let body = ShellChannelReady {
            channel_generation,
            granted_source_kinds,
        }
        .encode()
        .map_err(|_| Errno::EINVAL)?;
        let sequence = match self
            .journal
            .append(ShellFileKind::ChannelReady, &body, true)
        {
            Ok(sequence) => sequence,
            Err(error) => {
                self.journal.release_reservation(reservation);
                return Err(error);
            }
        };
        let state = self.image.as_mut().expect("image state checked");
        state.close_reservation = Some(reservation);
        state.readiness = ShellImageReadiness::Ready {
            granted_source_kinds,
        };
        Ok(sequence)
    }

    /// Publishes `ChannelClosed`, spending the reservation held since
    /// readiness. A second publication, or one for a stale generation,
    /// is refused and changes nothing.
    pub(in crate::shell_transport) fn image_channel_closed(
        &mut self,
        channel_generation: u64,
        reason: ShellChannelClosedReason,
    ) -> Result<u64, Errno> {
        let state = self.image.as_ref().ok_or(Errno::EACCES)?;
        if !self.image_disclosed()
            || matches!(
                state.readiness,
                ShellImageReadiness::Closed { .. } | ShellImageReadiness::Disabled
            )
        {
            return Err(Errno::EACCES);
        }
        if channel_generation != state.config.discovery.channel_generation {
            return Err(Errno::ESTALE);
        }
        let body = ShellChannelClosed {
            channel_generation,
            reason,
        }
        .encode()
        .map_err(|_| Errno::EINVAL)?;
        let sequence = self.journal.append_consuming_reservation(
            state.close_reservation.ok_or(Errno::EINVAL)?,
            ShellFileKind::ChannelClosed,
            &body,
        )?;
        let state = self.image.as_mut().expect("image state checked");
        state.close_reservation = None;
        state.readiness = ShellImageReadiness::Closed { reason };
        state.offers.close_sources();
        Ok(sequence)
    }

    /// Journals the unavailable refusal for one image offer: a
    /// `ResourceStatus` rejected with reason 13 under shell r9. Returns
    /// `false` when the journal has no room yet; the caller keeps the
    /// refusal pending and retries from journal progress.
    pub(super) fn publish_image_refusal(
        &mut self,
        refusal: PendingImageRefusal,
    ) -> Result<bool, Errno> {
        self.publish_image_refusal_reason(refusal, 13)
    }

    pub(super) fn publish_image_refusal_reason(
        &mut self,
        refusal: PendingImageRefusal,
        reason: u16,
    ) -> Result<bool, Errno> {
        self.image.as_ref().ok_or(Errno::EACCES)?;
        let record = ShellFileTransactionRecord {
            transaction: TransactionId::from_raw(refusal.offer_id),
            record: ShellContentRecord::ResourceStatus(sophia_protocol::ContentResourceStatus {
                grant: ContentGrant {
                    connection_epoch: self.epoch,
                    content_grant_epoch: refusal.admission_epoch,
                },
                resource: refusal.resource,
                status: 3,
                reason,
                next_ordinal: 0,
                admitted_bytes: 0,
            }),
        };
        let header = ShellFileHeader {
            kind: ShellFileKind::ResourceStatus,
            connection_epoch: self.epoch,
            submission_id: 0,
            sequence: 1,
        };
        let bytes = encode_shell_file_resource_status_at_revision(
            header,
            &record,
            crate::owned_ring_protocol::REVISION,
        )
        .map_err(|_| Errno::EINVAL)?;
        let body = &bytes[SHELL_FILE_HEADER_BYTES..];
        if !self
            .journal
            .fits(body.len() + SHELL_FILE_HEADER_BYTES, true)
        {
            return Ok(false);
        }
        self.journal
            .append(ShellFileKind::ResourceStatus, body, true)?;
        Ok(true)
    }

    /// Closes the epoch path after an authorized Hello failure or timeout
    /// before readiness. Unauthenticated or wrong-peer attempts never
    /// reach this hook, so they leave state unchanged; only the owner's
    /// live-authenticated failure verdict calls it. The closed state is
    /// effective immediately and can never be re-enabled (configuration
    /// is pre-negotiation only). Publication is a bounded pending event:
    /// it is attempted now and retried from ordinary service turns, even
    /// when the journal is full, because no Closed reservation is held
    /// before readiness.
    pub(in crate::shell_transport) fn image_channel_hello_failed(
        &mut self,
        channel_generation: u64,
        reason: ShellChannelClosedReason,
    ) -> Result<(), Errno> {
        let state = self.image.as_ref().ok_or(Errno::EACCES)?;
        if !self.image_disclosed() || state.readiness != ShellImageReadiness::AwaitingHello {
            return Err(Errno::EACCES);
        }
        if channel_generation != state.config.discovery.channel_generation {
            return Err(Errno::ESTALE);
        }
        let closed = ShellChannelClosed {
            channel_generation,
            reason,
        };
        self.pending_image_closed = Some(closed);
        self.image.as_mut().expect("image state checked").readiness =
            ShellImageReadiness::Closed { reason };
        let _ = self.retry_pending_image_closed();
        Ok(())
    }

    /// Retries the bounded pending `ChannelClosed`, if any, from owner
    /// service turns or journal progress.
    pub(in crate::shell_transport) fn retry_pending_image_closed(&mut self) -> Result<bool, Errno> {
        let Some(closed) = self.pending_image_closed else {
            return Ok(false);
        };
        let body = closed.encode().map_err(|_| Errno::EINVAL)?;
        if !self
            .journal
            .fits(body.len() + SHELL_FILE_HEADER_BYTES, true)
        {
            return Ok(false);
        }
        self.journal
            .append(ShellFileKind::ChannelClosed, &body, true)?;
        self.pending_image_closed = None;
        Ok(true)
    }

    /// Retries every bounded pending image publication (refusal, then
    /// closed). The socket service calls this from ordinary turns.
    pub(in crate::shell_transport) fn retry_pending_image_publications(
        &mut self,
    ) -> Result<bool, Errno> {
        let refusal = self.retry_pending_image_refusal()?;
        let closed = self.retry_pending_image_closed()?;
        Ok(refusal || closed)
    }

    /// Retries the bounded pending refusal, if any. Called from journal
    /// acknowledgement progress and owner service turns; emitting it is
    /// the only way further submissions unblock.
    pub(in crate::shell_transport) fn retry_pending_image_refusal(
        &mut self,
    ) -> Result<bool, Errno> {
        let Some(refusal) = self.pending_image_refusal else {
            return Ok(false);
        };
        if self.publish_image_refusal(refusal)? {
            self.pending_image_refusal = None;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
