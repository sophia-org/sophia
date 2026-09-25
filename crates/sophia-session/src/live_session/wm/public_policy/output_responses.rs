impl LivePublicPolicyState {
    fn poll_output_authority(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // The transport may already have admitted a replacement peer after the
        // old one vanished. Leave its bounded event queue untouched until the
        // physical rollback settles and the authority owner adopts the new
        // connection epoch; otherwise a valid proposal against the preserved
        // snapshot is spuriously rejected as stale.
        if self.output_cancel_requested.is_some() {
            return Ok(());
        }
        const MAX_EVENTS_PER_TURN: usize = 16;
        for _ in 0..MAX_EVENTS_PER_TURN {
            let event = match self.output_service.as_ref() {
                Some(service) => match service.try_event() {
                    Ok(Some(event)) => event,
                    Ok(None) => break,
                    Err(_disconnected) => {
                        self.output_service.take();
                        crate::session_println!(
                            "sophia_live_output_authority schema=1 status=degraded reason=service_disconnected preserved_topology=true"
                        );
                        break;
                    }
                },
                None => break,
            };
            match event {
                sophia_runtime::OutputTransportServiceEvent::Connected { connection_epoch } => {
                    let authority = self
                        .output_authority
                        .as_mut()
                        .ok_or("output service connected without an authority owner")?;
                    if connection_epoch > authority.connection_epoch() {
                        if authority.active_transaction().is_some() {
                            self.output_pending_connection_epoch = Some(
                                self.output_pending_connection_epoch
                                    .map_or(connection_epoch, |pending| {
                                        pending.max(connection_epoch)
                                    }),
                            );
                        } else {
                            authority.replace_connection_epoch(connection_epoch)?;
                        }
                    } else if connection_epoch != authority.connection_epoch() {
                        return Err("output service connected with a stale epoch".into());
                    }
                    crate::session_println!(
                        "sophia_live_output_authority schema=1 status=connected epoch={connection_epoch}"
                    );
                }
                sophia_runtime::OutputTransportServiceEvent::Proposal {
                    proposal,
                    admission,
                } => match admission {
                    sophia_runtime::OutputProposalAdmission::Active => {
                        self.settle_output_proposal(proposal)?;
                    }
                    sophia_runtime::OutputProposalAdmission::Queued { replaced } => {
                        if let Some(replaced) = replaced {
                            let authority = self
                                .output_authority
                                .as_ref()
                                .ok_or("queued output proposal has no authority owner")?;
                            self.output_service
                                .as_ref()
                                .ok_or("queued output proposal lost its service")?
                                .command(sophia_runtime::OutputTransportServiceCommand::Reply {
                                    transaction: replaced.transaction,
                                    outcome: sophia_protocol::OutputV1Outcome {
                                        connection_epoch: authority.connection_epoch(),
                                        topology_epoch: authority.published().topology_epoch,
                                        kind: sophia_protocol::OutputV1OutcomeKind::Stale,
                                        reason: sophia_protocol::SOPHIA_OUTPUT_OUTCOME_REASON_STALE,
                                    },
                                })
                                .map_err(|_| "output stale-reply queue disconnected")?;
                        }
                    }
                },
                sophia_runtime::OutputTransportServiceEvent::Promoted(proposal) => {
                    self.settle_output_proposal(proposal)?;
                }
                sophia_runtime::OutputTransportServiceEvent::ProposalRejected {
                    transaction,
                    message,
                } => {
                    crate::session_println!(
                        "sophia_live_output_authority schema=1 status=rejected transaction={} phase=admission reason={message:?}",
                        transaction.raw(),
                    );
                }
                sophia_runtime::OutputTransportServiceEvent::Disconnected {
                    connection_epoch,
                } => {
                    let replacement_epoch = connection_epoch
                        .checked_add(1)
                        .ok_or("output connection epoch exhausted after disconnect")?;
                    self.request_output_candidate_cancellation(
                        format!("output peer disconnected at epoch {connection_epoch}"),
                        Some(replacement_epoch),
                    )?;
                    crate::session_println!(
                        "sophia_live_output_authority schema=1 status=disconnected epoch={connection_epoch} preserved_topology=true"
                    );
                    break;
                }
                sophia_runtime::OutputTransportServiceEvent::AssigneeReplaced {
                    connection_epoch,
                    abandoned,
                } => {
                    self.request_output_candidate_cancellation(
                        format!("output assignee replaced at epoch {connection_epoch}"),
                        Some(connection_epoch),
                    )?;
                    crate::session_println!(
                        "sophia_live_output_authority schema=1 status=reauthorized epoch={} abandoned={} preserved_topology=true",
                        connection_epoch,
                        abandoned.len(),
                    );
                    if self.output_cancel_requested.is_some() {
                        break;
                    }
                }
                sophia_runtime::OutputTransportServiceEvent::ConnectionRejected { message } => {
                    crate::session_println!(
                        "sophia_live_output_authority schema=1 status=connection_rejected reason={message:?} preserved_topology=true"
                    );
                }
                sophia_runtime::OutputTransportServiceEvent::Failed { message } => {
                    self.request_output_candidate_cancellation(
                        format!("output authority service failed: {message}"),
                        None,
                    )?;
                    self.output_service.take();
                    crate::session_println!(
                        "sophia_live_output_authority schema=1 status=degraded reason={message:?} preserved_topology=true"
                    );
                    break;
                }
            }
        }
        Ok(())
    }

    fn settle_output_proposal(
        &mut self,
        proposal: sophia_runtime::AdmittedOutputProposal,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let admission = {
            let authority = self
                .output_authority
                .as_mut()
                .ok_or("output proposal has no authority owner")?;
            authority.admit(
                proposal.transaction,
                &proposal.message,
                &self.output_capabilities,
            )
        };
        let settlement = match admission {
            Ok(crate::live_output_authority::LiveOutputAuthorityAdmission::Validated(
                settlement,
            )) => settlement,
            Ok(crate::live_output_authority::LiveOutputAuthorityAdmission::Prepared) => {
                self.output_effect_dispatched = false;
                crate::session_println!(
                    "sophia_live_output_authority schema=1 status=effect_pending transaction={} preserved_topology=true",
                    proposal.transaction.raw(),
                );
                return Ok(());
            }
            Err(error) => {
                let authority = self
                    .output_authority
                    .as_ref()
                    .ok_or("output admission failure lost its authority owner")?;
                tracing::warn!(
                    "sophia_live_output_authority schema=1 status=rejected transaction={} phase=admission error={error}",
                    proposal.transaction.raw(),
                );
                crate::live_output_authority::LiveOutputAuthoritySettlement {
                    transaction: proposal.transaction,
                    outcome: sophia_protocol::OutputV1Outcome {
                        connection_epoch: authority.connection_epoch(),
                        topology_epoch: authority.published().topology_epoch,
                        kind: sophia_protocol::OutputV1OutcomeKind::Rejected,
                        reason: sophia_protocol::SOPHIA_OUTPUT_OUTCOME_REASON_INVARIANT,
                    },
                    published_snapshot: None,
                }
            }
        };
        self.send_output_settlement(settlement)
    }

    fn send_output_settlement(
        &self,
        settlement: crate::live_output_authority::LiveOutputAuthoritySettlement,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.output_service
            .as_ref()
            .ok_or("output settlement lost its transport service")?
            .command(sophia_runtime::OutputTransportServiceCommand::Settle {
                transaction: settlement.transaction,
                outcome: settlement.outcome,
            })
            .map_err(|_| "output settlement queue disconnected")?;
        crate::session_println!(
            "sophia_live_output_authority schema=1 status=settled transaction={} outcome={:?} topology_epoch={}",
            settlement.transaction.raw(),
            settlement.outcome.kind,
            settlement.outcome.topology_epoch,
        );
        Ok(())
    }

    fn finish_output_settlement(
        &mut self,
        settlement: crate::live_output_authority::LiveOutputAuthoritySettlement,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let startup = self.startup_output_transaction == Some(settlement.transaction);
        let cancelled = self
            .output_cancel_requested
            .as_ref()
            .is_some_and(|(transaction, _)| *transaction == settlement.transaction);
        let local_reason = cancelled
            .then(|| {
                self.output_cancel_requested
                    .as_ref()
                    .expect("matching output cancellation remains recorded")
                    .1
                    .clone()
            })
            .or_else(|| startup.then(|| "desktop profile startup".to_owned()));
        let publish_committed = (settlement.outcome.kind
            == sophia_protocol::OutputV1OutcomeKind::Committed)
            .then(|| settlement.published_snapshot.clone())
            .flatten();
        if let Some(reason) = local_reason {
            if let Some(connection_epoch) = self.output_pending_connection_epoch {
                let mut replacement = self
                    .output_authority
                    .as_ref()
                    .ok_or("cancelled output settlement lost its authority owner")?
                    .clone();
                replacement.replace_connection_epoch(connection_epoch)?;
                self.output_authority = Some(replacement);
            }
            if cancelled {
                self.output_cancel_requested = None;
            }
            if startup {
                self.startup_output_transaction = None;
            }
            self.output_pending_connection_epoch = None;
            crate::session_println!(
                "sophia_live_output_authority schema=3 status=settled_locally transaction={} outcome={:?} topology_epoch={} reason={reason:?} preserved_topology={}",
                settlement.transaction.raw(),
                settlement.outcome.kind,
                settlement.outcome.topology_epoch,
                publish_committed.is_none(),
            );
        } else if let Err(error) = self.send_output_settlement(settlement.clone()) {
            // The reducer is already terminal. In particular, a committed
            // topology has crossed physical first presentation and cannot be
            // made private again because its peer vanished between owner turns.
            self.output_service.take();
            tracing::warn!(
                "sophia_live_output_authority schema=2 status=degraded reason=terminal_settlement_transport transaction={} outcome={:?} error={error} preserved_topology=true",
                settlement.transaction.raw(),
                settlement.outcome.kind,
            );
        }
        // A committed topology is the desk from now on, so the transport's copy
        // has to become it: only the client that submitted learns the new epoch
        // from its outcome, and everyone who connects later -- a restarted
        // policy, a second tool -- is answered from the service's stored
        // snapshot. It goes out after the settlement, never before. A snapshot
        // is an unsolicited update and an outcome is the answer to a request;
        // sending the update first put a frame the client was not waiting for
        // in front of the one it was, and it failed to decode it.
        if let Some(published) = publish_committed {
            let topology_epoch = published.topology_epoch;
            let (transaction, transport_published) =
                self.publish_snapshot_to_transport(published, "committed_snapshot_transport")?;
            crate::session_println!(
                "sophia_live_output_authority schema=2 status=committed_snapshot_published transaction={} topology_epoch={topology_epoch} transport_published={transport_published}",
                transaction.raw(),
            );
        }
        Ok(())
    }

    fn request_output_candidate_cancellation(
        &mut self,
        reason: String,
        replacement_epoch: Option<u64>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(connection_epoch) = replacement_epoch {
            self.output_pending_connection_epoch = Some(
                self.output_pending_connection_epoch
                    .map_or(connection_epoch, |pending| pending.max(connection_epoch)),
            );
        }
        let startup_active = self.startup_output_transaction.is_some_and(|startup| {
            self.output_authority
                .as_ref()
                .and_then(|authority| authority.active_transaction())
                == Some(startup)
        });
        if startup_active {
            tracing::warn!(
                "sophia_live_output_authority schema=3 status=startup_peer_loss_ignored reason={reason:?} preserved_candidate=true"
            );
            return Ok(());
        }
        if self.output_effect_dispatched {
            let transaction = self
                .output_authority
                .as_ref()
                .and_then(|authority| authority.active_transaction())
                .ok_or("dispatched output effect lost its authority transaction")?;
            match self.output_cancel_requested.as_ref() {
                Some((pending, _)) if *pending != transaction => {
                    return Err("output cancellation targets a different transaction".into());
                }
                Some(_) => {}
                None => self.output_cancel_requested = Some((transaction, reason)),
            }
            return Ok(());
        }
        self.abandon_output_candidate()?;
        if let Some(connection_epoch) = self.output_pending_connection_epoch.take() {
            self.output_authority
                .as_mut()
                .ok_or("output assignee replacement has no authority owner")?
                .replace_connection_epoch(connection_epoch)?;
        }
        Ok(())
    }

    /// Whether an output policy candidate is dispatched or being cancelled.
    ///
    /// Publishing a hardware snapshot in either state is the race that
    /// `publish_output_authority_snapshot` refuses, so callers holding one ask
    /// here rather than discovering it as a session-ending error.
    fn output_candidate_active(&self) -> bool {
        self.output_authority
            .as_ref()
            .is_some_and(|authority| authority.active_transaction().is_some())
            || self.output_cancel_requested.is_some()
    }

    fn output_authority_topology_epoch(&self) -> Option<u64> {
        self.output_authority
            .as_ref()
            .map(|authority| authority.published().topology_epoch)
    }

    fn output_candidate_cancellation_reason(
        &self,
        transaction: TransactionId,
    ) -> Option<&str> {
        self.output_cancel_requested
            .as_ref()
            .filter(|(pending, _)| *pending == transaction)
            .map(|(_, reason)| reason.as_str())
    }
}
