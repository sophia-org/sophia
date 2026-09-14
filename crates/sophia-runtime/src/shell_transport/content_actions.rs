use sophia_protocol::{
    ContentAction, ContentActionAck, IpcMessageKind, ShellContentRecord, TransactionId,
};

use super::{ShellSessionTransport, ShellTransportError, content_admission};

const MAX_CONTENT_ACTION_FRAME_BYTES: usize = sophia_protocol::SOPHIA_IPC_HEADER_LEN + 112;

impl ShellSessionTransport {
    /// Reserve enough queue space for an action and its possible cancellation.
    pub fn content_action_capacity_available(&self) -> bool {
        let Some(limits) = self.content_limits.as_ref() else {
            return false;
        };
        self.output
            .len()
            .saturating_add(MAX_CONTENT_ACTION_FRAME_BYTES.saturating_mul(2))
            <= limits.max_output_queue_bytes as usize
    }

    pub fn send_content_action(
        &mut self,
        transaction: TransactionId,
        action: &ContentAction,
    ) -> Result<(), ShellTransportError> {
        self.send_content_record(transaction, &ShellContentRecord::Action(action.clone()))
    }

    pub fn poll_content_action_ack(
        &mut self,
    ) -> Result<Option<(TransactionId, ContentActionAck)>, ShellTransportError> {
        self.poll_io()?;
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
