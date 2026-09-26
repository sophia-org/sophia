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
    /// Session selected the file wire for this component at startup.
    file: bool,
    wire: Option<super::files::ShellFileWire>,
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
        self.begin_selected_negotiation(epochs, connection_epoch, timeout, policy, false)
    }

    /// As [`Self::begin_negotiation`], over `sophia_shell_fs_v1`: the admitted
    /// stream is served as this component's 9P export, and negotiation is
    /// its one submitted Negotiate record. Nothing here changes admission.
    pub fn begin_file_negotiation(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        connection_epoch: u64,
        timeout: Duration,
        policy: ShellContentAdmissionPolicy,
    ) -> Result<(), ShellTransportError> {
        self.begin_selected_negotiation(epochs, connection_epoch, timeout, policy, true)
    }

    fn begin_selected_negotiation(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        connection_epoch: u64,
        timeout: Duration,
        policy: ShellContentAdmissionPolicy,
        file: bool,
    ) -> Result<(), ShellTransportError> {
        if connection_epoch == 0 || connection_epoch <= self.connection_epoch {
            return Err(ShellTransportError::InvalidConnectionEpoch);
        }
        if self.negotiation.is_some()
            || self.stream.is_some()
            || self.files.is_some()
            || self.content_grant.is_some()
        {
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
            file,
            wire: None,
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
        if pending.stream.is_none() && pending.wire.is_none() {
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
        if pending.file {
            return self.visit_file_negotiation(epochs);
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
                self.install_negotiated(welcome, limits);
                self.stream = pending.stream;
                return Ok(Some(welcome));
            }
        }
        Ok(None)
    }

    /// Resets connection state for a freshly selected epoch. Both wires use
    /// it after their last fallible step; the caller installs its wire.
    fn install_negotiated(&mut self, welcome: ShellV1ServerWelcome, limits: Option<ContentLimits>) {
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
    }

    /// One nonblocking turn of the pending file export. The accepted stream
    /// becomes the export's single connection on the first visit. A refusal
    /// is journaled and the component is revoked once the peer acknowledged
    /// it, or at the negotiation deadline, whichever comes first.
    fn visit_file_negotiation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<ShellV1ServerWelcome>, ShellTransportError> {
        let profile = epochs.profile(self.store_grant);
        let pending = self.negotiation.as_mut().expect("visit retains handshake");
        if pending.wire.is_none() {
            let stream = pending.stream.take().expect("accepted stream retained");
            let (role, bounds) = super::files::role_bounds(profile);
            let export = super::files::ShellFiles::awaiting_negotiation(
                pending.epoch,
                role,
                bounds,
                self.file_qids,
                Instant::now(),
            );
            pending.wire = Some(super::files::ShellFileWire::adopt(stream, export)?);
        }
        let wire = pending.wire.as_mut().expect("adopted file wire");
        wire.turn()?;
        if let Some(refusal) = &pending.refusal {
            if wire.export().journal().records() == 0 {
                return Err(ShellTransportError::ContentAdmissionRefused(
                    refusal.clone(),
                ));
            }
            return Ok(None);
        }
        let hello = match wire.export_mut().take_inbound() {
            None => return Ok(None),
            Some(super::files::Inbound::Negotiate(hello)) => hello,
            Some(super::files::Inbound::Content(..)) => {
                return Err(ShellTransportError::WrongContentRecord);
            }
        };
        let epoch = pending.epoch;
        let policy = pending.policy;
        // Admission is retained in the actual registry before encoding.
        let selected = self.select_negotiation(epochs, epoch, policy, hello);
        let pending = self.negotiation.as_mut().expect("visit retains handshake");
        let wire = pending.wire.as_mut().expect("adopted file wire");
        match selected {
            Ok((welcome, limits)) => {
                let limits_object = limits
                    .as_ref()
                    .map(|limits| super::files::encode_limits_object(epoch, limits))
                    .transpose()?;
                let body = super::files::encode_negotiated(welcome, limits.is_some())?;
                wire.export_mut()
                    .complete_negotiation(limits_object)
                    .map_err(|error| ShellTransportError::Io(format!("{error:?}")))?;
                if !wire.append(
                    sophia_protocol::shell_files::ShellFileKind::Negotiated,
                    &body,
                    true,
                )? {
                    return Err(ShellTransportError::ContentQueueSaturated);
                }
                wire.turn()?;
                // No fallible operation after removing the exact handshake owner.
                let pending = self.negotiation.take().expect("completed handshake");
                self.install_negotiated(welcome, limits);
                self.files = pending.wire;
                Ok(Some(welcome))
            }
            Err(ShellTransportError::ContentAdmissionRefused(refusal)) => {
                let body = super::files::encode_refused(&refusal)?;
                if !wire.append(
                    sophia_protocol::shell_files::ShellFileKind::Refused,
                    &body,
                    true,
                )? {
                    return Err(ShellTransportError::ContentQueueSaturated);
                }
                wire.turn()?;
                pending.refusal = Some(refusal);
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}

impl PendingNegotiation {
    /// The next logical qid of an adopted but unfinished file export, so a
    /// following epoch never reuses a qid this peer already observed.
    pub(super) fn file_qids(&self) -> Option<u64> {
        self.wire.as_ref().map(|wire| wire.export().next_qid())
    }
}

fn io_error(error: io::Error) -> ShellTransportError {
    ShellTransportError::Io(error.to_string())
}
