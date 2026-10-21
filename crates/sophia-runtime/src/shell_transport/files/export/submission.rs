//! Submission custody: decoding a staged candidate into what the owners
//! receive, and image refusals published once custody transfers.
use super::*;

impl ShellFiles {
    /// Decodes a staged candidate into the value the owners receive and the
    /// image effect applied once custody transfers. The phase decides which
    /// kinds exist: Negotiate once before negotiation, content records only
    /// after a content grant.
    fn decode(
        &self,
        bytes: &[u8],
        kind: ShellFileKind,
    ) -> Result<(Option<Inbound>, SubmissionEffect), Errno> {
        let content = |decoded: Result<ShellFileTransactionRecord, ShellFilePayloadError>| {
            if !self.negotiated || !self.content {
                return Err(Errno::EACCES);
            }
            let value = decoded.map_err(|_| Errno::EINVAL)?;
            Ok(Inbound::Content(value.transaction, Box::new(value.record)))
        };
        let inbound = match kind {
            kind if shell_file_descriptor_max_bytes(kind).is_some() => {
                if !self.supports_descriptor_kind(kind) {
                    return Err(Errno::EACCES);
                }
                let value = decode_shell_file_descriptor(bytes, kind).map_err(|_| Errno::EINVAL)?;
                Inbound::Descriptor(Box::new(value))
            }
            ShellFileKind::Negotiate => {
                if self.negotiate_accepted {
                    return Err(EALREADY);
                }
                decode_shell_file_negotiate(bytes)
                    .map(Inbound::Negotiate)
                    .map_err(|_| Errno::EINVAL)?
            }
            ShellFileKind::AllocationRequest => {
                content(decode_shell_file_allocation_request(bytes))?
            }
            // Retired byte-upload records never acquire submission custody.
            ShellFileKind::ResourceBegin | ShellFileKind::ResourceEnd => {
                return Err(Errno::EINVAL);
            }
            ShellFileKind::ResourceCancel => content(decode_shell_file_resource_cancel(bytes))?,
            ShellFileKind::ResourceRetire => content(decode_shell_file_resource_retire(bytes))?,
            ShellFileKind::FrameDemand
            | ShellFileKind::FrameDemandCancel
            | ShellFileKind::ActionAck => content(decode_shell_file_transaction(bytes, kind))?,
            ShellFileKind::Candidate => {
                if !self.negotiated || !self.content {
                    return Err(Errno::EACCES);
                }
                let candidate = decode_shell_file_candidate(bytes).map_err(|_| Errno::EINVAL)?;
                Inbound::Candidate(Box::new(candidate))
            }
            ShellFileKind::NativeAllocationRequest => {
                if !self.negotiated || !self.content || !self.supports_native_launcher() {
                    return Err(Errno::EACCES);
                }
                let value = decode_shell_file_native_launcher_transaction(bytes, kind)
                    .map_err(|_| Errno::EINVAL)?;
                let ShellNativeLauncherRecord::AllocationRequest(request) = value.record else {
                    return Err(Errno::EINVAL);
                };
                Inbound::NativeAllocation(value.transaction, Box::new(request))
            }
            ShellFileKind::NativeInputAck => {
                if !self.negotiated || !self.content || !self.supports_native_launcher() {
                    return Err(Errno::EACCES);
                }
                let value = decode_shell_file_native_launcher_transaction(bytes, kind)
                    .map_err(|_| Errno::EINVAL)?;
                let ShellNativeLauncherRecord::InputAck(ack) = value.record else {
                    return Err(Errno::EINVAL);
                };
                Inbound::NativeInputAck(value.transaction, Box::new(ack))
            }
            ShellFileKind::NativeActivate => {
                if !self.negotiated || !self.content || !self.supports_native_launcher() {
                    return Err(Errno::EACCES);
                }
                let value = decode_shell_file_native_launcher_transaction(bytes, kind)
                    .map_err(|_| Errno::EINVAL)?;
                let ShellNativeLauncherRecord::Activate(activation) = value.record else {
                    return Err(Errno::EINVAL);
                };
                Inbound::NativeActivate(value.transaction, Box::new(activation))
            }
            ShellFileKind::NativeCandidate => {
                if !self.negotiated || !self.content || !self.supports_native_launcher() {
                    return Err(Errno::EACCES);
                }
                let candidate =
                    decode_shell_file_native_candidate(bytes).map_err(|_| Errno::EINVAL)?;
                Inbound::NativeCandidate(Box::new(candidate))
            }
            ShellFileKind::CatalogCandidate => {
                if !self.negotiated || !self.content || !self.supports_persistent_catalog() {
                    return Err(Errno::EACCES);
                }
                let candidate =
                    decode_shell_file_catalog_candidate(bytes).map_err(|_| Errno::EINVAL)?;
                Inbound::CatalogCandidate(Box::new(candidate))
            }
            ShellFileKind::CatalogActivate => {
                if !self.negotiated || !self.content || !self.supports_persistent_catalog() {
                    return Err(Errno::EACCES);
                }
                let value =
                    decode_shell_file_catalog_action(bytes, kind).map_err(|_| Errno::EINVAL)?;
                let ShellCatalogActionRecord::Activate(activation) = value.record else {
                    return Err(Errno::EINVAL);
                };
                Inbound::CatalogActivate(value.transaction, Box::new(activation))
            }
            ShellFileKind::IndicatorActivate => {
                if !self.negotiated || !self.supports_indicator_activation() {
                    return Err(Errno::EACCES);
                }
                let value =
                    decode_shell_file_indicator_activate(bytes).map_err(|_| Errno::EINVAL)?;
                Inbound::IndicatorActivate(value.transaction, Box::new(value.activation))
            }
            ShellFileKind::ImageBegin => return self.decode_image_begin(bytes),
            _ => return Err(Errno::EINVAL),
        };
        Ok((Some(inbound), SubmissionEffect::None))
    }

    /// Before readiness there is no custody. Explicit owner admission queues
    /// ImageBegin in wire order; otherwise readiness or closure produces the
    /// clean unavailable refusal without admitting a source.
    fn decode_image_begin(
        &self,
        bytes: &[u8],
    ) -> Result<(Option<Inbound>, SubmissionEffect), Errno> {
        if self.image_revision == crate::owned_ring_protocol::REVISION {
            return Err(Errno::EACCES);
        }
        if !self.image_disclosed() {
            return Err(Errno::EINVAL);
        }
        match self.image_readiness() {
            ShellImageReadiness::AwaitingHello | ShellImageReadiness::Disabled => {
                return Err(EBUSY);
            }
            ShellImageReadiness::Ready { .. } | ShellImageReadiness::Closed { .. } => {}
        }
        let body = bytes.get(SHELL_FILE_HEADER_BYTES..).ok_or(Errno::EINVAL)?;
        let metadata = ShellImageMetadata::decode(body).map_err(|_| Errno::EINVAL)?;
        if !self
            .image
            .as_ref()
            .is_some_and(|state| state.offers.accepts_id(metadata.offer_id))
        {
            return Err(Errno::ESTALE);
        }
        if self
            .content_limits
            .as_ref()
            .is_none_or(|limits| metadata.admission_epoch != limits.grant.content_grant_epoch)
        {
            return Err(Errno::ESTALE);
        }
        if matches!(self.image_readiness(), ShellImageReadiness::Ready { .. }) {
            let state = self.image.as_ref().expect("checked image state");
            // ChannelReady can reach the peer before the image owner's next
            // turn. Preserve custody and identities until that owner opts in.
            if !state.offers.enabled {
                return Err(Errno::EAGAIN);
            }
            if self
                .inbound
                .iter()
                .filter(|item| matches!(item, Inbound::ImageBegin(_)))
                .count()
                >= state.config.limits.max_offers as usize
            {
                return Err(Errno::EAGAIN);
            }
            return Ok((Some(Inbound::ImageBegin(metadata)), SubmissionEffect::None));
        }
        Ok((
            None,
            SubmissionEffect::PublishImageRefusal {
                refusal: PendingImageRefusal {
                    offer_id: metadata.offer_id,
                    admission_epoch: metadata.admission_epoch,
                    resource: ContentResourceId {
                        id: metadata.resource,
                        generation: metadata.resource_generation,
                    },
                },
            },
        ))
    }

    fn apply(&mut self, effect: SubmissionEffect) {
        match effect {
            SubmissionEffect::None => {}
            SubmissionEffect::PublishImageRefusal { refusal } => {
                self.image
                    .as_mut()
                    .expect("disclosed image state")
                    .offers
                    .note_submitted(refusal.offer_id);
                // Custody has already moved: publication failure is never
                // a submit error. The refusal stays export-pending and
                // retries from journal progress until it is emitted.
                self.pending_image_refusal = Some(refusal);
                if let Ok(true) = self.publish_image_refusal(refusal) {
                    self.pending_image_refusal = None;
                }
            }
        }
    }

    pub(super) fn submit(&mut self, bytes: &[u8]) -> Result<(), Errno> {
        self.submit_transaction(bytes)
    }

    pub(super) fn submit_decoded(&mut self, submit: ShellFileSubmit) -> Result<(), Errno> {
        // A pending image refusal blocks every further submission until
        // it is emitted. An identical retry of the accepted submission
        // that produced it returns accepted success and retries
        // publication; anything else is refused while it is pending.
        if self.pending_image_refusal.is_some() {
            let _ = self.retry_pending_image_refusal();
            if self.pending_image_refusal.is_some()
                && !self.accepted.as_ref().is_some_and(|a| a.submit == submit)
            {
                return Err(EBUSY);
            }
        }
        // Accepted bytes cannot be edited. An identical retry names the same
        // retained object and never enqueues it again.
        if let Some(accepted) = &self.accepted {
            return if accepted.submit == submit {
                Ok(())
            } else {
                Err(EBUSY)
            };
        }
        if submit.submission_id <= self.submission_watermark {
            return Err(EALREADY);
        }
        // Owner backpressure precedes any decoding work.
        if self.inbound.len() >= INBOUND_RECORDS {
            return Err(Errno::EAGAIN);
        }
        let staging = self.staging.as_ref().ok_or(Errno::ESTALE)?;
        if staging.bytes.len() != submit.candidate_bytes as usize {
            return Err(Errno::EINVAL);
        }
        let revision = if self.image_disclosed() { 9 } else { 8 };
        let record = decode_shell_file_record_at_revision(
            &staging.bytes,
            ShellFileClass::Candidate,
            revision,
        )
        .map_err(|_| Errno::EINVAL)?;
        if record.header.connection_epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        if record.header.submission_id != submit.submission_id {
            return Err(Errno::EINVAL);
        }
        let kind = record.header.kind;
        let (inbound, effect) = self.decode(&staging.bytes, kind)?;
        let body = encode_shell_file_submitted_body_at_revision(
            ShellFileSubmitted {
                submission_id: submit.submission_id,
                candidate_kind: kind,
            },
            revision,
        )
        .map_err(|_| Errno::EINVAL)?;
        // Every fallible check precedes custody transfer, including journal
        // capacity. Custody records never use the terminal reserve.
        let sequence = self
            .journal
            .append(ShellFileKind::Submitted, &body, false)?;
        let staging = self.staging.take().expect("candidate checked");
        self.accepted = Some(Accepted {
            submit,
            bytes: staging.bytes,
            handle: staging.handle,
            sequence,
        });
        self.submission_watermark = submit.submission_id;
        if kind == ShellFileKind::Negotiate {
            self.negotiate_accepted = true;
        }
        if let Some(inbound) = inbound {
            if let Inbound::ImageBegin(metadata) = &inbound {
                self.image
                    .as_mut()
                    .expect("disclosed image state")
                    .offers
                    .note_submitted(metadata.offer_id);
            }
            self.inbound.push_back(inbound);
        }
        self.apply(effect);
        Ok(())
    }
}

/// Image refusal publication after submission custody moves.
enum SubmissionEffect {
    None,
    PublishImageRefusal { refusal: PendingImageRefusal },
}
