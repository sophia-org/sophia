//! Submission custody: decoding a staged candidate into what the owners
//! receive, and the upload-slot effect applied once custody transfers.
use super::*;

impl ShellFiles {
    /// Decodes a staged candidate into the value the owners receive and the
    /// slot effect applied once custody transfers. The phase decides which
    /// kinds exist: Negotiate once before negotiation, content records only
    /// after a content grant.
    fn decode(&self, bytes: &[u8], kind: ShellFileKind) -> Result<(Inbound, SlotEffect), Errno> {
        let content = |decoded: Result<ShellFileTransactionRecord, ShellFilePayloadError>| {
            if !self.negotiated || !self.content {
                return Err(Errno::EACCES);
            }
            let value = decoded.map_err(|_| Errno::EINVAL)?;
            Ok(Inbound::Content(value.transaction, Box::new(value.record)))
        };
        let closing = |inbound: Inbound| {
            let resource = match &inbound {
                Inbound::Content(_, record) => match record.as_ref() {
                    ShellContentRecord::ResourceEnd(value) => Some(value.resource),
                    ShellContentRecord::ResourceCancel(value) => Some(value.resource),
                    _ => None,
                },
                Inbound::Negotiate(_) | Inbound::Candidate(_) => None,
            };
            (
                inbound,
                resource.map_or(SlotEffect::None, SlotEffect::Close),
            )
        };
        let inbound = match kind {
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
            ShellFileKind::ResourceBegin => return self.decode_begin(bytes),
            ShellFileKind::ResourceEnd => {
                return Ok(closing(content(decode_shell_file_resource_end(bytes))?));
            }
            ShellFileKind::ResourceCancel => {
                return Ok(closing(content(decode_shell_file_resource_cancel(bytes))?));
            }
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
            _ => return Err(Errno::EINVAL),
        };
        Ok((inbound, SlotEffect::None))
    }

    /// A Begin names a free slot. The slot is reserved (pending) only when the
    /// description has a valid layout; the store still decides admission and
    /// reports it, so a malformed description reaches its existing refusal.
    fn decode_begin(&self, bytes: &[u8]) -> Result<(Inbound, SlotEffect), Errno> {
        if !self.negotiated || !self.content {
            return Err(Errno::EACCES);
        }
        let value = decode_shell_file_resource_begin(bytes).map_err(|_| Errno::EINVAL)?;
        if value.slot >= u16::from(self.upload_slots) {
            return Err(Errno::EINVAL);
        }
        if self.uploads[usize::from(value.slot)].is_some() {
            return Err(EBUSY);
        }
        let ShellContentRecord::ResourceBegin(begin) = &value.record else {
            return Err(Errno::EINVAL);
        };
        let layout = self
            .content_limits
            .as_ref()
            .and_then(|limits| begin.layout(limits).ok());
        let effect = layout.map_or(SlotEffect::None, |layout| SlotEffect::Bind {
            slot: value.slot as u8,
            transaction: value.transaction,
            grant: begin.grant,
            resource: begin.resource,
            layout,
        });
        Ok((
            Inbound::Content(value.transaction, Box::new(value.record)),
            effect,
        ))
    }

    fn apply(&mut self, effect: SlotEffect) -> Result<(), Errno> {
        match effect {
            SlotEffect::None => {}
            SlotEffect::Bind {
                slot,
                transaction,
                grant,
                resource,
                layout,
            } => {
                let qid = self.allocate_qid()?;
                let chunk =
                    (u64::from(layout.row_bytes) * u64::from(layout.rows_per_chunk)) as usize;
                self.uploads[usize::from(slot)] = Some(Binding {
                    qid,
                    transaction,
                    grant,
                    resource,
                    layout,
                    admitted: false,
                    closing: false,
                    writer: None,
                    passed: 0,
                    ordinal: 0,
                    scratch: Vec::with_capacity(chunk),
                });
            }
            SlotEffect::Close(resource) => {
                for binding in self.uploads.iter_mut().flatten() {
                    if binding.resource == resource {
                        binding.closing = true;
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn submit(&mut self, bytes: &[u8]) -> Result<(), Errno> {
        let submit = decode_shell_file_submit(bytes).map_err(|_| Errno::EINVAL)?;
        if submit.connection_epoch != self.epoch {
            return Err(Errno::ESTALE);
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
        let record = decode_shell_file_record(&staging.bytes, ShellFileClass::Candidate)
            .map_err(|_| Errno::EINVAL)?;
        if record.header.connection_epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        if record.header.submission_id != submit.submission_id {
            return Err(Errno::EINVAL);
        }
        let kind = record.header.kind;
        let (inbound, effect) = self.decode(&staging.bytes, kind)?;
        let body = encode_shell_file_submitted_body(ShellFileSubmitted {
            submission_id: submit.submission_id,
            candidate_kind: kind,
        })
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
        self.inbound.push_back(inbound);
        // Only an allocated qid can fail here, and nothing can observe a
        // half-applied effect: the binding is created whole or not at all.
        self.apply(effect)
    }
}

/// What an accepted candidate does to the upload slots once custody moved.
enum SlotEffect {
    None,
    Bind {
        slot: u8,
        transaction: TransactionId,
        grant: ContentGrant,
        resource: ContentResourceId,
        layout: ContentResourceLayout,
    },
    Close(ContentResourceId),
}
