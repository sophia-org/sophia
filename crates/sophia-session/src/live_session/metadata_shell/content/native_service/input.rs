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
    /// Shared physical capture dispatch. Stale captures have no effect; overflow
    /// is explicit so the connection owner can retire this peer without losing
    /// committed input silently. Escape enters the existing retained close.
    pub fn dispatch_capture(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        event: &sophia_engine::LauncherInputEvent,
        transaction: TransactionId,
        now_usec: u64,
    ) -> Result<bool, ShellTransportError> {
        self.validate(transport)?;
        let sophia_engine::LauncherInput::Native { binding, command } = &event.input else {
            return Err(ShellTransportError::WrongActivation);
        };
        if transport.native_launcher_focus() != Some(*binding)
            || event.output.raw() != binding.output.id
            || event.presentation_epoch != binding.presentation_epoch
        {
            return Ok(false);
        }
        match command {
            sophia_engine::NativeLauncherCommand::Input { kind, text } => {
                if self.queue_input(transport, *binding, transaction, *kind, text, now_usec)? {
                    Ok(true)
                } else {
                    Err(ShellTransportError::ContentQueueSaturated)
                }
            }
            sophia_engine::NativeLauncherCommand::Dismiss => {
                let opening = transport
                    .native_launcher_state()
                    .ok_or(ShellTransportError::WrongActivation)?
                    .0;
                match self.begin_close(
                    transport,
                    opening,
                    transaction,
                    sophia_protocol::ContentReason::Cancelled,
                ) {
                    Ok(()) | Err(ShellTransportError::ContentQueueSaturated) => Ok(true),
                    Err(error) => Err(error),
                }
            }
        }
    }

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
