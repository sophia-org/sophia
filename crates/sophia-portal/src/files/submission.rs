use super::*;

impl<A: PortalFileAuthority> PortalFileExport<A> {
    pub(super) fn submit(&mut self, bytes: &[u8]) -> Result<(), Errno> {
        let c = self.live()?;
        let control = decode_portal_file_submit(bytes, c.admission.epoch).map_err(codec)?;
        let staged = &c.staging.as_ref().ok_or(Errno::EINVAL)?.bytes;
        if staged.len() != control.candidate_bytes as usize {
            return Err(Errno::EINVAL);
        }
        let (header, candidate) =
            decode_portal_file_candidate(staged, c.admission.epoch).map_err(codec)?;
        if header.submission_id != control.submission_id {
            return Err(Errno::EINVAL);
        }
        if let Some(accepted) = &c.accepted {
            if header.submission_id == accepted.id {
                if c.acknowledged >= accepted.receipt {
                    return Err(EALREADY);
                }
                return if accepted.bytes == *staged {
                    Ok(())
                } else {
                    Err(Errno::EINVAL)
                };
            }
            if header.submission_id < accepted.id {
                return Err(EALREADY);
            }
            if c.acknowledged < accepted.receipt {
                return Err(Errno::EAGAIN);
            }
        }
        if c.refused {
            return Err(Errno::EACCES);
        }
        match &candidate {
            PortalFileCandidate::Negotiate(_) if c.negotiated => return Err(EALREADY),
            PortalFileCandidate::Negotiate(_) => {}
            _ if !c.negotiated => return Err(Errno::EACCES),
            _ => {}
        }
        let receipt = PortalFileEvent::Submitted(PortalSubmitted {
            submission_id: header.submission_id,
            candidate_kind: header.kind,
        });
        let mut events = vec![receipt];
        let mut new_grant = None;
        let mut admitted_request = None;
        let mut completed = None;
        let mut new_upload = None;
        let mut negotiated = false;
        let mut refused = false;
        let now = self.authority.now_msec();
        let c = self.connection.as_ref().unwrap();

        match candidate {
            PortalFileCandidate::Negotiate(range) => {
                negotiated = range.minimum_revision <= 1
                    && range.maximum_revision >= 1
                    && range.minimum_revision <= range.maximum_revision;
                refused = !negotiated;
                events.push(if negotiated {
                    PortalFileEvent::Negotiated
                } else {
                    PortalFileEvent::Refused
                });
            }
            PortalFileCandidate::TransferRequest(request) => {
                let t = &request.transfer;
                if t.source_namespace != c.admission.source
                    || t.target_namespace != c.admission.target
                    || self.authority.namespace(t.source_namespace).is_none()
                    || self.authority.namespace(t.target_namespace).is_none()
                {
                    return Err(Errno::EACCES);
                }
                let valid_new = t.transfer.is_valid()
                    && t.generation != 0
                    && request.deadline_msec > now
                    && self.authority.source_generation(&request) == Some(t.generation)
                    && self.lifecycle.request(t.transfer).is_none()
                    && c.terminal.len() < self.limits.max_transfers as usize;
                if valid_new && self.retained >= self.limits.max_retained_transfers as usize {
                    return Err(Errno::ENOSPC);
                }
                if valid_new {
                    if self.permissions(t.source_namespace, t.target_namespace, t.kind)
                        && self.authority.allow(&request)
                    {
                        new_grant = Some(PortalGrant {
                            transfer: t.transfer,
                            source_namespace: t.source_namespace,
                            target_namespace: t.target_namespace,
                            kind: t.kind,
                            source_generation: t.generation,
                            broker_generation: self.limits.broker_generation,
                            deadline_msec: request.deadline_msec,
                            state: PortalGrantState::Active,
                        });
                    }
                    admitted_request = Some(request.clone());
                }
                events.push(PortalFileEvent::Decision(PortalBrokerResponsePacket {
                    transfer: t.transfer,
                    decision: new_grant.clone().map_or(
                        PortalBrokerResponseDecision::Denied,
                        PortalBrokerResponseDecision::Allowed,
                    ),
                }));
            }
            PortalFileCandidate::PayloadBegin(begin) => {
                let grant = self.active_grant(begin.transfer)?;
                if !self.authority.payload_handler().supports(grant.kind) {
                    return Err(Errno::EOPNOTSUPP);
                }
                if self.execution_pending(begin.transfer) {
                    return Err(EALREADY);
                }
                if self.upload.is_some() || self.issued.is_some() || self.execution.is_some() {
                    return Err(EBUSY);
                }
                if begin.total_bytes > u64::from(self.limits.max_payload_bytes) {
                    return Err(Errno::EINVAL);
                }
                new_upload = Some((begin.transfer, begin.total_bytes as usize));
            }
            PortalFileCandidate::PayloadEnd(end) => {
                self.active_grant(end.transfer)?;
                if self.execution_pending(end.transfer) {
                    return Err(EALREADY);
                }
                let upload = self
                    .upload
                    .as_ref()
                    .filter(|u| u.transfer == end.transfer)
                    .ok_or(Errno::EINVAL)?;
                if end.total_bytes != upload.total as u64 || upload.bytes.len() != upload.total {
                    return Err(Errno::EINVAL);
                }
                let request = self.lifecycle.request(end.transfer).ok_or(Errno::EINVAL)?;
                self.authority
                    .payload_handler()
                    .validate(request, &upload.bytes)?;
                if self.issued.is_some() {
                    return Err(EBUSY);
                }
                completed = Some((end.transfer, true));
            }
            PortalFileCandidate::TransferCancel(transfer) => {
                if self.execution_pending(transfer) {
                    return Err(EALREADY);
                }
                self.active_grant(transfer)?;
                events.push(PortalFileEvent::TransferOutcome(PortalTransferOutcome {
                    transfer,
                    state: PortalGrantState::Revoked,
                }));
                completed = Some((transfer, false));
            }
        }

        let c = self.connection.as_ref().unwrap();
        let terminal = c
            .terminal
            .len()
            .checked_add(usize::from(new_grant.is_some()))
            .and_then(|n| {
                n.checked_sub(usize::from(completed.is_some_and(|(_, execute)| !execute)))
            })
            .ok_or(Errno::EIO)?;
        // Sequence and byte-offset exhaustion must also leave room for every
        // retained terminal credit, not only the immediate records.
        c.journal
            .next_sequence()
            .checked_add((events.len() + terminal) as u64)
            .ok_or(Errno::ENOSPC)?;
        c.journal
            .size()
            .checked_add((events.len() * 88 + terminal * 48) as u64)
            .ok_or(Errno::ENOSPC)?;
        let records = events
            .iter()
            .enumerate()
            .map(|(i, e)| {
                encode_portal_file_event(c.admission.epoch, c.journal.next_sequence() + i as u64, e)
                    .map_err(|_| Errno::EINVAL)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let bounds = self.bounds(terminal);
        let new_qid = if new_upload.is_some() {
            Some(self.next_qid.checked_add(1).ok_or(Errno::ENOSPC)?)
        } else {
            None
        };
        let c = self.connection.as_mut().unwrap();
        let sequence = c.journal.next_sequence();
        let prepared = c.journal.prepare_batch(records, bounds)?;
        // No owner mutation precedes the whole journal reservation. All
        // lifecycle preconditions above were checked under this exclusive owner.
        if let Some(request) = admitted_request {
            let generation = request.transfer.generation;
            self.lifecycle
                .submit_decided(
                    request,
                    if new_grant.is_some() {
                        PortalPolicyDecision::Allow
                    } else {
                        PortalPolicyDecision::Deny
                    },
                    generation,
                    now,
                )
                .map_err(|_| Errno::EIO)?;
            self.retained += 1;
        }
        if let Some(grant) = new_grant {
            c.terminal.insert(grant.transfer, ());
        }
        if let Some((transfer, total)) = new_upload {
            self.upload = Some(Upload {
                transfer,
                qid: self.next_qid,
                total,
                bytes: Vec::with_capacity(total),
                writer: false,
            });
            self.next_qid = new_qid.unwrap();
        }
        if let Some((transfer, execute)) = completed {
            if execute {
                let upload = self.upload.take().unwrap();
                self.execution = Some(PortalFileExecution {
                    request: self
                        .lifecycle
                        .request(transfer)
                        .expect("accepted End retains its request")
                        .clone(),
                    grant: self.lifecycle.grant(transfer).unwrap().clone(),
                    payload: upload.bytes,
                    authority_revision: c.authority_revision,
                });
            } else {
                self.lifecycle
                    .executor_failed(transfer)
                    .map_err(|_| Errno::EIO)?;
                c.terminal.remove(&transfer);
                if self.upload.as_ref().is_some_and(|u| u.transfer == transfer) {
                    self.upload = None;
                }
            }
        }
        prepared.commit();
        c.accepted = Some(Accepted {
            id: header.submission_id,
            bytes: c.staging.as_ref().unwrap().bytes.clone(),
            receipt: sequence,
        });
        c.submitted = true;
        c.negotiated |= negotiated;
        c.refused |= refused;
        if refused {
            c.refusal_deadline = Some(
                Instant::now() + Duration::from_millis(self.limits.ack_progress_timeout_ms.into()),
            );
        }
        self.start_ack_clock();
        Ok(())
    }

    fn active_grant(&self, id: PortalTransferId) -> Result<&PortalGrant, Errno> {
        if self
            .connection
            .as_ref()
            .is_none_or(|c| !c.terminal.contains_key(&id))
        {
            return Err(Errno::EINVAL);
        }
        let grant = self.lifecycle.grant(id).ok_or(Errno::EINVAL)?;
        if grant.state != PortalGrantState::Active {
            return Err(EALREADY);
        }
        if grant.deadline_msec <= self.authority.now_msec() {
            return Err(Errno::ESTALE);
        }
        if !self.current_source(id) {
            return Err(Errno::ESTALE);
        }
        if !self.permissions(grant.source_namespace, grant.target_namespace, grant.kind) {
            return Err(Errno::EACCES);
        }
        Ok(grant)
    }

    pub(super) fn acknowledge(&mut self, bytes: &[u8]) -> Result<(), Errno> {
        let c = self.live()?;
        let ack = decode_portal_file_ack(bytes, c.admission.epoch).map_err(codec)?;
        if c.journal.ack(ack.connection_epoch, ack.sequence)? {
            c.acknowledged = ack.sequence;
            c.ack_deadline = None;
            if c.refused && c.journal.position().records == 0 {
                c.revoked = true;
            }
            self.start_ack_clock();
        }
        Ok(())
    }
}
