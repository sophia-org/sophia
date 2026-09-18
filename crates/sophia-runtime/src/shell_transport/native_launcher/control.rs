use super::*;
mod activation;
mod deadlines;
pub use activation::{NativeLauncherActivationDecision, NativeLauncherActivationEligibility};
mod state;
pub(in crate::shell_transport) use state::NativeControl;
pub(crate) use state::NativePresented;
use state::{AcceptIntent, InputReceipt};

impl ShellComponentTransport {
    fn native_input_slot(&self) -> Option<usize> {
        let maximum = (self.content_limits.as_ref()?.max_pending_actions as usize)
            .saturating_sub(self.action_cancellations.len());
        self.native_control.slot(maximum)
    }

    /// Caller supplies the exact current state to candidate intake; incoming
    /// client bytes never select the authoritative issued revision.
    pub fn native_launcher_state(&self) -> Option<(NativeLauncherOpening, u64)> {
        self.native_control.active().then(|| {
            (
                self.native_control.opening.expect("active opening"),
                self.native_control.revision,
            )
        })
    }
    pub fn native_launcher_focus(&self) -> Option<NativeLauncherBinding> {
        let focus = self.native_control.focus?;
        let shown = self.native_control.presented?;
        (self.native_control.active()
            && !self.native_control.launch_admitted
            && focus.grant == shown.grant
            && focus.output == shown.output
            && focus.allocation == shown.allocation
            && focus.candidate_generation == shown.candidate_generation
            && focus.presentation_epoch == shown.presentation_epoch
            && focus.interaction_generation == shown.interaction_generation
            && focus.opening == shown.content.opening
            && focus.catalog_generation == shown.content.catalog_generation
            && focus.state_revision == shown.content.state_revision)
            .then_some(focus)
    }

    /// Only a successful actual candidate Presented transition can populate the
    /// source observed here. Prepared and arbitrary caller bindings cannot do so.
    pub fn install_native_launcher_focus(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
    ) -> Result<NativeLauncherBinding, ShellTransportError> {
        self.require_native_launcher(epochs)?;
        if self.native_control.launch_admitted {
            return Err(ShellTransportError::WrongActivation);
        }
        let (opening, revision) = self
            .native_launcher_state()
            .ok_or(ShellTransportError::WrongCandidate)?;
        let shown = self
            .native_control
            .presented
            .ok_or(ShellTransportError::WrongCandidate)?;
        if shown.grant != opening.grant
            || shown.output != opening.output
            || shown.content.opening != opening.opening
            || shown.content.catalog_generation != opening.catalog_generation
            || shown.content.state_revision != revision
        {
            return Err(ShellTransportError::WrongCandidate);
        }
        if let Some(focus) = self.native_control.focus
            && focus.candidate_generation == shown.candidate_generation
            && focus.presentation_epoch == shown.presentation_epoch
        {
            self.flush_native_accept(epochs)?;
            return Ok(focus);
        }
        // The matching Presented outcome must own its FIFO position first.
        self.flush_content_candidate_events(epochs)?;
        let lease = self.native_control.next_lease;
        let next_lease = lease
            .checked_add(1)
            .ok_or(ShellTransportError::WrongActivation)?;
        let focus = NativeLauncherBinding {
            grant: shown.grant,
            opening: opening.opening,
            output: shown.output,
            allocation: shown.allocation,
            catalog_generation: shown.content.catalog_generation,
            candidate_generation: shown.candidate_generation,
            presentation_epoch: shown.presentation_epoch,
            interaction_generation: shown.interaction_generation,
            state_revision: shown.content.state_revision,
            focus_lease: lease,
        };
        let frame = encode_shell_native_launcher_frame(
            transaction,
            &ShellNativeLauncherRecord::Focus(focus),
        )?;
        let revoke = self
            .native_control
            .focus
            .map(|binding| {
                encode_shell_native_launcher_frame(
                    transaction,
                    &ShellNativeLauncherRecord::FocusRevoked(NativeLauncherFocusRevoked {
                        binding,
                        reason: ContentReason::Stale as u16,
                    }),
                )
            })
            .transpose()?;
        // New Focus plus its future revocation; old revocation transfers its
        // already charged credit. No I/O between this check and final transfer.
        if !self.control_capacity_available(epochs, 2) {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        if let Some(frame) = revoke {
            self.output.push(frame, true);
        }
        self.output.push(frame, true);
        self.native_control.focus = Some(focus);
        self.native_control.next_lease = next_lease;
        self.native_control.inputs.iter_mut().for_each(|entry| {
            if entry.is_some_and(|v| {
                v.kind == NativeLauncherInputKind::Accept && v.event.binding != focus
            }) {
                *entry = None;
            }
        });
        self.flush_native_accept(epochs)?;
        Ok(focus)
    }

    /// None retains one Enter intent for the exact issued revision. It owns an
    /// output credit but no fabricated Presented identity; later edits cancel it.
    pub fn issue_native_launcher_input(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        expected: NativeLauncherBinding,
        transaction: TransactionId,
        kind: NativeLauncherInputKind,
        text: &str,
        issued_mono_usec: u64,
    ) -> Result<Option<NativeLauncherEvent>, ShellTransportError> {
        self.require_native_launcher(epochs)?;
        let (_, revision) = self
            .native_launcher_state()
            .ok_or(ShellTransportError::WrongActivation)?;
        let focus = self
            .native_launcher_focus()
            .ok_or(ShellTransportError::WrongActivation)?;
        if focus != expected {
            return Err(ShellTransportError::WrongActivation);
        }
        if (kind == NativeLauncherInputKind::Text
            && (text.is_empty()
                || !sophia_protocol::shell_launcher_text_valid(
                    text,
                    SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES,
                )))
            || (kind != NativeLauncherInputKind::Text && !text.is_empty())
        {
            return Err(ShellTransportError::WrongContentRecord);
        }
        if !transaction.is_valid()
            || issued_mono_usec == 0
            || issued_mono_usec < self.native_control.last_issued
            || issued_mono_usec < self.native_control.last_service
        {
            return Err(ShellTransportError::WrongActivation);
        }
        let opening = self.native_control.opening.expect("active opening");
        if self.service_native_launcher_deadlines(epochs, opening, transaction, issued_mono_usec)? {
            return Err(ShellTransportError::WrongActivation);
        }
        if kind == NativeLauncherInputKind::Accept && focus.state_revision != revision {
            if !text.is_empty()
                || self.native_control.accept.is_some()
                || self.native_input_slot().is_none()
            {
                return Err(ShellTransportError::WrongActivation);
            }
            if !self.control_capacity_available(epochs, 1) {
                return Err(ShellTransportError::ContentQueueSaturated);
            }
            self.native_control.accept = Some(AcceptIntent {
                transaction,
                revision,
                issued: issued_mono_usec,
            });
            self.native_control.last_issued = issued_mono_usec;
            return Ok(None);
        }
        self.queue_native_input(epochs, transaction, kind, text, issued_mono_usec, false)
            .map(Some)
    }

    fn queue_native_input(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        transaction: TransactionId,
        kind: NativeLauncherInputKind,
        text: &str,
        issued: u64,
        transfer: bool,
    ) -> Result<NativeLauncherEvent, ShellTransportError> {
        let focus = self
            .native_control
            .focus
            .ok_or(ShellTransportError::WrongActivation)?;
        if kind == NativeLauncherInputKind::Accept
            && self
                .native_control
                .presented
                .is_none_or(|shown| shown.content.selected == 0)
        {
            return Err(ShellTransportError::WrongActivation);
        }
        let slot = self
            .native_input_slot()
            .ok_or(ShellTransportError::ContentQueueSaturated)?;
        let revision = if kind == NativeLauncherInputKind::Accept {
            self.native_control.revision
        } else {
            self.native_control
                .revision
                .checked_add(1)
                .ok_or(ShellTransportError::WrongActivation)?
        };
        let next_event = self
            .native_control
            .next_event
            .checked_add(1)
            .ok_or(ShellTransportError::WrongActivation)?;
        let event = NativeLauncherEvent {
            binding: focus,
            event_id: self.native_control.next_event,
            state_revision: revision,
        };
        let frame = encode_shell_native_launcher_frame(
            transaction,
            &ShellNativeLauncherRecord::Input(NativeLauncherInput {
                event,
                issued_mono_usec: issued,
                kind,
                text: text.to_owned(),
            }),
        )?;
        if self.content_limits.as_ref().is_none_or(|limits| {
            frame.len() - SOPHIA_IPC_HEADER_LEN > limits.max_frame_payload as usize
        }) {
            return Err(ShellTransportError::WrongContentRecord);
        }
        if frame.len() > self.control_frame_bytes()
            || !self.frame_capacity_available(epochs, frame.len(), true, transfer)
        {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        self.output.push(frame, true);
        self.native_control.inputs[slot] = Some(InputReceipt {
            event,
            kind,
            ack: None,
            ack_started: issued.max(self.native_control.last_service),
            activation_attempted: false,
        });
        self.native_control.next_event = next_event;
        self.native_control.revision = revision;
        self.native_control.last_issued = issued;
        // An unsent Enter intent is invalidated by an actual subsequent edit.
        self.native_control.accept = None;
        Ok(event)
    }

    pub(in crate::shell_transport) fn flush_native_accept(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
    ) -> Result<(), ShellTransportError> {
        let Some(intent) = self.native_control.accept else {
            return Ok(());
        };
        if !self.native_control.active() {
            return Ok(());
        }
        if self.native_control.revision != intent.revision {
            return Err(ShellTransportError::WrongActivation);
        }
        if self
            .native_control
            .focus
            .is_some_and(|v| v.state_revision == intent.revision)
        {
            if self
                .native_control
                .presented
                .is_some_and(|shown| shown.content.selected == 0)
            {
                self.native_control.accept = None;
                return Ok(());
            }
            self.queue_native_input(
                epochs,
                intent.transaction,
                NativeLauncherInputKind::Accept,
                "",
                intent.issued,
                true,
            )?;
        }
        Ok(())
    }

    /// A stale/mismatching ACK is observable but cannot mutate another event.
    /// A consumed Accept remains owned for independent launch admission.
    pub fn poll_native_launcher_input_ack(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, NativeLauncherInputAck, bool)>, ShellTransportError> {
        self.require_native_launcher(epochs)?;
        self.poll_io_bounded(epochs, 64 * 1024)?;
        let Some(index) = self
            .inbox
            .iter()
            .position(|f| u16::from_le_bytes([f[6], f[7]]) == 194)
        else {
            return if self.peer_closed {
                Err(ShellTransportError::NotConnected)
            } else {
                Ok(None)
            };
        };
        let (transaction, ShellNativeLauncherRecord::InputAck(ack)) =
            decode_shell_native_launcher_frame(&self.inbox[index])?
        else {
            return Err(ShellTransportError::WrongContentRecord);
        };
        if Some(ack.event.binding.grant) != self.content_grant {
            return Err(ShellTransportError::WrongContentGrant);
        }
        let found = self
            .native_control
            .inputs
            .iter()
            .position(|v| v.is_some_and(|v| v.event == ack.event && v.ack.is_none()));
        if let Some(index) = found {
            let input = self.native_control.inputs[index]
                .as_mut()
                .expect("exact receipt");
            if input.kind == NativeLauncherInputKind::Accept {
                input.ack = Some(ack.disposition);
            } else {
                self.native_control.inputs[index] = None;
            }
        }
        self.inbox.remove(index);
        Ok(Some((transaction, ack, found.is_some())))
    }

    /// Disarm immediately; retain exact revocation/Closed obligations if their
    /// FIFO transfer is temporarily refused. Closing never disposes pixel owners.
    pub fn close_native_launcher(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        expected: NativeLauncherOpening,
        transaction: TransactionId,
        reason: ContentReason,
    ) -> Result<(), ShellTransportError> {
        self.require_native_launcher(epochs)?;
        let opening = self
            .native_control
            .opening
            .ok_or(ShellTransportError::WrongActivation)?;
        if opening != expected {
            return Err(ShellTransportError::WrongActivation);
        }
        encode_shell_native_launcher_frame(
            transaction,
            &ShellNativeLauncherRecord::Closed(NativeLauncherClosed {
                grant: opening.grant,
                opening: opening.opening,
                reason: reason as u16,
            }),
        )?;
        if let Some(recorded) = self.native_control.closing
            && recorded != (transaction, reason as u16)
        {
            return Err(ShellTransportError::WrongActivation);
        }
        self.native_control.closing = Some((transaction, reason as u16));
        self.flush_native_close(epochs)
    }

    pub(in crate::shell_transport) fn flush_native_close(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<(), ShellTransportError> {
        let Some((tx, reason)) = self.native_control.closing else {
            return Ok(());
        };
        let opening = self
            .native_control
            .opening
            .ok_or(ShellTransportError::WrongActivation)?;
        let closed = encode_shell_native_launcher_frame(
            tx,
            &ShellNativeLauncherRecord::Closed(NativeLauncherClosed {
                grant: opening.grant,
                opening: opening.opening,
                reason,
            }),
        )?;
        let revoked = self
            .native_control
            .focus
            .map(|binding| {
                encode_shell_native_launcher_frame(
                    tx,
                    &ShellNativeLauncherRecord::FocusRevoked(NativeLauncherFocusRevoked {
                        binding,
                        reason,
                    }),
                )
            })
            .transpose()?;
        if !self.control_capacity_available(epochs, 0) {
            return Ok(());
        }
        epochs
            .allocations_mut(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .close_native_proposals(opening)?;
        self.flush_content_allocation_events(epochs)?;
        epochs
            .active_candidates_mut(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .close_native_opening(opening)?;
        self.flush_content_candidate_events(epochs)?;
        if let Some(frame) = revoked {
            self.output.push(frame, true);
        }
        self.output.push(closed, true);
        self.native_control.clear_opening();
        Ok(())
    }
}
