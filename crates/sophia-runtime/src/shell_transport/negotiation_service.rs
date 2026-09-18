//! Retained protected handshake. No sleep or blocking socket operation here.
use super::*;
use std::io;
use std::time::Instant;

const HELLO_BYTES: usize = SOPHIA_IPC_HEADER_LEN + 12;
pub(super) const REPLY_BYTES: usize = 512;
pub(super) const REPLY_RECORDS: usize = 2;

pub(super) struct PendingNegotiation {
    stream: Option<UnixStream>,
    deadline: Instant,
    epoch: u64,
    policy: ShellContentAdmissionPolicy,
    hello: [u8; HELLO_BYTES],
    received: usize,
    reply: [u8; REPLY_BYTES],
    reply_len: usize,
    sent: usize,
    selected: Option<(ShellV1ServerWelcome, Option<ContentLimits>)>,
    refusal: Option<ContentAdmissionRefused>,
}

impl ShellComponentTransport {
    /// Start without accepting or contacting a peer. The owner must visit poll
    /// with a deadline and disconnect on abandonment. One pending handshake
    /// reserves two records/512 bytes plus a fixed Hello buffer; no ordinary
    /// traffic is admitted until these initial records have fully left it.
    pub fn begin_negotiation(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        connection_epoch: u64,
        timeout: Duration,
        policy: ShellContentAdmissionPolicy,
    ) -> Result<(), ShellTransportError> {
        if connection_epoch == 0 || connection_epoch <= self.connection_epoch {
            return Err(ShellTransportError::InvalidConnectionEpoch);
        }
        if self.negotiation.is_some() || self.stream.is_some() || self.content_grant.is_some() {
            return Err(ShellTransportError::WrongContentGrant);
        }
        if self.reserved_limits.as_ref().is_some_and(|limits| {
            limits.grant.connection_epoch != connection_epoch
                || epochs.resources(limits.grant).is_none()
        }) {
            return Err(ShellTransportError::WrongContentGrant);
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| ShellTransportError::Io("negotiation deadline out of range".into()))?;
        self.negotiation = Some(PendingNegotiation {
            stream: None,
            deadline,
            epoch: connection_epoch,
            policy,
            hello: [0; HELLO_BYTES],
            received: 0,
            reply: [0; REPLY_BYTES],
            reply_len: 0,
            sent: 0,
            selected: None,
            refusal: None,
        });
        Ok(())
    }

    /// At most one accept and 32 read/write attempts, sharing at most 64 KiB
    /// with this visit's byte budget. Zero bytes performs no socket operation.
    /// None means still pending, not connected. Welcome is returned exactly
    /// once, after all welcome/limit bytes have been written (not peer receipt).
    /// On returned failure, the exact reservation/socket is revoked; neighbors
    /// remain owned by the registry. Interruption/unwind is not a terminal ACK.
    pub fn poll_negotiation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        byte_budget: usize,
    ) -> Result<Option<ShellV1ServerWelcome>, ShellTransportError> {
        let result = self.visit_negotiation(epochs, byte_budget.min(64 * 1024));
        if result.is_err() && self.negotiation.is_some() {
            let _ = self.disconnect(epochs);
        }
        result
    }

    fn visit_negotiation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        mut budget: usize,
    ) -> Result<Option<ShellV1ServerWelcome>, ShellTransportError> {
        let pending = self
            .negotiation
            .as_mut()
            .ok_or(ShellTransportError::NotConnected)?;
        if Instant::now() >= pending.deadline {
            return Err(PolicyRoleEndpointError::AcceptTimedOut.into());
        }
        if budget == 0 {
            return Ok(None);
        }
        if pending.stream.is_none() {
            let Some(stream) = self.endpoint.poll_expected()? else {
                return Ok(None);
            };
            // Store before the fallible mode change. Returned-error cleanup owns it.
            pending.stream = Some(stream);
            pending
                .stream
                .as_ref()
                .expect("accepted stream retained")
                .set_nonblocking(true)
                .map_err(io_error)?;
        }
        for _ in 0..32 {
            if budget == 0 {
                break;
            }
            let pending = self.negotiation.as_mut().expect("visit retains handshake");
            if pending.received < HELLO_BYTES {
                let end = if pending.received < SOPHIA_IPC_HEADER_LEN {
                    SOPHIA_IPC_HEADER_LEN
                } else {
                    HELLO_BYTES
                };
                let end = end.min(pending.received + budget);
                let read = pending
                    .stream
                    .as_mut()
                    .expect("accepted stream retained")
                    .read(&mut pending.hello[pending.received..end]);
                match read {
                    Ok(0) => return Err(ShellTransportError::Io("shell Hello EOF".into())),
                    Ok(count) => {
                        pending.received += count;
                        budget -= count;
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(io_error(error)),
                }
                if pending.received == SOPHIA_IPC_HEADER_LEN {
                    let length = u32::from_le_bytes(
                        pending.hello[16..20].try_into().expect("header length"),
                    );
                    if length != 12 {
                        return Err(ShellTransportError::Io(
                            "shell Hello payload length must be 12".into(),
                        ));
                    }
                }
                if pending.received < HELLO_BYTES {
                    continue;
                }
            }
            if pending.reply_len == 0 {
                let hello = decode_shell_v1_client_hello_frame(&pending.hello)?;
                let epoch = pending.epoch;
                let policy = pending.policy;
                // Admission is retained in the actual registry before encoding.
                let selected = self.select_negotiation(epochs, epoch, policy, hello);
                let pending = self.negotiation.as_mut().expect("visit retains handshake");
                let bytes = match selected {
                    Ok((welcome, limits)) => {
                        pending.selected = Some((welcome, limits));
                        let mut bytes = encode_shell_v1_server_welcome_frame(welcome)?;
                        if let Some(limits) = &pending.selected.as_ref().expect("selected").1 {
                            bytes.extend(sophia_protocol::encode_shell_content_frame(
                                TransactionId::INVALID,
                                &sophia_protocol::ShellContentRecord::Limits(limits.clone()),
                            )?);
                        }
                        bytes
                    }
                    Err(ShellTransportError::ContentAdmissionRefused(refusal)) => {
                        pending.refusal = Some(refusal.clone());
                        sophia_protocol::encode_shell_content_frame(
                            TransactionId::INVALID,
                            &sophia_protocol::ShellContentRecord::AdmissionRefused(refusal),
                        )?
                    }
                    Err(error) => return Err(error),
                };
                if bytes.len() > REPLY_BYTES {
                    return Err(ShellTransportError::ContentQueueSaturated);
                }
                pending.reply[..bytes.len()].copy_from_slice(&bytes);
                pending.reply_len = bytes.len();
            }
            if budget == 0 {
                return Ok(None);
            }
            let pending = self.negotiation.as_mut().expect("visit retains handshake");
            let end = pending.reply_len.min(pending.sent + budget);
            let written = pending
                .stream
                .as_mut()
                .expect("accepted stream retained")
                .write(&pending.reply[pending.sent..end]);
            match written {
                Ok(0) => return Err(ShellTransportError::Io("shell welcome write zero".into())),
                Ok(count) => {
                    pending.sent += count;
                    budget -= count;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(io_error(error)),
            }
            if pending.sent == pending.reply_len {
                if let Some(refusal) = &pending.refusal {
                    return Err(ShellTransportError::ContentAdmissionRefused(
                        refusal.clone(),
                    ));
                }
                // No fallible operation after removing the exact handshake owner.
                let pending = self.negotiation.take().expect("completed handshake");
                let (welcome, limits) = pending.selected.expect("successful selection");
                self.pending_activations.clear();
                self.last_candidate_generation = 0;
                self.requested_candidate = None;
                self.pending_candidate = None;
                self.presented_candidate = None;
                self.connection_epoch = welcome.connection_epoch;
                self.content_grant = limits.as_ref().map(|limits| limits.grant);
                self.content_limits = limits;
                self.reserved_limits = None;
                self.peer_closed = false;
                self.input.clear();
                self.output.clear();
                self.action_cancellations.clear();
                self.indicator_response = None;
                self.catalog_response = None;
                self.native_control = super::native_launcher::control::NativeControl::default();
                self.inbox.clear();
                self.capabilities = welcome.capabilities;
                self.stream = pending.stream;
                return Ok(Some(welcome));
            }
        }
        Ok(None)
    }
}

fn io_error(error: io::Error) -> ShellTransportError {
    ShellTransportError::Io(error.to_string())
}
