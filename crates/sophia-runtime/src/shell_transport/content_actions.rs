use super::ShellComponentTransport;
use sophia_protocol::{
    ContentAction, ContentActionAck, IpcMessageKind, ShellContentRecord, TransactionId,
};

use super::{ShellSessionTransport, ShellTransportError, content_admission};

impl ShellComponentTransport {
    /// Admission requires two real aggregate credits: Action and cancellation.
    pub fn content_action_capacity_available(&self, epochs: &crate::ContentEpochRegistry) -> bool {
        self.content_limits.as_ref().is_some_and(|limits| {
            self.action_cancellations.len() < limits.max_pending_actions as usize
                && self.action_cancellations.len() < self.action_cancellations.capacity()
                && self.control_capacity_available(epochs, 2)
        })
    }

    pub fn send_content_action(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        action: &ContentAction,
    ) -> Result<(), ShellTransportError> {
        if action.kind == 3 {
            let Some(index) = self
                .action_cancellations
                .iter()
                .position(|pending| pending.event_id == action.event_id)
            else {
                return Err(ShellTransportError::WrongActivation);
            };
            let mut expected = self.action_cancellations[index].clone();
            expected.kind = action.kind;
            expected.reason = action.reason;
            if expected != *action {
                return Err(ShellTransportError::WrongActivation);
            }
            self.queue_content_record(
                epochs,
                transaction,
                &ShellContentRecord::Action(action.clone()),
                true,
            )?;
            self.action_cancellations.remove(index);
        } else {
            if self
                .action_cancellations
                .iter()
                .any(|pending| pending.event_id == action.event_id)
                || !self.content_action_capacity_available(epochs)
            {
                return Err(ShellTransportError::ContentQueueSaturated);
            }
            // Admission above checks the preallocated slot as well as the
            // aggregate credits. After the FIFO owns Action, recording its
            // exact cancellation credit cannot allocate or call user code.
            self.queue_content_record(
                epochs,
                transaction,
                &ShellContentRecord::Action(action.clone()),
                false,
            )?;
            self.action_cancellations.push(action.clone());
        }
        Ok(())
    }

    /// The action owner has settled receipt/effect or transferred cancellation.
    /// A deadline alone must not release an unqueued cancellation obligation.
    /// Keeping a credit cannot reactivate a target; it only reserves a cancel.
    pub fn retain_content_action_reservations(&mut self, mut live: impl FnMut(u64) -> bool) {
        self.action_cancellations
            .retain(|action| live(action.event_id));
    }

    pub fn poll_content_action_ack(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, ContentActionAck)>, ShellTransportError> {
        self.poll_io(epochs)?;
        let at = self.inbox.iter().position(|frame| {
            u16::from_le_bytes([frame[6], frame[7]]) == IpcMessageKind::ShellContentActionAck as u16
        });
        let Some(frame) = at.and_then(|index| self.inbox.remove(index)) else {
            return if self.peer_closed {
                Err(ShellTransportError::NotConnected)
            } else {
                Ok(None)
            };
        };
        let (transaction, record) = sophia_protocol::decode_shell_content_frame(&frame)?;
        if !content_admission::client_record(&record) {
            return Err(ShellTransportError::WrongContentRecord);
        }
        if content_admission::record_grant(&record) != self.content_grant {
            return Err(ShellTransportError::WrongContentGrant);
        }
        let ShellContentRecord::ActionAck(ack) = record else {
            return Err(ShellTransportError::WrongContentRecord);
        };
        Ok(Some((transaction, ack)))
    }
}

// Legacy single-shell facade, delegating to the same shared registry path.

// The owned legacy and borrowed Session façades share forwarding, not policy.
macro_rules! transport_facade {
    ($transport:ty) => {
        impl $transport {
            pub fn content_action_capacity_available(&self) -> bool {
                self.state
                    .content_action_capacity_available(&self.content_epochs)
            }

            pub fn send_content_action(
                &mut self,
                transaction: TransactionId,
                action: &ContentAction,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .send_content_action(&mut self.content_epochs, transaction, action)
            }

            pub fn poll_content_action_ack(
                &mut self,
            ) -> Result<Option<(TransactionId, ContentActionAck)>, ShellTransportError> {
                self.state.poll_content_action_ack(&mut self.content_epochs)
            }
        }
        impl $transport {
            pub fn retain_content_action_reservations(&mut self, live: impl FnMut(u64) -> bool) {
                self.state.retain_content_action_reservations(live)
            }
        }
    };
}
transport_facade!(ShellSessionTransport);
transport_facade!(crate::shell_transport::ShellTransportConnection<'_>);
