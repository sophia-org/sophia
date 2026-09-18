//! Bounded semantic input custody before the transport owns its exact event.
use super::*;
use sophia_protocol::{NativeLauncherBinding, NativeLauncherInputKind, TransactionId};

pub(super) struct PendingInput {
    binding: NativeLauncherBinding,
    transaction: TransactionId,
    kind: NativeLauncherInputKind,
    text: String,
    queued_usec: u64,
}

impl NativeLauncherContentService {
    /// Refusal leaves the caller's input untouched. Admission captures the exact
    /// focus, never a future row or a replacement focus. The live caller retires
    /// an overflowing peer rather than silently dropping committed text.
    #[allow(clippy::too_many_arguments)]
    pub fn queue_input(
        &mut self,
        transport: &ShellTransportConnection<'_>,
        binding: NativeLauncherBinding,
        transaction: TransactionId,
        kind: NativeLauncherInputKind,
        text: &str,
        queued_usec: u64,
    ) -> Result<bool, ShellTransportError> {
        self.validate(transport)?;
        if self.closing.is_some()
            || transport.native_launcher_focus() != Some(binding)
            || !transaction.is_valid()
            || queued_usec == 0
            || (kind == NativeLauncherInputKind::Text
                && (text.is_empty()
                    || !sophia_protocol::shell_launcher_text_valid(
                        text,
                        sophia_protocol::SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES,
                    )))
            || (kind != NativeLauncherInputKind::Text && !text.is_empty())
        {
            return Err(ShellTransportError::WrongActivation);
        }
        if self.inputs.len() == 32 || text.len() > 32768 - self.input_bytes {
            return Ok(false);
        }
        let owned = text.to_owned();
        self.inputs.push_back(PendingInput {
            binding,
            transaction,
            kind,
            text: owned,
            queued_usec,
        });
        self.input_bytes += text.len();
        Ok(true)
    }

    pub fn pending_inputs(&self) -> usize {
        self.inputs.len()
    }

    /// ACK service and input transfer each have a fixed record budget. A queue
    /// refusal retains the front transaction/text. Focus publication waits for
    /// this queue, so a normal new raster cannot retarget withheld input.
    pub fn service_inputs(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        now_usec: u64,
    ) -> Result<usize, ShellTransportError> {
        self.validate(transport)?;
        for _ in 0..32 {
            if transport.poll_native_launcher_input_ack()?.is_none() {
                break;
            }
        }
        let mut transferred = 0;
        for _ in 0..32 {
            let Some(input) = self.inputs.front() else {
                break;
            };
            if now_usec < input.queued_usec || now_usec - input.queued_usec > 5_000_000 {
                return Err(ShellTransportError::WrongActivation);
            }
            match transport.issue_native_launcher_input(
                input.binding,
                input.transaction,
                input.kind,
                &input.text,
                now_usec,
            ) {
                Ok(_) => {
                    // Transport owns the event (or Accept intent). Removal is
                    // infallible and performs no I/O or user callback.
                    let input = self.inputs.pop_front().expect("front remains owned");
                    self.input_bytes -= input.text.len();
                    transferred += 1;
                }
                Err(ShellTransportError::ContentQueueSaturated) => break,
                Err(error) => return Err(error),
            }
        }
        Ok(transferred)
    }
}
