use super::ShellComponentTransport;
use sophia_protocol::{ContentAction, ContentActionAck, ShellContentRecord, TransactionId};

use super::wire::ContentWant;
use super::{ShellSessionTransport, ShellTransportError};

impl ShellComponentTransport {
    /// Admission requires two real aggregate credits: Action and cancellation.
    pub fn content_action_capacity_available(&self, epochs: &crate::ContentEpochRegistry) -> bool {
        self.content_limits.as_ref().is_some_and(|limits| {
            self.action_cancellations.len() + self.native_control.input_occupancy()
                < limits.max_pending_actions as usize
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
        let taken = self.take_content(ContentWant::ActionAck)?;
        Ok(self
            .admit_client_record(taken)?
            .map(|(transaction, record)| {
                let ShellContentRecord::ActionAck(ack) = record else {
                    unreachable!("an action ack was selected");
                };
                (transaction, ack)
            }))
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
